pub mod document;
pub mod event;
pub mod forwards;
pub mod metrics;
pub mod selection;
pub mod session;
pub mod state;
pub mod terminal;

use crate::{
    command::{Action, Command, Keymap, ResourceCommand},
    config::Config,
    kube::{ConnectOptions, Connection, discovery::Resource, watch::Query},
    resources::store::Store,
};
use anyhow::{Context, Result};
use crossterm::event::{Event as TerminalEvent, EventStream, KeyCode, KeyEventKind, KeyModifiers};
use event::{Event, Payload};
use futures_util::StreamExt;
use state::{Document, Mode, State};
use std::{io::IsTerminal, time::Duration};
use tokio::{sync::mpsc, task::JoinSet};
use tokio_util::sync::CancellationToken;

pub struct Runtime {
    pub state: State,
    pub connection: Option<Connection>,
    options: ConnectOptions,
    config: Config,
    config_path: Option<std::path::PathBuf>,
    tx: mpsc::Sender<Event>,
    tasks: JoinSet<()>,
    sessions: session::Sessions,
    forwards: forwards::Forwards,
    pending_forward: Option<(Connection, crate::resources::SharedObject)>,
    metrics_due: std::time::Instant,
    metrics_inflight: bool,
    metrics_generation: u64,
    metrics_failures: u32,
    scope: CancellationToken,
    document: CancellationToken,
    pending: Option<ResourceCommand>,
    pending_history: Option<state::HistoryEntry>,
    /// Set by `open_shell()`/`open_attach()`, consumed by `run()` right after each
    /// key/event dispatch. Both are foreground and blocking -- unlike logs/exec
    /// output, nothing else in the app runs concurrently with either -- so neither
    /// goes through `Sessions`; `run()` itself owns suspending/resuming the
    /// terminal (see `app::terminal::TerminalHandoff`) around the one call that
    /// runs whichever is pending.
    pending_interactive: Option<Interactive>,
    palette_document: Option<Document>,
    namespace_by_context: std::collections::HashMap<String, Option<String>>,
    recent_namespaces: std::collections::VecDeque<String>,
    history: std::collections::VecDeque<state::HistoryEntry>,
    forward: std::collections::VecDeque<state::HistoryEntry>,
}
/// Back-stack cap: fixed from the start so it can never grow unbounded across a long
/// session. 100 is generous for a "how did I get here" trail without being a memory
/// concern -- each entry is a handful of short strings, never store/rows data.
const HISTORY_LIMIT: usize = 100;
/// Either kind of foreground, blocking, terminal-owning session `run()` can run.
enum Interactive {
    Shell(crate::kube::exec::ShellRequest),
    Attach(crate::kube::exec::AttachRequest),
}
impl Runtime {
    pub fn new(
        options: ConnectOptions,
        config: Config,
        config_path: Option<std::path::PathBuf>,
        query: Query,
    ) -> Result<(Self, mpsc::Receiver<Event>)> {
        let settings = config.resolve("", "")?;
        let state = State::new(query, &settings)?;
        let (tx, rx) = mpsc::channel(256);
        Ok((
            Self {
                state,
                connection: None,
                options,
                config,
                config_path,
                tx,
                tasks: JoinSet::new(),
                sessions: session::Sessions::default(),
                forwards: forwards::Forwards::default(),
                pending_forward: None,
                metrics_due: std::time::Instant::now(),
                metrics_inflight: false,
                metrics_generation: 0,
                metrics_failures: 0,
                scope: CancellationToken::new(),
                document: CancellationToken::new(),
                pending: None,
                pending_history: None,
                pending_interactive: None,
                palette_document: None,
                namespace_by_context: std::collections::HashMap::new(),
                recent_namespaces: std::collections::VecDeque::new(),
                history: std::collections::VecDeque::new(),
                forward: std::collections::VecDeque::new(),
            },
            rx,
        ))
    }
    /// Switch to a different context, restoring that context's last-viewed namespace
    /// if this session has visited it before, instead of always resetting to the
    /// kubeconfig default. Remembers the outgoing context's current namespace first.
    fn switch_context(&mut self, context: String) {
        self.pending = None;
        self.pending_history = None;
        self.push_history();
        if let Some(current) = self.connection.as_ref().map(|c| c.context.clone()) {
            self.namespace_by_context
                .insert(current, self.state.query.namespace.clone());
        }
        self.state.query.namespace = namespace_for_context(&self.namespace_by_context, &context);
        self.connect(Some(context));
    }
    /// Switch the visible namespace (`None` is all-namespaces) and remember it in the
    /// recents list shown at the top of the namespace picker on future opens.
    fn switch_namespace(&mut self, namespace: Option<String>) -> Result<()> {
        self.push_history();
        if let Some(ns) = &namespace {
            self.recent_namespaces.retain(|n| n != ns);
            self.recent_namespaces.push_front(ns.clone());
            self.recent_namespaces.truncate(5);
        }
        self.state.query.namespace = namespace;
        self.state.mode = Mode::Table;
        // If a just-issued context switch's connect is still in flight, self.connection
        // is briefly None and watch() fails with "Not connected"; the namespace is
        // already recorded in state.query above, so Payload::Connected's rewatch()
        // fallback re-applies it correctly once the connection lands. Surfacing that
        // transient failure here would instead reopen the palette with this command's
        // text, silently absorbing subsequent keystrokes as edits to it (see navigate()).
        let _ = self.watch();
        Ok(())
    }
    /// Snapshot the current navigation *intent* (never store/rows) onto the back stack
    /// and drop any forward stack, matching standard back/forward semantics: taking a
    /// new path invalidates the old "redo". A no-op (not yet connected, or no resource
    /// resolved yet -- e.g. still on the very first connect) pushes nothing.
    fn push_history(&mut self) {
        let Some(entry) = self.current_history_entry() else {
            return;
        };
        push_capped(&mut self.history, entry, HISTORY_LIMIT);
        self.forward.clear();
    }
    /// The current view as a `HistoryEntry`, or `None` before anything is resolved yet
    /// (e.g. still on the very first connect) -- there is nothing meaningful to record.
    fn current_history_entry(&self) -> Option<state::HistoryEntry> {
        let connection = self.connection.as_ref()?;
        let resource = self.state.resource.as_ref()?;
        Some(state::HistoryEntry {
            context: connection.context.clone(),
            namespace: self.state.query.namespace.clone(),
            resource: resource.clone(),
            labels: self.state.query.labels.clone(),
            fields: self.state.query.fields.clone(),
            filter_text: self.state.filter_text.clone(),
            sort: self.state.sort.clone(),
            descending: self.state.descending,
            selected: self.state.selected.clone(),
        })
    }
    /// Restore a history entry's navigation intent: reconnect only if its context
    /// differs from the current one, re-resolve `resource` (a canonical qualified name)
    /// against whatever catalog ends up current, then start a fresh watch. Never
    /// revives old rows -- the normal watch/rebuild pipeline repopulates real data, and
    /// the existing UID-not-found-clears-selection invariant in `rebuild()` handles a
    /// selection that no longer exists (or was replaced by a same-named different UID).
    fn apply_history(&mut self, entry: state::HistoryEntry) {
        self.pending = None;
        self.pending_history = None;
        if self
            .connection
            .as_ref()
            .is_some_and(|c| c.context == entry.context)
        {
            self.finish_history(entry, false);
        } else {
            let context = entry.context.clone();
            self.pending_history = Some(entry);
            self.connect(Some(context));
        }
    }
    fn finish_history(&mut self, entry: state::HistoryEntry, crossed_catalog: bool) {
        let result = (|| -> Result<()> {
            let connection = self.connection.as_ref().context("Not connected")?;
            let resource = if crossed_catalog {
                connection
                    .catalog
                    .resolve(&entry.resource.id(), &connection.settings.aliases)?
            } else {
                entry.resource.clone()
            };
            let filter = crate::filters::Expr::parse(&entry.filter_text)?;
            // Keep query.resource (the text rewatch()/a later context switch re-resolves)
            // in sync with what was actually just restored -- otherwise it would still
            // hold whatever was typed before this restore, and a subsequent context
            // switch would re-resolve THAT stale text instead of the resource just
            // returned to.
            self.state.query.resource = resource.id();
            self.state.query.namespace = entry.namespace;
            self.state.query.labels = entry.labels;
            self.state.query.fields = entry.fields;
            self.state.filter_text = entry.filter_text;
            self.state.filter = filter;
            self.state.sort = entry.sort;
            self.state.descending = entry.descending;
            self.watch_resource(resource)?;
            // Set after watch_resource: cancel_scope() unconditionally clears selection,
            // so the desired UID must be applied afterward, then validated by the normal
            // rebuild() once the fresh list arrives (existing invariant, not duplicated).
            self.state.selected = entry.selected;
            Ok(())
        })();
        if let Err(e) = result {
            self.state.error = Some(format!("Could not restore history entry: {e}"));
        }
    }
    /// Fetch the real namespace list from the cluster (bounded to 500, same pattern as
    /// Events) and open it as a picker, instead of navigating away to a namespaces
    /// resource table and losing whatever resource was on screen. `<all>` is always
    /// first; recently-visited namespaces (this context, this session) come next.
    fn open_namespace_picker(&mut self) -> Result<()> {
        let connection = self.connection.clone().context("Not connected yet")?;
        let resource = connection
            .catalog
            .resolve("namespaces", &connection.settings.aliases)?;
        self.document.cancel();
        self.document = self.scope.child_token();
        self.state.request += 1;
        let request = self.state.request;
        let epoch = self.state.epoch;
        let tx = self.tx.clone();
        let cancel = self.document.clone();
        self.state.mode = Mode::Loading;
        self.state.status = "Listing namespaces…".into();
        self.tasks.spawn(async move {
            let result = tokio::select! {
                biased;
                _=cancel.cancelled()=>return,
                result=crate::kube::discovery::list_names(&connection, &resource)=>result,
            };
            let payload = match result {
                Ok((names, truncated)) => Payload::NamespaceList {
                    request,
                    names,
                    truncated,
                },
                Err(e) => Payload::DocumentError {
                    request,
                    error: e.to_string(),
                },
            };
            tokio::select! {_=cancel.cancelled()=>{},_=tx.send(Event{epoch,payload})=>{}}
        });
        Ok(())
    }
    pub fn connect(&mut self, context: Option<String>) {
        self.recent_namespaces.clear();
        self.cancel_scope();
        self.connection = None;
        self.state.resource = None;
        if context.is_some() {
            self.options.context = context;
        }
        self.state.context = self
            .options
            .context
            .clone()
            .unwrap_or_else(|| "current kubeconfig context".into());
        self.state.status = "Connecting and discovering APIs…".into();
        let options = self.options.clone();
        let config = self.config.clone();
        let tx = self.tx.clone();
        let cancel = self.scope.clone();
        let epoch = self.state.epoch;
        self.tasks.spawn(async move{
            let result=tokio::select!{biased;_=cancel.cancelled()=>return,result=crate::kube::connect(options,config)=>result};
            let payload=match result{Ok(connection)=>Payload::Connected(Box::new(connection)),Err(error)=>Payload::ConnectError(error.to_string())};
            tokio::select!{_=cancel.cancelled()=>{},_=tx.send(Event{epoch,payload})=>{}}
        });
    }
    fn cancel_scope(&mut self) {
        self.state.metrics = metrics::Cache::default();
        self.metrics_due = std::time::Instant::now();
        self.metrics_inflight = false;
        self.metrics_failures = 0;
        self.pending_forward = None;
        self.palette_document = None;
        self.scope.cancel();
        self.document.cancel();
        self.scope = CancellationToken::new();
        self.document = self.scope.child_token();
        self.state.epoch += 1;
        self.state.request += 1;
        self.state.selected = None;
        // M10.1: the bulk multi-select set is scoped to the view it was
        // made in -- a context/resource-kind/namespace switch (every one
        // of which funnels through cancel_scope) clears it wholesale,
        // exactly like the single-select `selected` field above, rather
        // than letting it silently survive into a different scope.
        self.state.selection.clear();
        self.state.autoselect = true;
        self.state.rows.clear();
        self.state.filter_unknown = 0;
        self.state.store = Store::new(
            self.state.settings.max_objects,
            self.state.settings.max_bytes,
        );
        self.state.mode = Mode::Table;
        self.state.synced = false;
        self.state.printer_columns.clear();
        self.state.dirty = true;
    }
    /// Start (or restart) the watch for an already-resolved canonical `resource` (GVK).
    /// Callers must supply the identity explicitly rather than have it re-derived from a
    /// human alias here, so that a name disambiguated once (e.g. an ambiguous shortname,
    /// or one that collided with a CRD) cannot silently re-resolve to something else on a
    /// later Refresh/namespace switch just because the catalog changed in between.
    fn watch_resource(&mut self, resource: Resource) -> Result<()> {
        anyhow::ensure!(
            resource.verbs.iter().any(|v| v == "watch"),
            "Resource API does not advertise watch; snapshot polling is not implemented"
        );
        let connection = self.connection.clone().context("Not connected")?;
        self.cancel_scope();
        self.state.query.resource = resource.id();
        self.state.resource = Some(resource.clone());
        self.state.status = format!("Listing {}…", resource.qualified());
        let epoch = self.state.epoch;
        let tx = self.tx.clone();
        let cancel = self.scope.clone();
        let query = self.state.query.clone();
        // Best-effort CRD printer-column enrichment: one bounded read, independent of
        // and in parallel with the watch itself (see kube::printer for why this isn't
        // tied to the watch loop). A failure here is silently dropped -- generic/
        // curated columns already work without it; see Payload::PrinterColumns.
        let printer_connection = connection.clone();
        let printer_resource = resource.clone();
        let printer_tx = tx.clone();
        let printer_cancel = cancel.clone();
        self.tasks.spawn(async move {
            if let Ok(columns) =
                crate::kube::printer::fetch(&printer_connection, &printer_resource).await
            {
                tokio::select! {
                    _=printer_cancel.cancelled()=>{},
                    _=printer_tx.send(Event{epoch,payload:Payload::PrinterColumns(columns)})=>{},
                }
            }
        });
        self.tasks.spawn(crate::kube::watch::run(
            connection, resource, query, epoch, tx, cancel,
        ));
        Ok(())
    }
    /// Re-run the watch for whatever resource is already active (Refresh, namespace and
    /// all-namespaces switches) by reusing its resolved identity, never by re-resolving
    /// `state.query.resource`'s human text against the catalog again.
    pub fn watch(&mut self) -> Result<()> {
        let resource = self
            .state
            .resource
            .clone()
            .context("No resource selected yet")?;
        self.watch_resource(resource)
    }
    /// After a context switch with no explicit pending navigation, the previously
    /// resolved `state.resource` belongs to the OLD cluster's catalog and must not be
    /// reused: crossing to a different cluster is exactly the case where re-resolving
    /// the same human name is correct, since its GVK metadata (verbs, scope,
    /// shortnames) can legitimately differ there.
    fn rewatch(&mut self) -> Result<()> {
        let connection = self.connection.as_ref().context("Not connected")?;
        let resource = connection
            .catalog
            .resolve(&self.state.query.resource, &connection.settings.aliases)?;
        self.watch_resource(resource)
    }
    pub fn reduce(&mut self, event: Event) {
        if event.epoch != self.state.epoch {
            return;
        }
        self.state.dirty = true;
        match event.payload {
            Payload::Metrics {
                generation,
                pins,
                result,
            } => {
                if generation != self.metrics_generation {
                    return;
                }
                self.metrics_inflight = false;
                self.metrics_failures = if result.is_err() {
                    self.metrics_failures.saturating_add(1)
                } else {
                    0
                };
                let delay = if self.metrics_failures == 0 {
                    crate::kube::metrics::POLL
                } else {
                    Duration::from_secs(15 * (1_u64 << self.metrics_failures.min(3)))
                };
                self.metrics_due = std::time::Instant::now() + delay;
                let result = if self.state.synced {
                    result
                } else {
                    Err(crate::evidence::Unknown::Stale)
                };
                self.state.metrics.apply(pins, result, &self.state.store);
            }
            Payload::Connected(connection) => {
                // Connection completion starts a fresh watch, which clears old view
                // state. A palette opened *during* connection is new user input,
                // not abandoned-scope data: keep its buffer across that reset.
                // Otherwise the remaining typed letters become table shortcuts.
                let pending_input = if matches!(self.state.mode, Mode::Command(_)) {
                    Some(std::mem::replace(&mut self.state.mode, Mode::Table))
                } else {
                    None
                };
                self.state.context = connection.context.clone();
                self.state.settings = connection.settings.clone();
                match Keymap::compile(&connection.settings.keys) {
                    Ok(k) => self.state.keymap = k,
                    Err(e) => {
                        self.state.error = Some(e.to_string());
                    }
                }
                if self.state.query.namespace.as_deref() == Some("") {
                    self.state.query.namespace = Some(connection.namespace.clone());
                }
                self.connection = Some(*connection);
                if let Some(entry) = self.pending_history.take() {
                    self.finish_history(entry, true);
                } else if let Some(query) = self.pending.take() {
                    if let Err(e) = self.navigate(query) {
                        self.state.error = Some(e.to_string());
                    }
                } else if let Err(e) = self.rewatch() {
                    self.state.error = Some(e.to_string());
                }
                if let Some(input) = pending_input {
                    self.state.mode = input;
                }
            }
            Payload::ConnectError(e) => {
                self.state.status = "Connection failed".into();
                self.state.error = Some(e);
            }
            Payload::Begin => {
                self.state.metrics.apply(
                    Default::default(),
                    Err(crate::evidence::Unknown::Stale),
                    &self.state.store,
                );
                self.state.store.begin();
                self.state.synced = false;
                self.state.status = "Synchronizing list (previous rows may be stale)…".into();
            }
            Payload::Apply(object, initial) => {
                self.state.store.apply(object, initial);
            }
            Payload::Delete(object) => self.state.store.delete(&object),
            Payload::Ready => {
                self.state.store.finish();
                self.state.synced = true;
                self.state.error = None;
                self.state.status = "List synchronized · watching for changes".into();
            }
            Payload::WatchError(error) => {
                self.state.metrics.apply(
                    Default::default(),
                    Err(crate::evidence::Unknown::Stale),
                    &self.state.store,
                );
                self.state.watch_errors += 1;
                self.state.synced = false;
                self.state.error = Some(error);
                self.state.status = "STALE · recovering watch".into();
            }
            Payload::Document {
                request,
                title,
                text,
                adjacent,
            } if request == self.state.request => {
                if let Some(doc) = self.active_document_mut() {
                    doc.title = title;
                    doc.replace(text);
                    doc.adjacent = adjacent;
                    doc.adjacent_selected = 0;
                    doc.freshness = document::Freshness::Snapshot(chrono::Utc::now());
                }
            }
            Payload::MutationDryRun { request, outcome } if request == self.state.request => {
                if let Some(doc) = self.active_document_mut()
                    && let Some(workflow) = doc.workflow.as_mut()
                {
                    workflow.dry_run = Some(outcome);
                    let text = crate::mutation::view::workflow_report(workflow);
                    doc.replace(text);
                }
            }
            Payload::MutationCommit {
                request,
                outcome,
                verification,
            } if request == self.state.request => {
                if let Some(doc) = self.active_document_mut()
                    && let Some(workflow) = doc.workflow.as_mut()
                {
                    workflow.commit = Some(outcome);
                    workflow.verification = verification;
                    let text = crate::mutation::view::workflow_report(workflow);
                    doc.replace(text);
                }
            }
            Payload::DrainPlanned { request, planned } if request == self.state.request => {
                if let Some(doc) = self.active_document_mut()
                    && let Some(drain) = doc.drain.as_mut()
                {
                    drain.preview = match planned {
                        Ok(p) => crate::mutation::drain::DrainPreview::Ready(p),
                        Err(e) => crate::mutation::drain::DrainPreview::Failed(e),
                    };
                    let text = crate::mutation::view::drain_report(drain);
                    doc.replace(text);
                }
            }
            Payload::DrainCommit { request, report } if request == self.state.request => {
                if let Some(doc) = self.active_document_mut()
                    && let Some(drain) = doc.drain.as_mut()
                {
                    drain.report = Some(report);
                    let text = crate::mutation::view::drain_report(drain);
                    doc.replace(text);
                }
            }
            Payload::BulkCommit { request, results } if request == self.state.request => {
                if let Some(doc) = self.active_document_mut()
                    && let Some(bulk) = doc.bulk.as_mut()
                {
                    bulk.results = Some(results);
                    let text = crate::mutation::view::bulk_report(bulk);
                    doc.replace(text);
                }
            }
            Payload::DocumentError { request, error } if request == self.state.request => {
                if let Some(doc) = self.active_document_mut() {
                    doc.replace(String::new());
                    doc.adjacent = vec![];
                    doc.freshness = document::Freshness::Error(error);
                } else {
                    self.state.error = Some(error);
                    self.state.mode = Mode::Table;
                }
            }
            Payload::LogLine {
                request,
                session,
                line,
            } if request == self.state.request => {
                if let Some(doc) = self.active_document_mut()
                    && doc.session == Some(session)
                {
                    doc.append(line);
                }
            }
            Payload::LogStarted { request, session } if request == self.state.request => {
                self.sessions.running(session);
                if let Some(status) = self.sessions.state(session)
                    && let Some(doc) = self.active_document_mut()
                    && doc.session == Some(session)
                {
                    doc.streaming = status == session::State::Running;
                    doc.session_state = Some(status);
                }
            }
            Payload::LogSourceError {
                request,
                session,
                message,
            } if request == self.state.request => {
                if let Some(doc) = self.active_document_mut()
                    && doc.session == Some(session)
                {
                    doc.append(format!("SOURCE ERROR: {message}"));
                    if doc.source_errors.len() < crate::kube::logs::MAX_SOURCES {
                        doc.source_errors.push(message);
                    }
                }
            }
            Payload::ExecStarted { request, session } if request == self.state.request => {
                self.sessions.running(session);
                if let Some(status) = self.sessions.state(session)
                    && let Some(doc) = self.active_document_mut()
                    && doc.session == Some(session)
                {
                    doc.streaming = status == session::State::Running;
                    doc.session_state = Some(status);
                }
            }
            Payload::ExecLine {
                request,
                session,
                line,
            } if request == self.state.request => {
                if let Some(doc) = self.active_document_mut()
                    && doc.session == Some(session)
                {
                    doc.append(line);
                }
            }
            Payload::NamespaceList {
                request,
                names,
                truncated,
            } if request == self.state.request => {
                let mut items = vec!["<all>".to_string()];
                for recent in &self.recent_namespaces {
                    if names.contains(recent) && !items.contains(recent) {
                        items.push(recent.clone());
                    }
                }
                let mut rest: Vec<String> =
                    names.into_iter().filter(|n| !items.contains(n)).collect();
                rest.sort();
                items.extend(rest);
                let active = match &self.state.query.namespace {
                    None => Some(0),
                    Some(ns) => items.iter().position(|i| i == ns),
                };
                self.state.status = if truncated {
                    "PARTIAL: namespace list limited to 500".into()
                } else {
                    "Namespace list ready".into()
                };
                self.state.mode = Mode::Picker(state::Picker {
                    kind: state::PickerKind::Namespace,
                    title: "Namespaces · Enter switches, Esc cancels".into(),
                    items,
                    active,
                    cursor: active.unwrap_or(0),
                });
            }
            Payload::PrinterColumns(columns) => {
                self.state.printer_columns = columns;
            }
            _ => {}
        }
    }
    fn navigate(&mut self, query: ResourceCommand) -> Result<()> {
        let filter = crate::filters::Expr::parse(query.filter.as_deref().unwrap_or(""))?;
        if let Some(context) = &query.context
            && self
                .connection
                .as_ref()
                .is_none_or(|c| &c.context != context)
        {
            self.push_history();
            self.state.query.namespace = Some("".into());
            self.pending = Some(query.clone());
            self.connect(Some(context.clone()));
            return Ok(());
        }
        // A connect from a just-issued context switch may still be in flight (self.
        // connection is briefly None between that switch and its Payload::Connected).
        // Queue this query instead of failing outright: erroring here would reopen the
        // palette with this query's text still in the edit buffer, silently absorbing
        // any further keystrokes the user types as edits to that stale buffer instead
        // of as the next command they intended. Payload::Connected applies `pending`
        // once the connection lands, exactly like the explicit-context-switch case above.
        let Some(connection) = self.connection.as_ref() else {
            self.pending = Some(query);
            return Ok(());
        };
        // Resolve once, here, from the human-typed name. This is the ONLY point that
        // should turn an alias/shortname into a canonical GVK: everything downstream
        // (Refresh, namespace switches) must reuse this resolved `Resource`, not the
        // string, so a name that was disambiguated once stays stable for this session
        // even if the catalog changes later (a CRD colliding with it appears/disappears).
        let resource = connection
            .catalog
            .resolve(&query.resource, &connection.settings.aliases)?;
        anyhow::ensure!(
            resource.verbs.iter().any(|v| v == "watch"),
            "Resource does not advertise watch"
        );
        self.push_history();
        if query.all {
            self.state.query.namespace = None;
        } else if let Some(ns) = query.namespace {
            self.state.query.namespace = if ns == "all" || ns == "*" {
                None
            } else {
                Some(ns)
            };
        }
        self.state.query.resource = resource.id();
        self.state.query.labels = query.labels;
        self.state.query.fields = query.fields;
        self.state.filter_text = query.filter.unwrap_or_default();
        self.state.filter = filter;
        self.state.sort = "NAME".into();
        self.state.descending = false;
        self.watch_resource(resource)
    }
    pub fn command(&mut self, text: &str) -> Result<()> {
        self.state.error = None;
        match crate::command::parse(text)? {
            Command::Action(action) => {
                // The palette can type any action regardless of mode, unlike a key
                // binding, which the keymap only ever matches for its own mode. Without
                // this check, typing e.g. :search_next while not viewing a document
                // would silently do nothing instead of the same "not available here"
                // feedback a mismatched key press would never even have a chance to
                // produce (since it isn't bound outside its mode in the first place).
                let mode_name = self.mode_name();
                if let Some(binding) = crate::command::registry()
                    .into_iter()
                    .find(|b| b.action == action)
                {
                    anyhow::ensure!(
                        crate::command::available(binding.mode, mode_name),
                        "{} is not available in {mode_name} mode",
                        binding.name
                    );
                }
                self.action(action)
            }
            Command::Resource(query) => {
                self.pending_history = None;
                self.pending = None;
                self.navigate(query)
            }
            Command::Namespace(Some(ns)) => self.switch_namespace(if ns == "all" || ns == "*" {
                None
            } else {
                Some(ns)
            }),
            Command::Namespace(None) => self.open_namespace_picker(),
            Command::Context(Some(context)) => {
                self.switch_context(context);
                Ok(())
            }
            Command::Context(None) => {
                let connection = self.connection.as_ref().context("Not connected")?;
                let active = connection
                    .contexts
                    .iter()
                    .position(|c| c == &connection.context);
                self.document.cancel();
                self.state.request += 1;
                self.state.mode = Mode::Picker(state::Picker {
                    kind: state::PickerKind::Context,
                    title: "Contexts · Enter switches, Esc cancels".into(),
                    items: connection.contexts.clone(),
                    active,
                    cursor: active.unwrap_or(0),
                });
                Ok(())
            }
            Command::Info => {
                self.open_static("Runtime diagnostics", self.info());
                Ok(())
            }
            Command::Reload => {
                let config = Config::load(self.config_path.as_deref())?;
                let mut settings = if let Some(c) = &self.connection {
                    config.resolve(&c.cluster, &c.context)?
                } else {
                    config.resolve("", "")?
                };
                settings.readonly |= self.options.force_readonly;
                let keymap = Keymap::compile(&settings.keys)?;
                // Validate all original scopes before changing any policy or task.
                let mut revoked = Vec::new();
                for (id, entry) in &self.forwards.entries {
                    if self.options.force_readonly
                        || config
                            .resolve(&entry.scope.cluster, &entry.scope.context)?
                            .readonly
                    {
                        revoked.push(*id);
                    }
                }
                self.config = config;
                for id in revoked {
                    self.sessions.stop(id);
                }
                self.state.settings = settings.clone();
                self.state.keymap = keymap;
                if let Some(connection) = self.connection.as_mut() {
                    connection.settings = settings;
                }
                self.state.status =
                    "Configuration reloaded; cache limits apply at next view change".into();
                Ok(())
            }
            Command::Logs {
                container,
                previous,
            } => self.open_logs(container, previous),
            Command::Sort(spec) => {
                let (column, direction) = match spec.rsplit_once(':') {
                    Some((column, direction @ ("asc" | "desc"))) => (column, direction),
                    _ => (spec.as_str(), "asc"),
                };
                let field = crate::filters::value::Field::parse(column)?;
                anyhow::ensure!(
                    field.key.starts_with("field:")
                        || self
                            .state
                            .columns()
                            .iter()
                            .any(|c| c.eq_ignore_ascii_case(column)),
                    "Unknown sort column"
                );
                self.state.sort = if field.key.starts_with("field:") {
                    column.into()
                } else {
                    column.to_uppercase()
                };
                self.state.descending = direction == "desc";
                self.state.dirty = true;
                Ok(())
            }
            Command::Exec { container, command } => {
                anyhow::ensure!(
                    !self.state.settings.readonly,
                    "Exec is unavailable in read-only mode"
                );
                self.open_exec(container, command)
            }
            Command::Shell { container, shell } => {
                anyhow::ensure!(
                    !self.state.settings.readonly,
                    "Shell is unavailable in read-only mode"
                );
                self.open_shell(container, shell)
            }
            Command::Attach { container } => {
                anyhow::ensure!(
                    !self.state.settings.readonly,
                    "Attach is unavailable in read-only mode"
                );
                self.open_attach(container)
            }
            Command::Forward(ports) => {
                anyhow::ensure!(
                    !self.options.force_readonly && !self.state.settings.readonly,
                    "Port forwarding is unavailable in read-only mode"
                );
                let connection = self.connection.clone().context("Not connected")?;
                let object = self.state.selected_object().context("Select a Pod first")?;
                self.start_forward(connection, object, ports)
            }
            Command::Scale(replicas) => {
                let object = self.state.selected_object().context("Select a row first")?;
                let resource = self
                    .state
                    .resource
                    .clone()
                    .context("No resource selected")?;
                let current = object
                    .value
                    .pointer("/spec/replicas")
                    .and_then(serde_json::Value::as_i64);
                let scope = self.mutation_scope(&object, &resource)?;
                let request_id = scope.request;
                let built = crate::mutation::workflow::scale(
                    scope, resource, current, replicas, request_id,
                )
                .map_err(|e| anyhow::anyhow!(e))?;
                self.open_workflow_document(format!("Scale: {}", object.name), built)
            }
            Command::Restart => {
                let object = self.state.selected_object().context("Select a row first")?;
                let resource = self
                    .state
                    .resource
                    .clone()
                    .context("No resource selected")?;
                let scope = self.mutation_scope(&object, &resource)?;
                let request_id = scope.request;
                // Generated exactly once here, when the intent is first built --
                // never regenerated on a later render of the same workflow.
                let timestamp = chrono::Utc::now().to_rfc3339();
                let built =
                    crate::mutation::workflow::restart(scope, resource, &timestamp, request_id)
                        .map_err(|e| anyhow::anyhow!(e))?;
                self.open_workflow_document(format!("Restart: {}", object.name), built)
            }
            Command::Delete => {
                let object = self.state.selected_object().context("Select a row first")?;
                let resource = self
                    .state
                    .resource
                    .clone()
                    .context("No resource selected")?;
                let scope = self.mutation_scope(&object, &resource)?;
                let request_id = scope.request;
                let built = crate::mutation::workflow::delete(scope, resource, request_id)
                    .map_err(|e| anyhow::anyhow!(e))?;
                self.open_workflow_document(format!("Delete: {}", object.name), built)
            }
            Command::Label { key, value } => {
                let object = self.state.selected_object().context("Select a row first")?;
                let resource = self
                    .state
                    .resource
                    .clone()
                    .context("No resource selected")?;
                let scope = self.mutation_scope(&object, &resource)?;
                let request_id = scope.request;
                let built = crate::mutation::workflow::label(
                    scope,
                    resource,
                    &key,
                    value.as_deref(),
                    request_id,
                )
                .map_err(|e| anyhow::anyhow!(e))?;
                self.open_workflow_document(format!("Label: {}", object.name), built)
            }
            Command::Annotate { key, value } => {
                let object = self.state.selected_object().context("Select a row first")?;
                let resource = self
                    .state
                    .resource
                    .clone()
                    .context("No resource selected")?;
                let scope = self.mutation_scope(&object, &resource)?;
                let request_id = scope.request;
                let built = crate::mutation::workflow::annotate(
                    scope,
                    resource,
                    &key,
                    value.as_deref(),
                    request_id,
                )
                .map_err(|e| anyhow::anyhow!(e))?;
                self.open_workflow_document(format!("Annotate: {}", object.name), built)
            }
            Command::Cordon => {
                let object = self.state.selected_object().context("Select a row first")?;
                let resource = self
                    .state
                    .resource
                    .clone()
                    .context("No resource selected")?;
                let scope = self.mutation_scope(&object, &resource)?;
                let request_id = scope.request;
                let built = crate::mutation::workflow::cordon(scope, resource, request_id)
                    .map_err(|e| anyhow::anyhow!(e))?;
                self.open_workflow_document(format!("Cordon: {}", object.name), built)
            }
            Command::Uncordon => {
                let object = self.state.selected_object().context("Select a row first")?;
                let resource = self
                    .state
                    .resource
                    .clone()
                    .context("No resource selected")?;
                let scope = self.mutation_scope(&object, &resource)?;
                let request_id = scope.request;
                let built = crate::mutation::workflow::uncordon(scope, resource, request_id)
                    .map_err(|e| anyhow::anyhow!(e))?;
                self.open_workflow_document(format!("Uncordon: {}", object.name), built)
            }
            Command::SetImage { container, image } => {
                let object = self.state.selected_object().context("Select a row first")?;
                let resource = self
                    .state
                    .resource
                    .clone()
                    .context("No resource selected")?;
                let containers = object
                    .value
                    .pointer("/spec/template/spec/containers")
                    .and_then(serde_json::Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                let scope = self.mutation_scope(&object, &resource)?;
                let request_id = scope.request;
                let built = crate::mutation::workflow::set_image(
                    scope,
                    resource,
                    &containers,
                    container.as_deref(),
                    &image,
                    request_id,
                )
                .map_err(|e| anyhow::anyhow!(e))?;
                self.open_workflow_document(format!("Set image: {}", object.name), built)
            }
            Command::Trigger => {
                let object = self.state.selected_object().context("Select a row first")?;
                let resource = self
                    .state
                    .resource
                    .clone()
                    .context("No resource selected")?;
                let job_template_spec = object
                    .value
                    .pointer("/spec/jobTemplate/spec")
                    .cloned()
                    .context("This object has no spec.jobTemplate.spec")?;
                let connection = self.connection.as_ref().context("Not connected")?;
                let job_resource = connection
                    .catalog
                    .resolve("batch/v1/jobs", &connection.settings.aliases)
                    .context("Job kind is not available on this cluster")?;
                // Generated exactly once here, when the intent is first built
                // -- never regenerated on a later render of the same preview,
                // matching Restart's own timestamp discipline.
                let mut job_name =
                    format!("{}-trigger-{}", object.name, chrono::Utc::now().timestamp());
                job_name.truncate(63);
                job_name = job_name.trim_end_matches('-').to_string();
                let scope = self.mutation_scope(&object, &resource)?;
                let request_id = scope.request;
                let built = crate::mutation::workflow::trigger_cronjob(
                    scope,
                    resource,
                    job_resource,
                    &job_template_spec,
                    &job_name,
                    request_id,
                )
                .map_err(|e| anyhow::anyhow!(e))?;
                self.open_workflow_document(format!("Trigger: {}", object.name), built)
            }
            Command::Evict => {
                let object = self.state.selected_object().context("Select a row first")?;
                let resource = self
                    .state
                    .resource
                    .clone()
                    .context("No resource selected")?;
                let scope = self.mutation_scope(&object, &resource)?;
                let request_id = scope.request;
                let built = crate::mutation::workflow::evict(scope, resource, request_id)
                    .map_err(|e| anyhow::anyhow!(e))?;
                self.open_workflow_document(format!("Evict: {}", object.name), built)
            }
            Command::ForceDelete => {
                let object = self.state.selected_object().context("Select a row first")?;
                let resource = self
                    .state
                    .resource
                    .clone()
                    .context("No resource selected")?;
                let scope = self.mutation_scope(&object, &resource)?;
                let request_id = scope.request;
                let built = crate::mutation::workflow::force_delete(scope, resource, request_id)
                    .map_err(|e| anyhow::anyhow!(e))?;
                self.open_workflow_document(format!("Force delete: {}", object.name), built)
            }
            Command::BulkLabel { key, value } => self.open_bulk_workflow(
                "bulk_label",
                "Bulk label",
                |scope, resource, _object, request_id| {
                    crate::mutation::workflow::label(
                        scope,
                        resource,
                        &key,
                        value.as_deref(),
                        request_id,
                    )
                },
            ),
            Command::BulkAnnotate { key, value } => self.open_bulk_workflow(
                "bulk_annotate",
                "Bulk annotate",
                |scope, resource, _object, request_id| {
                    crate::mutation::workflow::annotate(
                        scope,
                        resource,
                        &key,
                        value.as_deref(),
                        request_id,
                    )
                },
            ),
            Command::BulkScale(replicas) => self.open_bulk_workflow(
                "bulk_scale",
                "Bulk scale",
                |scope, resource, object, request_id| {
                    let current = object
                        .value
                        .pointer("/spec/replicas")
                        .and_then(serde_json::Value::as_i64);
                    crate::mutation::workflow::scale(scope, resource, current, replicas, request_id)
                },
            ),
            Command::BulkRestart => {
                // Generated exactly once for the whole bulk operation, shared
                // across every target -- matches Restart's own "generated
                // once, never per-render" discipline, extended to "never
                // per-target" so a bulk restart reads as one coordinated
                // action, not N independently-timestamped ones.
                let timestamp = chrono::Utc::now().to_rfc3339();
                self.open_bulk_workflow(
                    "bulk_restart",
                    "Bulk restart",
                    |scope, resource, _object, request_id| {
                        crate::mutation::workflow::restart(scope, resource, &timestamp, request_id)
                    },
                )
            }
            Command::BulkDelete => self.open_bulk_workflow(
                "bulk_delete",
                "Bulk delete",
                |scope, resource, _object, request_id| {
                    crate::mutation::workflow::delete(scope, resource, request_id)
                },
            ),
            Command::BulkEvict => self.open_bulk_workflow(
                "bulk_evict",
                "Bulk evict",
                |scope, resource, _object, request_id| {
                    crate::mutation::workflow::evict(scope, resource, request_id)
                },
            ),
            Command::BulkCordon => self.open_bulk_workflow(
                "bulk_cordon",
                "Bulk cordon",
                |scope, resource, _object, request_id| {
                    crate::mutation::workflow::cordon(scope, resource, request_id)
                },
            ),
            Command::BulkUncordon => self.open_bulk_workflow(
                "bulk_uncordon",
                "Bulk uncordon",
                |scope, resource, _object, request_id| {
                    crate::mutation::workflow::uncordon(scope, resource, request_id)
                },
            ),
            Command::BulkSetImage { container, image } => self.open_bulk_workflow(
                "bulk_set_image",
                "Bulk set image",
                |scope, resource, object, request_id| {
                    let containers = object
                        .value
                        .pointer("/spec/template/spec/containers")
                        .and_then(serde_json::Value::as_array)
                        .cloned()
                        .unwrap_or_default();
                    crate::mutation::workflow::set_image(
                        scope,
                        resource,
                        &containers,
                        container.as_deref(),
                        &image,
                        request_id,
                    )
                },
            ),
            Command::BulkTrigger => {
                let connection = self.connection.as_ref().context("Not connected")?;
                let job_resource = connection
                    .catalog
                    .resolve("batch/v1/jobs", &connection.settings.aliases)
                    .context("Job kind is not available on this cluster")?;
                // Shared across every target's own generated Job name below,
                // matching Trigger's own single-timestamp-per-preview
                // discipline; each target still gets its own distinct Job
                // name (object.name is part of it), so a bulk trigger never
                // collides two CronJobs' Jobs into the same name.
                let timestamp = chrono::Utc::now().timestamp();
                self.open_bulk_workflow(
                    "bulk_trigger",
                    "Bulk trigger",
                    move |scope, resource, object, request_id| {
                        let job_template_spec = object
                            .value
                            .pointer("/spec/jobTemplate/spec")
                            .cloned()
                            .ok_or_else(|| {
                                "this object has no spec.jobTemplate.spec".to_string()
                            })?;
                        let mut job_name = format!("{}-trigger-{timestamp}", object.name);
                        job_name.truncate(63);
                        job_name = job_name.trim_end_matches('-').to_string();
                        crate::mutation::workflow::trigger_cronjob(
                            scope,
                            resource,
                            job_resource.clone(),
                            &job_template_spec,
                            &job_name,
                            request_id,
                        )
                    },
                )
            }
            Command::Drain => {
                let object = self.state.selected_object().context("Select a row first")?;
                let resource = self
                    .state
                    .resource
                    .clone()
                    .context("No resource selected")?;
                anyhow::ensure!(
                    crate::mutation::drain::DRAIN_KINDS.contains(&resource.api.kind.as_str()),
                    "drain is not supported for {}",
                    resource.api.kind
                );
                let connection = self.connection.as_ref().context("Not connected")?;
                let pod_resource = connection
                    .catalog
                    .resolve("v1/pods", &connection.settings.aliases)
                    .context("Pod kind is not available on this cluster")?;
                let scope = self.mutation_scope(&object, &resource)?;
                let request_id = scope.request;
                let cordon_built = crate::mutation::workflow::cordon(scope, resource, request_id)
                    .map_err(|e| anyhow::anyhow!(e))?;
                self.open_drain_document(
                    format!("Drain: {}", object.name),
                    cordon_built,
                    pod_resource,
                )
            }
            Command::Flux => self.open_flux_view(),
            Command::FluxSuspend => {
                let object = self.state.selected_object().context("Select a row first")?;
                let resource = self
                    .state
                    .resource
                    .clone()
                    .context("No resource selected")?;
                let scope = self.mutation_scope(&object, &resource)?;
                let request_id = scope.request;
                let built = crate::mutation::workflow::flux_suspend(scope, resource, request_id)
                    .map_err(|e| anyhow::anyhow!(e))?;
                self.open_workflow_document(format!("Flux suspend: {}", object.name), built)
            }
            Command::FluxResume => {
                let object = self.state.selected_object().context("Select a row first")?;
                let resource = self
                    .state
                    .resource
                    .clone()
                    .context("No resource selected")?;
                let scope = self.mutation_scope(&object, &resource)?;
                let request_id = scope.request;
                let built = crate::mutation::workflow::flux_resume(scope, resource, request_id)
                    .map_err(|e| anyhow::anyhow!(e))?;
                self.open_workflow_document(format!("Flux resume: {}", object.name), built)
            }
            Command::FluxReconcile => {
                let object = self.state.selected_object().context("Select a row first")?;
                let resource = self
                    .state
                    .resource
                    .clone()
                    .context("No resource selected")?;
                let scope = self.mutation_scope(&object, &resource)?;
                let request_id = scope.request;
                // Generated exactly once here, when the intent is first built --
                // never regenerated on a later render of the same workflow,
                // matching Restart's own timestamp discipline.
                let timestamp = chrono::Utc::now().to_rfc3339();
                let built = crate::mutation::workflow::flux_reconcile(
                    scope, resource, &timestamp, request_id,
                )
                .map_err(|e| anyhow::anyhow!(e))?;
                self.open_workflow_document(format!("Flux reconcile: {}", object.name), built)
            }
            Command::ArgoCd => self.open_argocd_view(),
            Command::ArgoCdSync => {
                let object = self.state.selected_object().context("Select a row first")?;
                let resource = self
                    .state
                    .resource
                    .clone()
                    .context("No resource selected")?;
                let scope = self.mutation_scope(&object, &resource)?;
                let request_id = scope.request;
                let built = crate::mutation::workflow::argocd_sync(scope, resource, request_id)
                    .map_err(|e| anyhow::anyhow!(e))?;
                self.open_workflow_document(format!("Argo CD sync: {}", object.name), built)
            }
            Command::ArgoCdRefresh { hard } => {
                let object = self.state.selected_object().context("Select a row first")?;
                let resource = self
                    .state
                    .resource
                    .clone()
                    .context("No resource selected")?;
                let scope = self.mutation_scope(&object, &resource)?;
                let request_id = scope.request;
                let built =
                    crate::mutation::workflow::argocd_refresh(scope, resource, hard, request_id)
                        .map_err(|e| anyhow::anyhow!(e))?;
                self.open_workflow_document(format!("Argo CD refresh: {}", object.name), built)
            }
            Command::ArgoCdRollback { revision } => {
                let object = self.state.selected_object().context("Select a row first")?;
                let resource = self
                    .state
                    .resource
                    .clone()
                    .context("No resource selected")?;
                // The revision must already be one Argo CD itself recorded
                // for this exact Application -- never a free-form/
                // unverified string (see `workflow::argocd_rollback`'s own
                // doc comment for why: this is what keeps rollback from
                // becoming "sync to anything the user happens to type").
                let known = object
                    .value
                    .pointer("/status/history")
                    .and_then(serde_json::Value::as_array)
                    .is_some_and(|history| {
                        history.iter().any(|entry| {
                            entry.get("revision").and_then(serde_json::Value::as_str)
                                == Some(revision.as_str())
                        })
                    });
                anyhow::ensure!(
                    known,
                    "{revision:?} is not in this Application's own status.history; rollback only \
                     accepts an already-recorded revision"
                );
                let scope = self.mutation_scope(&object, &resource)?;
                let request_id = scope.request;
                let built = crate::mutation::workflow::argocd_rollback(
                    scope, resource, &revision, request_id,
                )
                .map_err(|e| anyhow::anyhow!(e))?;
                self.open_workflow_document(format!("Argo CD rollback: {}", object.name), built)
            }
            Command::Helm => self.open_helm_view(),
            Command::StopForward(number) => {
                let id = self
                    .forwards
                    .entries
                    .keys()
                    .find(|id| id.number() == number)
                    .copied()
                    .context("Unknown forward session ID")?;
                anyhow::ensure!(self.sessions.stop(id), "Forward already ended");
                self.update_forwards();
                Ok(())
            }
        }
    }
    pub fn action(&mut self, action: Action) -> Result<()> {
        use Action::*;
        match action {
            Quit => self.state.quit = true,
            Down | Up | First | Last | PageDown | PageUp => self.state.move_selection(action),
            Back => {
                if !matches!(self.state.mode, Mode::Table) {
                    self.document.cancel();
                    self.state.request += 1;
                    self.state.mode = Mode::Table;
                    self.palette_document = None;
                } else if !self.state.filter_text.is_empty() {
                    self.state.filter_text.clear();
                    self.state.filter = crate::filters::Expr::All;
                } else {
                    self.state.quit = true;
                }
            }
            Palette => {
                self.open_palette(String::new());
            }
            Filter => {
                self.state.mode = if let Mode::Document(doc) =
                    std::mem::replace(&mut self.state.mode, Mode::Table)
                {
                    Mode::Search(doc, String::new())
                } else {
                    Mode::Filter(self.state.filter_text.clone())
                };
            }
            Help => self.open_static("Keyboard reference", self.state.keymap.help()),
            Yaml | Describe | Explain | Events | Adjacent | Xray => self.open_document(action)?,
            Follow => self.follow_adjacent()?,
            Policy => self.open_policy_view()?,
            Mutations => self.open_mutations_view()?,
            Timeline => {
                let object = self.state.selected_object().context("Select a row first")?;
                let uid = object.uid.clone();
                self.open_static("Timeline", self.timeline_text(&uid));
                if let Some(doc) = self.active_document_mut() {
                    doc.timeline_for = Some(uid);
                }
            }
            Logs => self.open_logs(None, false)?,
            PreviousLogs => self.open_logs(None, true)?,
            Forward => self.pick_forward()?,
            ForwardManager => {
                let text = self.forwards.document(&self.sessions);
                self.open_static("Port forwards · original contexts", text);
                if let Some(doc) = self.active_document_mut() {
                    doc.forward_manager = true;
                }
            }
            StopForward => self.open_palette("pf_stop ".into()),
            LogsVisible => {
                self.state.prepare();
                let objects = self.state.rows.clone();
                self.start_logs(crate::kube::logs::Request {
                    objects,
                    options: crate::kube::logs::LogOptions {
                        container: Some("*".into()),
                        previous: false,
                    },
                })?;
            }
            PauseLogs | ClearLogs | FilterLogs => {
                let doc = self.active_document_mut().context("Open logs first")?;
                anyhow::ensure!(doc.session.is_some(), "This action requires a log session");
                match action {
                    PauseLogs => doc.follow = !doc.follow,
                    ClearLogs => doc.replace(String::new()),
                    FilterLogs => doc.toggle_log_filter(),
                    _ => unreachable!(),
                }
            }
            Refresh => {
                if matches!(self.state.mode, Mode::Document(_)) {
                    self.refresh_document()?;
                } else {
                    self.watch()?;
                }
            }
            AllNamespaces => self.switch_namespace(None)?,
            Namespaces => self.open_namespace_picker()?,
            Sort => {
                let columns = self.state.columns();
                let i = columns
                    .iter()
                    .position(|c| c == &self.state.sort)
                    .unwrap_or(0);
                if let Some(next) = columns.get((i + 1) % columns.len().max(1)) {
                    self.state.sort = next.clone();
                }
            }
            Reverse => self.state.descending = !self.state.descending,
            Wide => self.state.wide = !self.state.wide,
            Wrap => {
                if let Mode::Document(d) = &mut self.state.mode {
                    d.wrap = !d.wrap;
                }
            }
            ScrollLeft | ScrollRight => {
                if let Mode::Document(d) = &mut self.state.mode {
                    anyhow::ensure!(!d.wrap, "Turn wrapping off before horizontal scrolling");
                    d.horizontal = if action == ScrollLeft {
                        d.horizontal.saturating_sub(4)
                    } else {
                        d.horizontal.saturating_add(4)
                    };
                }
            }
            Fullscreen => {
                if let Mode::Document(d) = &mut self.state.mode {
                    d.fullscreen = !d.fullscreen;
                }
            }
            SearchNext | SearchPrevious => {
                if let Mode::Document(d) = &mut self.state.mode {
                    d.search_next(action == SearchPrevious);
                }
            }
            ToggleWarnings => {
                let source = self
                    .active_document_mut()
                    .and_then(|d| d.source.as_mut())
                    .context("No document open")?;
                anyhow::ensure!(
                    source.action == Action::Events,
                    "Warning-only filtering only applies to the Events view"
                );
                source.warning_only = !source.warning_only;
                self.refresh_document()?;
            }
            HistoryBack => {
                if let Some(entry) = self.history.pop_back() {
                    if let Some(current) = self.current_history_entry() {
                        self.forward.push_back(current);
                    }
                    self.apply_history(entry);
                }
            }
            HistoryForward => {
                if let Some(entry) = self.forward.pop_back() {
                    if let Some(current) = self.current_history_entry() {
                        push_capped(&mut self.history, current, HISTORY_LIMIT);
                    }
                    self.apply_history(entry);
                }
            }
            MutationDryRun => self.mutation_dry_run()?,
            MutationConfirm => self.mutation_confirm()?,
            ToggleSelect => self.toggle_select()?,
            SelectVisible => self.select_visible(),
            ClearSelection => self.state.selection.clear(),
            InvertSelection => self.invert_selection(),
            InspectSelection => self.open_selection_view(),
        }
        self.state.dirty = true;
        Ok(())
    }
    /// M10.1: toggles the row under the cursor in the bulk multi-select
    /// set. Deliberately separate from cursor movement (`Down`/`Up`/...)
    /// -- pressing this never moves the cursor, and moving the cursor
    /// never changes the selected set.
    fn toggle_select(&mut self) -> Result<()> {
        let object = self.state.selected_object().context("Select a row first")?;
        let selected = self
            .state
            .selection
            .toggle(&object)
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        self.state.status = format!(
            "{} {} ({} selected)",
            if selected { "Selected" } else { "Deselected" },
            object.name,
            self.state.selection.len()
        );
        Ok(())
    }
    /// M10.1: adds every currently visible (already-filtered) row to the
    /// selection -- reuses the existing bounded `rows`, never a fresh
    /// unbounded scan. Idempotent per row (`Selection::add`, not
    /// `toggle`): re-running this over a partially-selected view only
    /// adds, never deselects. If the bound is hit partway through, the
    /// rows already added stay selected and the refusal is reported
    /// explicitly -- never silently stopping with no indication of how
    /// many were actually added versus skipped.
    fn select_visible(&mut self) {
        let before = self.state.selection.len();
        let total = self.state.rows.len();
        let mut refused = false;
        for object in &self.state.rows.clone() {
            if self.state.selection.add(object).is_err() {
                refused = true;
                break;
            }
        }
        let added = self.state.selection.len() - before;
        self.state.status = if refused {
            format!(
                "Selected {added} more (bound {} reached; {} of {total} visible rows not added)",
                selection::MAX_SELECTION,
                total.saturating_sub(before + added)
            )
        } else {
            format!(
                "Selected {added} more visible rows ({} total)",
                self.state.selection.len()
            )
        };
    }
    /// M10.2: inverts the selection strictly within currently visible
    /// (already-filtered) rows -- never "everything in the cluster minus
    /// what's selected." A row already selected is deselected (never
    /// fails); a row not yet selected is added (can hit the bound). If
    /// the bound is hit partway through, everything toggled so far stays
    /// toggled and the refusal is reported explicitly.
    fn invert_selection(&mut self) {
        let before = self.state.selection.len();
        let mut refused = false;
        for object in &self.state.rows.clone() {
            if self.state.selection.toggle(object).is_err() {
                refused = true;
                break;
            }
        }
        let after = self.state.selection.len();
        self.state.status = if refused {
            format!(
                "Inverted selection partially (bound {} reached); {after} selected",
                selection::MAX_SELECTION
            )
        } else {
            format!("Inverted selection: {after} selected (was {before})")
        };
    }
    /// M10.2: a read-only, bounded detail view of the current selection
    /// -- never issues a request, reuses the already-fetched `rows` for
    /// staleness. Opens even when the selection is empty (reports that
    /// explicitly), matching every other read-only view's "never silent"
    /// convention rather than erroring on an empty selection.
    fn open_selection_view(&mut self) {
        let resource_label = self
            .state
            .resource
            .as_ref()
            .map(crate::kube::discovery::Resource::qualified)
            .unwrap_or_else(|| self.state.query.resource.clone());
        let text = selection::report(&self.state.selection, &self.state.rows, &resource_label);
        self.open_static("Selection", text);
    }
    /// Session-local, UID-scoped watch history -- never Events or an audit
    /// log. Newest first, since the most recent transition is almost always
    /// what prompted opening this. `RelistObserved` entries are marked, since
    /// they may compress real changes that happened while disconnected.
    fn timeline_text(&self, uid: &str) -> String {
        let history = self.state.store.histories.get(uid);
        if history.is_none_or(|h| h.is_empty()) {
            return "No meaningful watch changes observed for this UID during this view. \
                    History is session-local and bounded (64 entries/object, 256 objects); \
                    it is not an audit log and does not persist across restarts."
                .into();
        }
        history
            .into_iter()
            .flatten()
            .rev()
            .map(|c| {
                let source = match c.source {
                    crate::resources::store::Source::WatchObserved => "watch",
                    crate::resources::store::Source::RelistObserved => "relist",
                };
                format!("{} [{source}] {}", c.at.to_rfc3339(), c.summary)
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
    fn open_static(&mut self, title: &str, text: String) {
        self.document.cancel();
        self.state.request += 1;
        self.state.mode = Mode::Document(Document::new(title.into(), crate::safety::text(&text)));
        self.state.dirty = true;
    }
    fn active_document_mut(&mut self) -> Option<&mut Document> {
        match &mut self.state.mode {
            Mode::Document(doc) | Mode::Search(doc, _) => Some(doc),
            Mode::Command(_) => self.palette_document.as_mut(),
            _ => None,
        }
    }
    fn mode_name(&self) -> &'static str {
        match &self.state.mode {
            Mode::Document(doc) if doc.forward_manager => "forwards",
            Mode::Document(doc) if doc.session.is_some() => "logs",
            Mode::Document(doc) if doc.workflow.is_some() => "mutation",
            Mode::Document(doc) if doc.drain.is_some() => "drain",
            Mode::Document(doc) if doc.bulk.is_some() => "bulk",
            Mode::Document(_) => "document",
            _ => "table",
        }
    }
    fn open_palette(&mut self, text: String) {
        let previous = std::mem::replace(&mut self.state.mode, Mode::Command(text));
        if let Mode::Document(doc) = previous {
            self.palette_document = Some(doc);
        }
    }
    fn close_palette(&mut self) {
        self.state.mode = self
            .palette_document
            .take()
            .map(Mode::Document)
            .unwrap_or(Mode::Table);
    }
    /// M8.0: read-only, local, no network. `cluster_verified_for_mutation` is a
    /// distinct state from `readonly` -- it reflects only the external,
    /// out-of-band attestation carried by `--mutation-test-cluster-verified`
    /// (see `scripts/test-cluster.sh`, which sets it only after independently
    /// proving cluster identity via Docker/API introspection). It is never
    /// derived from `readonly`, never from the context/cluster name.
    fn open_policy_view(&mut self) -> Result<()> {
        let object = self.state.selected_object().context("Select a row first")?;
        let resource = self
            .state
            .resource
            .clone()
            .context("No resource selected")?;
        let connection = self.connection.as_ref().context("Not connected")?;
        let scope = session::Scope {
            epoch: self.state.epoch,
            request: self.state.request,
            context: connection.context.clone(),
            cluster: connection.cluster.clone(),
            resource: resource.id(),
            namespace: object.namespace.clone(),
            name: object.name.clone(),
            uid: object.uid.clone(),
        };
        let policy_context = self.mutation_policy_context();
        let text = crate::mutation::view::policy_report(&policy_context, &scope, &resource);
        self.open_static(&format!("Policy: {}", object.name), text);
        Ok(())
    }
    /// M9.1: read-only, no network beyond what discovery already fetched at
    /// connect time. If the currently selected object is one of Flux's own
    /// reconciliation kinds (`integrations::flux::is_status_kind`), shows
    /// that object's own status (conditions/observedGeneration/suspend/
    /// revision/sourceRef) rendered verbatim; otherwise shows the overall
    /// Flux capability report (which kinds are actually installed) --
    /// never a guess from names alone, matching M9.0's own contract.
    fn open_flux_view(&mut self) -> Result<()> {
        let connection = self.connection.as_ref().context("Not connected")?;
        let selected = self
            .state
            .resource
            .as_ref()
            .filter(|r| crate::integrations::flux::is_status_kind(&r.api.kind))
            .and_then(|_| self.state.selected_object());
        let (title, text) = match selected {
            Some(object) => {
                let resource = self.state.resource.as_ref().expect("filtered above");
                let text = crate::integrations::flux::status_report(
                    &resource.api.kind,
                    &object.name,
                    &object.value,
                );
                (format!("Flux: {}", object.name), text)
            }
            None => {
                let discovery = crate::integrations::discover(
                    &connection.catalog,
                    crate::integrations::Integration::Flux,
                    crate::integrations::flux::KINDS,
                );
                (
                    "Flux".to_string(),
                    crate::integrations::view::discovery_report(&discovery),
                )
            }
        };
        self.open_static(&title, text);
        Ok(())
    }
    /// M9.3: read-only, no network beyond what discovery already fetched.
    /// Mirrors `open_flux_view` exactly -- selected object's own status if
    /// it is Argo CD's `Application` kind, otherwise the capability report.
    fn open_argocd_view(&mut self) -> Result<()> {
        let connection = self.connection.as_ref().context("Not connected")?;
        let selected = self
            .state
            .resource
            .as_ref()
            .filter(|r| crate::integrations::argocd::is_status_kind(&r.api.kind))
            .and_then(|_| self.state.selected_object());
        let (title, text) = match selected {
            Some(object) => {
                let resource = self.state.resource.as_ref().expect("filtered above");
                let text = crate::integrations::argocd::status_report(
                    &resource.api.kind,
                    &object.name,
                    &object.value,
                );
                (format!("Argo CD: {}", object.name), text)
            }
            None => {
                let discovery = crate::integrations::discover(
                    &connection.catalog,
                    crate::integrations::Integration::ArgoCd,
                    crate::integrations::argocd::KINDS,
                );
                (
                    "Argo CD".to_string(),
                    crate::integrations::view::discovery_report(&discovery),
                )
            }
        };
        self.open_static(&title, text);
        Ok(())
    }
    /// M9.5: the ONLY path in this app that ever reads a Helm release
    /// Secret's body. Unlike `:flux`/`:argocd`, there is no "capability
    /// report" fallback: Helm has no CRD/schema presence to discover
    /// (see `integrations::helm`'s own module doc comment), so the only
    /// meaningful state is "a release record is selected" or not.
    ///
    /// The already-cached, already-redacted `object` here is used only
    /// to decide whether this looks like a Helm release Secret and to
    /// identify WHICH object to fetch (namespace/name/uid) -- it is never
    /// itself the source of the release body (its `data` is already
    /// `<redacted>` by `Object::new`'s own unconditional Secret
    /// redaction) and it is never treated as authorization to skip
    /// re-verification. `kube::helm::read_release` performs a fresh,
    /// bounded GET and independently re-checks UID and `type` before
    /// ever decoding -- see that module's own doc comment.
    fn open_helm_view(&mut self) -> Result<()> {
        let object = self
            .state
            .selected_object()
            .context("Select a Helm release Secret first (type=helm.sh/release.v1)")?;
        anyhow::ensure!(
            object.kind == "Secret",
            "Selected object is not a Secret (Helm releases are stored as Secrets)"
        );
        let connection = self.connection.clone().context("Not connected")?;
        let scope = session::Scope {
            epoch: self.state.epoch,
            request: self.state.request,
            context: connection.context.clone(),
            cluster: connection.cluster.clone(),
            resource: "v1/secrets".to_string(),
            namespace: object.namespace.clone(),
            name: object.name.clone(),
            uid: object.uid.clone(),
        };
        let name = object.name.clone();
        self.open_static(&format!("Helm: {name}"), "Loading Helm release...".into());
        self.document.cancel();
        self.document = self.scope.child_token();
        self.state.request += 1;
        let request = self.state.request;
        let epoch = self.state.epoch;
        let tx = self.tx.clone();
        let cancel = self.document.clone();
        self.tasks.spawn(async move {
            let result = tokio::select! {
                biased;
                _ = cancel.cancelled() => return,
                result = crate::kube::helm::read_release(&connection, &scope, &cancel) => result,
            };
            let payload = match result {
                Ok(view) => Payload::Document {
                    request,
                    title: format!("Helm: {name}"),
                    text: crate::integrations::helm::status_report(&view),
                    adjacent: vec![],
                },
                Err(e) => Payload::DocumentError {
                    request,
                    error: e.to_string(),
                },
            };
            tokio::select! { _ = cancel.cancelled() => {}, _ = tx.send(Event { epoch, payload }) => {} }
        });
        Ok(())
    }
    /// M8.0: the single place that builds a `PolicyContext` from live runtime
    /// state. `cluster_verified_for_mutation` comes only from the explicit
    /// `--mutation-test-cluster-verified` attestation (see `ConnectOptions`'
    /// own doc comment) -- never from `readonly`, never from context/cluster
    /// name. Built fresh on every call, never cached across a context switch.
    fn mutation_policy_context(&self) -> crate::mutation::policy::PolicyContext {
        crate::mutation::policy::PolicyContext {
            readonly: self.state.settings.readonly,
            readonly_forced: self.options.force_readonly,
            cluster_verified_for_mutation: self.options.mutation_test_cluster_verified,
            ..Default::default()
        }
    }
    /// M8.0: bumps `state.request` and builds the incarnation-safe `Scope`
    /// every mutation builder needs -- the same convention already used by
    /// owned sessions (logs/exec/forward) and M7's own fixtures.
    fn mutation_scope(
        &mut self,
        object: &crate::resources::Object,
        resource: &crate::kube::discovery::Resource,
    ) -> Result<session::Scope> {
        let connection = self.connection.as_ref().context("Not connected")?;
        self.document.cancel();
        self.state.request += 1;
        Ok(session::Scope {
            epoch: self.state.epoch,
            request: self.state.request,
            context: connection.context.clone(),
            cluster: connection.cluster.clone(),
            resource: resource.id(),
            namespace: object.namespace.clone(),
            name: object.name.clone(),
            uid: object.uid.clone(),
        })
    }
    /// M8.0: the one place a mutation preview document is opened -- builds
    /// the `PolicyEvaluation` fresh (never trusts a cached decision), never
    /// issues a network request itself.
    fn open_workflow_document(
        &mut self,
        title: String,
        built: crate::mutation::workflow::Built,
    ) -> Result<()> {
        let policy_context = self.mutation_policy_context();
        let evaluation = crate::mutation::policy::evaluate(&policy_context, &built.intent);
        let workflow = crate::mutation::workflow::Workflow::new(built, evaluation);
        let text = crate::mutation::view::workflow_report(&workflow);
        let mut doc = Document::new(title, text);
        doc.workflow = Some(workflow);
        self.state.mode = Mode::Document(doc);
        self.palette_document = None;
        Ok(())
    }
    /// M10.3: the one place a bulk mutation preview document is opened.
    /// Builds one ordinary `Built`+`PolicyEvaluation` per currently
    /// selected target via `build` -- the exact same per-target pipeline
    /// `open_workflow_document` uses for a single target, just run once
    /// per selection member. A target no longer present in the current
    /// `rows` (stale, per `app::selection`'s own contract) is recorded as
    /// an explicit build-time `Unsupported` rather than silently skipped
    /// or guessed at from a possibly-stale cached copy. Never issues a
    /// network request itself.
    fn open_bulk_workflow(
        &mut self,
        source_action: &str,
        title: &str,
        build: impl Fn(
            session::Scope,
            Resource,
            &crate::resources::Object,
            u64,
        ) -> Result<crate::mutation::workflow::Built, String>,
    ) -> Result<()> {
        anyhow::ensure!(
            !self.state.selection.is_empty(),
            "Selection is empty -- select at least one target first (space to toggle, V for visible)"
        );
        let connection = self.connection.as_ref().context("Not connected")?;
        let context = connection.context.clone();
        let cluster = connection.cluster.clone();
        let resource = self
            .state
            .resource
            .clone()
            .context("No resource selected")?;
        self.document.cancel();
        self.state.request += 1;
        let request_id = self.state.request;
        let epoch = self.state.epoch;
        let policy_context = self.mutation_policy_context();
        let rows = self.state.rows.clone();
        let mut items = Vec::new();
        for target in self.state.selection.iter() {
            let scope = session::Scope {
                epoch,
                request: request_id,
                context: context.clone(),
                cluster: cluster.clone(),
                resource: resource.id(),
                namespace: target.namespace.clone(),
                name: target.name.clone(),
                uid: target.uid.clone(),
            };
            let built = match rows.iter().find(|o| o.uid == target.uid) {
                Some(object) => build(scope, resource.clone(), object, request_id),
                None => {
                    Err("target is stale (no longer present in the current view); reselect it after a fresh list".into())
                }
            };
            items.push(crate::mutation::bulk::BulkItem::from_built(
                target.uid.clone(),
                target.namespace.clone(),
                target.name.clone(),
                built,
                |b| crate::mutation::policy::evaluate(&policy_context, &b.intent),
            ));
        }
        let workflow = crate::mutation::bulk::BulkWorkflow::new(source_action.into(), items);
        let text = crate::mutation::view::bulk_report(&workflow);
        let mut doc = Document::new(title.into(), text);
        doc.bulk = Some(workflow);
        self.state.mode = Mode::Document(doc);
        self.palette_document = None;
        Ok(())
    }
    /// M8B.5: opens the Drain preview immediately (policy evaluated fresh,
    /// same as `open_workflow_document`), then kicks off the one live read
    /// (list Pods on this Node) needed to make the preview truthful. The
    /// preview is never blocked open waiting on that read -- it opens in
    /// an explicit `Loading` state first.
    fn open_drain_document(
        &mut self,
        title: String,
        cordon_built: crate::mutation::workflow::Built,
        pod_resource: Resource,
    ) -> Result<()> {
        let policy_context = self.mutation_policy_context();
        let evaluation = crate::mutation::policy::evaluate(&policy_context, &cordon_built.intent);
        let drain = crate::mutation::drain::DrainWorkflow::new(
            cordon_built.intent,
            cordon_built.payload,
            cordon_built.change,
            pod_resource.clone(),
            evaluation,
        );
        let text = crate::mutation::view::drain_report(&drain);
        let mut doc = Document::new(title, text);
        doc.drain = Some(drain);
        self.state.mode = Mode::Document(doc);
        self.palette_document = None;
        self.start_drain_preview(pod_resource);
        Ok(())
    }
    /// M8B.5: the one live read behind the Drain preview -- bounded,
    /// read-only, and never authoritative for execution (`kube::drain::
    /// drain` always re-lists fresh at commit time; this is display-
    /// freshness only, never a TOCTOU shortcut).
    ///
    /// `mutation_scope()` (already called by the caller to build the
    /// cordon scope) only cancels `self.document`, it never reassigns a
    /// fresh child token -- every other spawn site does that refresh
    /// itself right before spawning (see `mutation_confirm`/
    /// `mutation_dry_run`), and this one must too, or the task below
    /// would capture an already-cancelled token and return immediately
    /// without ever listing anything, leaving the preview stuck on
    /// `Loading` forever (found live against the real cluster).
    fn start_drain_preview(&mut self, pod_resource: Resource) {
        self.document = self.scope.child_token();
        let Some(connection) = self.connection.clone() else {
            return;
        };
        let Some(node_name) = self
            .active_document_mut()
            .and_then(|d| d.drain.as_ref())
            .map(|d| d.cordon_intent.target.scope.name.clone())
        else {
            return;
        };
        let request = self.state.request;
        let epoch = self.state.epoch;
        let tx = self.tx.clone();
        let cancel = self.document.clone();
        self.tasks.spawn(async move {
            let result = tokio::select! {
                biased;
                _ = cancel.cancelled() => return,
                result = crate::kube::drain::pods_on_node(&connection, &pod_resource, &node_name, &cancel) => result,
            };
            let planned = match result {
                Ok(pods) => Ok(crate::mutation::drain::plan(&pods)),
                Err(outcome) => Err(format!("{outcome:?}")),
            };
            tokio::select! {
                _ = cancel.cancelled() => {},
                _ = tx.send(Event { epoch, payload: Payload::DrainPlanned { request, planned } }) => {},
            }
        });
    }
    /// M8B.5: Drain's own confirm handler -- the exact same arm/commit
    /// contract `mutation_confirm` uses for a single intent
    /// (`RequireStrongerConfirmation` arms on the first press, commits on
    /// the second; `Deny`/`Unsupported` sends zero requests, ever), but
    /// the "commit" here is the whole `kube::drain::drain` orchestrator --
    /// never a second, parallel confirmation mechanism.
    fn drain_confirm(&mut self) -> Result<()> {
        let connection = self.connection.clone().context("Not connected")?;
        let epoch = self.state.epoch;
        let doc = self
            .active_document_mut()
            .context("No mutation preview open")?;
        let drain = doc.drain.as_mut().context("No drain preview open")?;
        anyhow::ensure!(
            !matches!(
                drain.evaluation.decision,
                crate::mutation::PolicyDecision::Deny
                    | crate::mutation::PolicyDecision::Unsupported
            ),
            "DENIED: this action is not permitted, no request will be sent"
        );
        anyhow::ensure!(
            drain.cordon_intent.target.scope.epoch == epoch,
            "Target replaced or context changed; reopen the preview"
        );
        match &drain.preview {
            crate::mutation::drain::DrainPreview::Ready(_) => {}
            crate::mutation::drain::DrainPreview::Loading => {
                anyhow::bail!(
                    "Still listing Pods on this Node; wait for the plan before confirming"
                )
            }
            crate::mutation::drain::DrainPreview::Failed(message) => {
                anyhow::bail!("Cannot confirm: the Pod list failed to load ({message})")
            }
        }
        if drain.requirement() == crate::mutation::ConfirmationRequirement::Strong && !drain.armed {
            drain.armed = true;
            let text = crate::mutation::view::drain_report(doc.drain.as_ref().unwrap());
            doc.replace(text);
            return Ok(());
        }
        let node_scope = drain.cordon_intent.target.scope.clone();
        let node_resource = drain.cordon_intent.target.resource.clone();
        let pod_resource = drain.pod_resource.clone();
        let request_id = drain.cordon_intent.request_id;
        drain.armed = false;
        self.document.cancel();
        self.document = self.scope.child_token();
        self.state.request += 1;
        let request = self.state.request;
        let tx = self.tx.clone();
        let cancel = self.document.clone();
        let policy_context = self.mutation_policy_context();
        let journal = self.mutation_journal();
        self.tasks.spawn(async move {
            let report = tokio::select! {
                biased;
                _ = cancel.cancelled() => return,
                report = crate::kube::drain::drain(&connection, &policy_context, epoch, node_scope, node_resource, pod_resource, request_id, &journal, &cancel) => report,
            };
            tokio::select! {
                _ = cancel.cancelled() => {},
                _ = tx.send(Event { epoch, payload: Payload::DrainCommit { request, report } }) => {},
            }
        });
        Ok(())
    }
    /// M10.3: Bulk's own confirm handler -- the exact same arm/commit
    /// contract as `mutation_confirm`'s single-target one
    /// (`RequireStrongerConfirmation` arms on the first press, commits on
    /// the second; `Deny`/`Unsupported` items are excluded from
    /// execution, never sent), but the confirmation strength is the
    /// STRONGEST among every eligible target (`BulkWorkflow::requirement`)
    /// and "commit" is `kube::mutation::bulk_commit`'s own sequential
    /// per-target loop -- never a second, parallel confirmation
    /// mechanism, and no target's own individual `Confirmation`/
    /// `PolicyEvaluation` is bypassed by this bulk-level gesture.
    fn bulk_confirm(&mut self) -> Result<()> {
        let connection = self.connection.clone().context("Not connected")?;
        let epoch = self.state.epoch;
        let doc = self
            .active_document_mut()
            .context("No mutation preview open")?;
        let bulk = doc.bulk.as_mut().context("No bulk preview open")?;
        anyhow::ensure!(!bulk.is_empty(), "Selection was empty; nothing to do");
        anyhow::ensure!(
            bulk.eligible_count() > 0,
            "No eligible targets: every selected target is denied or unsupported"
        );
        let built_epoch = bulk
            .eligible()
            .next()
            .map(|w| w.intent.target.scope.epoch)
            .context("No eligible targets")?;
        anyhow::ensure!(
            built_epoch == epoch,
            "Target replaced or context changed; reopen the preview"
        );
        if bulk.requirement() == crate::mutation::ConfirmationRequirement::Strong && !bulk.armed {
            bulk.armed = true;
            let text = crate::mutation::view::bulk_report(doc.bulk.as_ref().unwrap());
            doc.replace(text);
            return Ok(());
        }
        bulk.armed = false;
        let items: Vec<_> = bulk
            .eligible()
            .map(|w| (w.intent.clone(), w.payload.clone(), w.confirmation()))
            .collect();
        self.document.cancel();
        self.document = self.scope.child_token();
        self.state.request += 1;
        let request = self.state.request;
        let tx = self.tx.clone();
        let cancel = self.document.clone();
        let policy_context = self.mutation_policy_context();
        let journal = self.mutation_journal();
        self.tasks.spawn(async move {
            let results = tokio::select! {
                biased;
                _ = cancel.cancelled() => return,
                results = crate::kube::mutation::bulk_commit(&connection, &policy_context, epoch, &items, &journal, &cancel) => results,
            };
            tokio::select! {
                _ = cancel.cancelled() => {},
                _ = tx.send(Event { epoch, payload: Payload::BulkCommit { request, results } }) => {},
            }
        });
        Ok(())
    }
    /// M8.0: a server dry-run only -- never gated behind confirmation, but
    /// still fully gated behind policy: `Deny`/`Unsupported` sends zero
    /// requests, exactly like a real commit would refuse to.
    fn mutation_dry_run(&mut self) -> Result<()> {
        let connection = self.connection.clone().context("Not connected")?;
        anyhow::ensure!(
            self.active_document_mut().is_none_or(|d| d.drain.is_none()),
            "Dry-run is not implemented for Drain -- use the preview text and the double confirm instead"
        );
        anyhow::ensure!(
            self.active_document_mut().is_none_or(|d| d.bulk.is_none()),
            "Dry-run is not implemented for bulk actions -- use the preview text and confirm instead"
        );
        let workflow = self
            .active_document_mut()
            .and_then(|d| d.workflow.as_ref())
            .context("No mutation preview open")?;
        anyhow::ensure!(
            !matches!(
                workflow.evaluation.decision,
                crate::mutation::PolicyDecision::Deny
                    | crate::mutation::PolicyDecision::Unsupported
            ),
            "DENIED: this action is not permitted, no request will be sent"
        );
        let intent = workflow.intent.clone();
        let payload = workflow.payload.clone();
        self.document.cancel();
        self.document = self.scope.child_token();
        self.state.request += 1;
        let request = self.state.request;
        let epoch = self.state.epoch;
        let tx = self.tx.clone();
        let cancel = self.document.clone();
        let journal = self.mutation_journal();
        self.tasks.spawn(async move {
            let outcome = tokio::select! {
                biased;
                _ = cancel.cancelled() => return,
                outcome = crate::kube::mutation::preflight(&connection, &intent, payload.as_ref(), &journal, &cancel) => outcome,
            };
            tokio::select! {
                _ = cancel.cancelled() => {},
                _ = tx.send(Event { epoch, payload: Payload::MutationDryRun { request, outcome } }) => {},
            }
        });
        Ok(())
    }
    /// M8.0: `Allow`/`RequireConfirmation` commit on the first press;
    /// `RequireStrongerConfirmation` arms on the first press and commits on
    /// the second -- `armed` is UX state only, never authorization itself.
    /// `Deny`/`Unsupported` sends zero requests, ever.
    fn mutation_confirm(&mut self) -> Result<()> {
        if self
            .active_document_mut()
            .is_some_and(|d| d.drain.is_some())
        {
            return self.drain_confirm();
        }
        if self.active_document_mut().is_some_and(|d| d.bulk.is_some()) {
            return self.bulk_confirm();
        }
        let connection = self.connection.clone().context("Not connected")?;
        let epoch = self.state.epoch;
        let doc = self
            .active_document_mut()
            .context("No mutation preview open")?;
        let workflow = doc.workflow.as_mut().context("No mutation preview open")?;
        anyhow::ensure!(
            !matches!(
                workflow.evaluation.decision,
                crate::mutation::PolicyDecision::Deny
                    | crate::mutation::PolicyDecision::Unsupported
            ),
            "DENIED: this action is not permitted, no request will be sent"
        );
        anyhow::ensure!(
            workflow.intent.target.scope.epoch == epoch,
            "Target replaced or context changed; reopen the preview"
        );
        if workflow.requirement() == crate::mutation::ConfirmationRequirement::Strong
            && !workflow.armed
        {
            workflow.armed = true;
            doc.replace(crate::mutation::view::workflow_report(
                doc.workflow.as_ref().unwrap(),
            ));
            return Ok(());
        }
        let confirmation = workflow.confirmation();
        let intent = workflow.intent.clone();
        let payload = workflow.payload.clone();
        workflow.armed = false;
        self.document.cancel();
        self.document = self.scope.child_token();
        self.state.request += 1;
        let request = self.state.request;
        let tx = self.tx.clone();
        let cancel = self.document.clone();
        let policy_context = self.mutation_policy_context();
        let journal = self.mutation_journal();
        self.tasks.spawn(async move {
            let outcome = tokio::select! {
                biased;
                _ = cancel.cancelled() => return,
                outcome = crate::kube::mutation::commit(&connection, &policy_context, epoch, &intent, Some(&confirmation), payload.clone(), &journal, &cancel) => outcome,
            };
            // M8.5: a verification failure/timeout NEVER downgrades `outcome`
            // below -- the server already confirmed (or refused) the write.
            // Only attempted for a real commit; anything else has nothing to
            // verify. Exactly one bounded, cancellable attempt, never retried.
            let verification = if matches!(
                outcome,
                crate::mutation::MutationOutcome::Committed
                    | crate::mutation::MutationOutcome::CommittedButJournalIncomplete
            ) {
                let v = tokio::select! {
                    biased;
                    _ = cancel.cancelled() => crate::mutation::Verification::Unknown,
                    v = crate::kube::mutation::verify(&connection, &intent, payload.as_ref(), &cancel) => v,
                };
                let _ = journal.append(crate::kube::mutation::verification_record(&intent, &v));
                Some(v)
            } else {
                None
            };
            tokio::select! {
                _ = cancel.cancelled() => {},
                _ = tx.send(Event { epoch, payload: Payload::MutationCommit { request, outcome, verification } }) => {},
            }
        });
        Ok(())
    }
    fn mutation_journal(&self) -> crate::mutation::journal::Journal {
        crate::mutation::journal::Journal::new(crate::config::directory().join("mutations.jsonl"))
    }
    /// M7.5: bounded, read-only, local file read -- zero Kubernetes requests.
    fn open_mutations_view(&mut self) -> Result<()> {
        let records = self.mutation_journal().recent(200);
        let text = crate::mutation::view::journal_report(&records);
        self.open_static("Mutation journal", text);
        Ok(())
    }
    fn open_document(&mut self, action: Action) -> Result<()> {
        let object = self.state.selected_object().context("Select a row first")?;
        let resource = self
            .state
            .resource
            .clone()
            .context("No resource selected")?;
        let mut doc = Document::new(format!("{action:?}: {}", object.name), String::new());
        doc.source = Some(document::Source {
            resource,
            selected: object,
            action,
            warning_only: false,
        });
        self.state.mode = Mode::Document(doc);
        self.refresh_document()
    }
    fn refresh_document(&mut self) -> Result<()> {
        if self
            .active_document_mut()
            .is_some_and(|doc| doc.forward_manager)
        {
            self.update_forwards();
            return Ok(());
        }
        if let Some(request) = self
            .active_document_mut()
            .and_then(|d| d.log_request.clone())
        {
            return self.start_logs(request);
        }
        if let Some(request) = self
            .active_document_mut()
            .and_then(|d| d.exec_request.clone())
        {
            anyhow::ensure!(
                !self.state.settings.readonly,
                "Exec is unavailable in read-only mode"
            );
            return self.start_exec(request);
        }
        if let Some(uid) = self
            .active_document_mut()
            .and_then(|d| d.timeline_for.clone())
        {
            let text = self.timeline_text(&uid);
            if let Some(doc) = self.active_document_mut() {
                doc.replace(text);
                doc.freshness = document::Freshness::Snapshot(chrono::Utc::now());
            }
            return Ok(());
        }
        if self
            .active_document_mut()
            .and_then(|d| d.source.as_ref())
            .is_some_and(|s| matches!(s.action, Action::Adjacent | Action::Xray))
        {
            let source = self
                .active_document_mut()
                .and_then(|d| d.source.clone())
                .context("No document open")?;
            return self.start_adjacent(source);
        }
        let connection = self.connection.clone().context("Not connected")?;
        let doc = self.active_document_mut().context("No document open")?;
        let source = doc
            .source
            .clone()
            .context("This local document or log has no refresh source; reopen it")?;
        doc.freshness = document::Freshness::Refreshing;
        self.document.cancel();
        self.document = self.scope.child_token();
        self.state.request += 1;
        let request = self.state.request;
        let epoch = self.state.epoch;
        let tx = self.tx.clone();
        let cancel = self.document.clone();
        // Metrics are descriptive evidence, not fetched fresh on demand -- this
        // snapshots whatever the view's own collector last successfully sampled,
        // taken now (not baked into Source) so a later Refresh sees a newer
        // sample rather than replaying whatever was current when first opened.
        let metrics = (source.action == Action::Explain).then(|| {
            (
                self.state.metrics.amount(&source.selected, true),
                self.state.metrics.amount(&source.selected, false),
            )
        });
        self.tasks.spawn(async move {
            let document::Source { resource, selected:object, action, warning_only } = source;
            let result=tokio::select!{biased;_=cancel.cancelled()=>return,result=crate::kube::evidence::document(&connection,&resource,&object,action,warning_only,metrics)=>result};
            let payload=match result{Ok(text)=>Payload::Document{request,title:format!("{action:?}: {}",object.name),text,adjacent:vec![]},Err(e)=>Payload::DocumentError{request,error:e.to_string()}};
            tokio::select!{_=cancel.cancelled()=>{},_=tx.send(Event{epoch,payload})=>{}}
        });
        Ok(())
    }
    /// Adjacent and Xray share this path instead of the generic per-action
    /// `kube::evidence::document`, since both return a structured graph the
    /// document must keep as navigable targets, not a plain evidence string.
    fn start_adjacent(&mut self, source: document::Source) -> Result<()> {
        let connection = self.connection.clone().context("Not connected")?;
        if let Some(doc) = self.active_document_mut() {
            doc.freshness = document::Freshness::Refreshing;
        }
        self.document.cancel();
        self.document = self.scope.child_token();
        self.state.request += 1;
        let request = self.state.request;
        let epoch = self.state.epoch;
        let scope = self.state.epoch;
        let tx = self.tx.clone();
        let cancel = self.document.clone();
        self.tasks.spawn(async move {
            let document::Source { resource, selected: object, action, .. } = source;
            let result = if action == Action::Xray {
                tokio::select! {
                    biased;
                    _ = cancel.cancelled() => return,
                    result = crate::kube::relationships::report::xray(&connection, scope, &resource, &object, &cancel, 2) => result,
                }
            } else {
                tokio::select! {
                    biased;
                    _ = cancel.cancelled() => return,
                    result = crate::kube::relationships::report::adjacent(&connection, scope, &resource, &object, &cancel) => result,
                }
            };
            let payload = match result {
                Ok(report) => {
                    let (text, adjacent) = if action == Action::Xray {
                        crate::xray::report(&report, 2)
                    } else {
                        crate::adjacent::report(&report)
                    };
                    Payload::Document {
                        request,
                        title: format!("{}: {}", if action == Action::Xray { "Xray" } else { "Adjacent" }, object.name),
                        text,
                        adjacent,
                    }
                }
                Err(e) => Payload::DocumentError {
                    request,
                    error: e.to_string(),
                },
            };
            tokio::select! { _ = cancel.cancelled() => {}, _ = tx.send(Event { epoch, payload }) => {} }
        });
        Ok(())
    }
    /// Jump to the currently selected Adjacent/Xray target (moved with Up/Down,
    /// never ambiguous even when the whole report fits on screen), via its
    /// exact canonical identity (UID-verified on arrival by the normal watch/
    /// rebuild path) -- never by name alone. Reuses the same history stack as
    /// `[`/`]` so this is a normal, reversible navigation, not a special case.
    fn follow_adjacent(&mut self) -> Result<()> {
        let doc = self.active_document_mut().context("No document open")?;
        let target = doc
            .adjacent
            .get(doc.adjacent_selected)
            .cloned()
            .context("No related object in this document")?;
        let context = self
            .connection
            .as_ref()
            .context("Not connected")?
            .context
            .clone();
        self.push_history();
        let entry = state::HistoryEntry {
            context,
            namespace: target
                .resource
                .namespaced
                .then_some(target.namespace.clone()),
            resource: target.resource,
            labels: None,
            fields: None,
            filter_text: String::new(),
            sort: "NAME".into(),
            descending: false,
            selected: Some(target.uid),
        };
        self.apply_history(entry);
        Ok(())
    }
    fn open_logs(&mut self, container: Option<String>, previous: bool) -> Result<()> {
        let object = self.state.selected_object().context("Select a Pod first")?;
        self.start_logs(crate::kube::logs::Request {
            objects: vec![object],
            options: crate::kube::logs::LogOptions {
                container,
                previous,
            },
        })
    }
    fn start_logs(&mut self, request: crate::kube::logs::Request) -> Result<()> {
        let sources = crate::kube::logs::sources(&request)?;
        let object = sources.first().context("No log sources")?.object.clone();
        let connection = self.connection.clone().context("Not connected")?;
        self.document.cancel();
        self.document = self.scope.child_token();
        self.state.request += 1;
        let mut doc = Document::new(
            format!(
                "Logs: {} · {} sources · timestamps · {}",
                object.name,
                sources.len(),
                if request.options.previous {
                    "previous"
                } else {
                    "follow"
                }
            ),
            String::new(),
        );
        doc.follow = true;
        doc.log_request = Some(request.clone());
        let scope = session::Scope {
            epoch: self.state.epoch,
            request: self.state.request,
            context: connection.context.clone(),
            cluster: connection.cluster.clone(),
            resource: "v1/pods".into(),
            namespace: object.namespace.clone(),
            name: object.name.clone(),
            uid: object.uid.clone(),
        };
        let epoch = self.state.epoch;
        let request_id = self.state.request;
        let tx = self.tx.clone();
        let id = self
            .sessions
            .spawn(session::Kind::Logs, scope, self.document.clone(), |id| {
                crate::kube::logs::run(connection, request, epoch, request_id, tx, id)
            })?;
        doc.session = Some(id);
        doc.session_state = Some(session::State::Starting);
        self.state.mode = Mode::Document(doc);
        Ok(())
    }
    /// One-shot, non-interactive exec: structured argv, explicit container, UID-pinned.
    /// Interactive TTY shell needs a terminal-handoff mechanism this does not attempt
    /// (see docs/EXEC.md); callers must never reach this path when `settings.readonly`.
    fn open_exec(&mut self, container: Option<String>, command: Vec<String>) -> Result<()> {
        let object = self.state.selected_object().context("Select a Pod first")?;
        self.start_exec(crate::kube::exec::Request {
            object,
            container,
            command,
        })
    }
    fn start_exec(&mut self, request: crate::kube::exec::Request) -> Result<()> {
        let target_container = crate::kube::exec::container(&request)?;
        let object = request.object.clone();
        let connection = self.connection.clone().context("Not connected")?;
        self.document.cancel();
        self.document = self.scope.child_token();
        self.state.request += 1;
        let mut doc = Document::new(
            format!(
                "Exec: {}/{} · {}",
                object.name,
                target_container,
                request.command.join(" ")
            ),
            String::new(),
        );
        doc.follow = true;
        doc.exec_request = Some(request.clone());
        let scope = session::Scope {
            epoch: self.state.epoch,
            request: self.state.request,
            context: connection.context.clone(),
            cluster: connection.cluster.clone(),
            resource: "v1/pods".into(),
            namespace: object.namespace.clone(),
            name: object.name.clone(),
            uid: object.uid.clone(),
        };
        let epoch = self.state.epoch;
        let request_id = self.state.request;
        let tx = self.tx.clone();
        let id = self
            .sessions
            .spawn(session::Kind::Exec, scope, self.document.clone(), |id| {
                crate::kube::exec::run(connection, request, epoch, request_id, tx, id)
            })?;
        doc.session = Some(id);
        doc.session_state = Some(session::State::Starting);
        self.state.mode = Mode::Document(doc);
        Ok(())
    }
    /// Queues an interactive shell/attach request; `run()` picks it up right after
    /// this key/command dispatch returns and does the actual terminal handoff.
    /// Does nothing to `state.mode`/`connection`/`document` here -- unlike every
    /// other document-opening action, these are not a `Mode::Document` and do not
    /// touch the table's current view at all while they run.
    fn open_shell(&mut self, container: Option<String>, shell: Option<String>) -> Result<()> {
        let object = self.state.selected_object().context("Select a Pod first")?;
        let request = crate::kube::exec::ShellRequest {
            object,
            container,
            shell,
        };
        crate::kube::exec::shell_container(&request)?;
        self.pending_interactive = Some(Interactive::Shell(request));
        Ok(())
    }
    fn open_attach(&mut self, container: Option<String>) -> Result<()> {
        let object = self.state.selected_object().context("Select a Pod first")?;
        let request = crate::kube::exec::AttachRequest { object, container };
        let resolved = crate::kube::exec::attach_container(&request)?;
        anyhow::ensure!(
            crate::kube::exec::container_supports_interactive_attach(&request.object, &resolved),
            "Container {resolved} was not started with stdin+tty (required for an \
             interactive attach); use :logs to view its output instead"
        );
        self.pending_interactive = Some(Interactive::Attach(request));
        Ok(())
    }
    fn take_pending_interactive(&mut self) -> Option<Interactive> {
        self.pending_interactive.take()
    }
    fn session_finished(&mut self, record: session::Record) {
        if record.kind == session::Kind::PortForward {
            // Durable session identity, never current context/epoch. No forward
            // event can become row/document data in the foreground view.
            if self
                .forwards
                .entries
                .get(&record.id)
                .is_some_and(|entry| entry.scope == record.scope)
            {
                self.update_forwards();
            }
            return;
        }
        if record.scope.epoch != self.state.epoch || record.scope.request != self.state.request {
            return;
        }
        if let Some(doc) = self.active_document_mut()
            && doc.session == Some(record.id)
        {
            doc.streaming = false;
            doc.session_state = Some(record.state);
            self.state.dirty = true;
        }
    }
    pub fn info(&self) -> String {
        let mut out = format!(
            "{} {}\nRead-only: {}\nContext: {}\nResource: {}\nRows: {}\nCache estimated JSON bytes: {}\nWatch errors: {}\nActive tasks: {}\nQueue capacity: {} / 256\n",
            crate::brand::NAME,
            crate::brand::VERSION,
            self.state.settings.readonly,
            self.state.context,
            self.state.query.resource,
            self.state.store.objects.len(),
            self.state.store.bytes(),
            self.state.watch_errors,
            self.tasks.len(),
            self.tx.capacity()
        );
        if let Some(c) = &self.connection {
            out.push_str(&format!(
                "Kubernetes: {}\nDiscovered resources: {}\nDiscovery warnings:\n{}\n",
                c.version,
                c.catalog.resources.len(),
                c.catalog.warnings.join("\n")
            ));
        }
        out.push_str(&format!(
            "Active sessions: {}\n",
            self.sessions.active_count()
        ));
        out.push_str(&format!(
            "Active forwards: {}\n",
            self.forwards.active_count(&self.sessions)
        ));
        out.push_str(&format!(
            "Metrics requests started: {}\nMetrics request active: {}\n{}\n",
            self.metrics_generation,
            self.metrics_inflight,
            self.state.metrics.summary()
        ));
        if let Some(object) = self.state.selected_object() {
            out.push_str(&self.state.metrics.report(&object));
        }
        crate::safety::text(&out)
    }
    pub async fn shutdown(&mut self) {
        self.scope.cancel();
        self.document.cancel();
        self.sessions.shutdown().await;
        self.tasks.abort_all();
        while self.tasks.join_next().await.is_some() {}
    }
    fn pick_forward(&mut self) -> Result<()> {
        anyhow::ensure!(
            !self.options.force_readonly && !self.state.settings.readonly,
            "Port forwarding is unavailable in read-only mode"
        );
        let connection = self.connection.clone().context("Not connected")?;
        let object = self.state.selected_object().context("Select a Pod first")?;
        crate::kube::forward::validate_target(&object)?;
        let ports = crate::kube::forward::declared_ports(&object);
        anyhow::ensure!(
            !ports.is_empty(),
            "No declared TCP ports; use :forward REMOTE or :forward LOCAL:REMOTE"
        );
        self.document.cancel();
        self.state.request += 1;
        self.pending_forward = Some((connection, object));
        self.state.mode = Mode::Picker(state::Picker {
            kind: state::PickerKind::Port,
            title:
                "Pod TCP port · Enter uses automatic loopback port · custom: :forward LOCAL:REMOTE"
                    .into(),
            items: ports.iter().map(ToString::to_string).collect(),
            active: None,
            cursor: 0,
        });
        Ok(())
    }
    fn start_forward(
        &mut self,
        connection: Connection,
        object: crate::resources::SharedObject,
        ports: crate::kube::forward::Ports,
    ) -> Result<()> {
        anyhow::ensure!(
            !self.options.force_readonly
                && !self.state.settings.readonly
                && !connection.settings.readonly,
            "Port forwarding is unavailable in read-only mode"
        );
        crate::kube::forward::validate_target(&object)?;
        anyhow::ensure!(
            self.forwards.active_count(&self.sessions) < crate::kube::forward::MAX_FORWARDS,
            "Forward limit reached (4); stop one with :pf_stop ID"
        );
        let scope = session::Scope {
            epoch: self.state.epoch,
            request: self.state.request,
            context: connection.context.clone(),
            cluster: connection.cluster.clone(),
            resource: "v1/pods".into(),
            namespace: object.namespace.clone(),
            name: object.name.clone(),
            uid: object.uid.clone(),
        };
        let (tx, rx) = tokio::sync::watch::channel(crate::kube::forward::Progress::default());
        // Deliberately independent of scope/document tokens; Sessions owns shutdown.
        let id = self.sessions.spawn(
            session::Kind::PortForward,
            scope.clone(),
            CancellationToken::new(),
            |_| async move {
                let result = crate::kube::forward::run(connection, object, ports, tx.clone()).await;
                tx.send_modify(|p| {
                    p.ended = true;
                    p.clients = 0;
                    if let Err(error) = &result {
                        p.last_error = Some(error.clone());
                    }
                });
                match result {
                    Ok(()) => session::Outcome::Completed,
                    Err(error) => session::Outcome::Failed(error.to_string()),
                }
            },
        )?;
        self.forwards.prune(&self.sessions);
        self.forwards.entries.insert(
            id,
            forwards::Entry {
                scope,
                ports,
                started: std::time::Instant::now(),
                progress: rx,
            },
        );
        self.action(Action::ForwardManager)?;
        self.update_forwards();
        Ok(())
    }
    fn update_forwards(&mut self) {
        for (id, entry) in &self.forwards.entries {
            let progress = entry.progress.borrow();
            if progress.local.is_some() && !progress.ended {
                self.sessions.running(*id);
            }
        }
        self.state.forward_count = self.forwards.active_count(&self.sessions);
        if self
            .active_document_mut()
            .is_some_and(|doc| doc.forward_manager)
        {
            let text = self.forwards.document(&self.sessions);
            if let Some(doc) = self.active_document_mut() {
                doc.replace(text);
            }
        }
        self.state.dirty = true;
    }

    fn poll_metrics(&mut self) {
        self.state.metrics.expire();
        if self.metrics_inflight
            || !self.state.synced
            || std::time::Instant::now() < self.metrics_due
        {
            return;
        }
        let Some(resource) = self
            .state
            .resource
            .clone()
            .filter(crate::kube::metrics::supported)
        else {
            return;
        };
        let Some(connection) = self.connection.clone() else {
            return;
        };
        let pins = self
            .state
            .store
            .objects
            .iter()
            .take(crate::kube::metrics::MAX_SAMPLES)
            .map(|(slot, o)| {
                (
                    slot.clone(),
                    crate::kube::metrics::Pin {
                        uid: o.uid.clone(),
                        created: o.created,
                    },
                )
            })
            .collect();
        self.metrics_generation += 1;
        let generation = self.metrics_generation;
        let epoch = self.state.epoch;
        let namespace = self.state.query.namespace.clone();
        let tx = self.tx.clone();
        let cancel = self.scope.clone();
        self.metrics_inflight = true;
        self.tasks.spawn(async move {
            let result = tokio::select! { biased; _ = cancel.cancelled() => return, result = crate::kube::metrics::fetch(&connection, &resource, namespace.as_deref()) => result };
            tokio::select! { biased; _ = cancel.cancelled() => {}, _ = tx.send(Event { epoch, payload: Payload::Metrics { generation, pins, result } }) => {} }
        });
    }
}

/// The namespace to switch to for `context`: whatever this session last viewed there,
/// or `Some("")` (a sentinel `Payload::Connected` resolves to the connection's kubeconfig
/// default namespace) the first time this context is visited.
fn namespace_for_context(
    memory: &std::collections::HashMap<String, Option<String>>,
    context: &str,
) -> Option<String> {
    memory
        .get(context)
        .cloned()
        .unwrap_or_else(|| Some(String::new()))
}

/// Push onto a bounded back/forward stack, dropping the oldest entry once `cap` would
/// otherwise be exceeded, so a long session's history can never grow unbounded.
fn push_capped(
    stack: &mut std::collections::VecDeque<state::HistoryEntry>,
    entry: state::HistoryEntry,
    cap: usize,
) {
    stack.push_back(entry);
    while stack.len() > cap {
        stack.pop_front();
    }
}

struct TerminalGuard;
impl Drop for TerminalGuard {
    fn drop(&mut self) {
        ratatui::restore();
    }
}
pub async fn run(mut runtime: Runtime, mut rx: mpsc::Receiver<Event>) -> Result<()> {
    anyhow::ensure!(
        std::io::stdin().is_terminal() && std::io::stdout().is_terminal(),
        "Interactive mode needs a TTY; use --snapshot, --check or info --offline"
    );
    let mut terminal = ratatui::try_init()?;
    let _guard = TerminalGuard;
    let mut input = EventStream::new();
    let mut frames = tokio::time::interval(Duration::from_millis(33));
    frames.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut clock = tokio::time::interval(Duration::from_secs(1));
    runtime.connect(None);
    // Set right after resuming from an interactive shell or attach; cleared on
    // the very next key regardless. Works around a real, root-caused limitation
    // (see docs/EXEC.md): whenever `run_interactive()`'s forwarding loop ends
    // with a local stdin read still in flight (the common case: the OTHER
    // select! branch, remote closure, wins the race -- observed after both a
    // typed "exit" and a local Ctrl-D, so this is not reliably tied to which
    // side initiates the close), that read is left orphaned -- tokio::io::Stdin's
    // blocking-pool read cannot be cancelled on Unix. It silently consumes
    // exactly the next available chunk of real stdin data (byte-traced live:
    // observed eating an entire subsequently-typed command) before
    // self-terminating, after which a
    // trailing bare Enter is what the fresh EventStream actually sees and
    // delivers. Discarding that one bare Enter converts what would otherwise be
    // a wrong action (opening the wrong document under a misdelivered Enter)
    // into a safe no-op -- the user notices nothing happened and retypes,
    // rather than SAURON doing something unintended. It does not recover the
    // swallowed text; that needs a cancellation-safe stdin reader (e.g. a
    // non-blocking fd wrapped in tokio::io::unix::AsyncFd) to fix completely,
    // which is future work, not shipped here.
    let mut suppress_phantom_enter = false;
    let result=async {
        while !runtime.state.quit {
            tokio::select! {
                biased;
                _=tokio::signal::ctrl_c()=>{runtime.state.quit=true;},
                event=input.next()=>{
                    match event {
                    Some(Ok(TerminalEvent::Key(key))) if key.kind!=KeyEventKind::Release=>{
                        let suppressed=std::mem::take(&mut suppress_phantom_enter);
                        if suppressed && key.code==KeyCode::Enter && key.modifiers.is_empty(){continue;}
                        if let Some(action)=runtime.state.keymap.action(key,"input")&& action==Action::Quit{runtime.state.quit=true;continue;}
                        let mode=std::mem::replace(&mut runtime.state.mode,Mode::Table);
                        let command_mode=matches!(mode,Mode::Command(_));
                        match mode {
                            Mode::Command(mut text)|Mode::Filter(mut text)=>{
                                let command=command_mode;
                                match key.code {
                                    KeyCode::Esc=>{runtime.state.input_error=None;if command{runtime.close_palette();}},
                                    KeyCode::Enter=>{
                                        if command{runtime.close_palette();}
                                        let result=if command{runtime.command(&text)}else{runtime.state.set_filter(&text)};
                                        if let Err(e)=result {if command{runtime.state.error=Some(e.to_string());runtime.open_palette(text);}else{runtime.state.input_error=Some(e.to_string());runtime.state.mode=Mode::Filter(text);}}
                                    },
                                    KeyCode::Tab if command=>{if let Some(s)=runtime.suggestions(&text).first(){text=s.clone();}runtime.state.mode=Mode::Command(text);},
                                    _=>{edit(&mut text,key);runtime.state.mode=if command{Mode::Command(text)}else{Mode::Filter(text)};},
                                }
                            },
                            Mode::Picker(mut picker)=>{
                                match key.code{
                                    KeyCode::Esc=>{runtime.state.mode=Mode::Table;},
                                    KeyCode::Up=>{picker.cursor=picker.cursor.saturating_sub(1);runtime.state.mode=Mode::Picker(picker);},
                                    KeyCode::Down=>{picker.cursor=(picker.cursor+1).min(picker.items.len().saturating_sub(1));runtime.state.mode=Mode::Picker(picker);},
                                    KeyCode::Enter=>{
                                        if let Some(item)=picker.items.get(picker.cursor).cloned(){
                                            let result=match picker.kind{
                                                state::PickerKind::Context=>{runtime.switch_context(item);Ok(())},
                                                state::PickerKind::Namespace=>runtime.switch_namespace((item!="<all>").then_some(item)),
                                                state::PickerKind::Port=>{
                                                    match runtime.pending_forward.take() {
                                                        Some((connection, object))=>crate::kube::forward::Ports::parse(&item).and_then(|ports| runtime.start_forward(connection,object,ports)),
                                                        None=>Err(anyhow::anyhow!("Port selection expired; select the Pod again")),
                                                    }
                                                },
                                            };
                                            runtime.state.input_error=result.err().map(|e|e.to_string());
                                        }
                                    },
                                    _=>{runtime.state.mode=Mode::Picker(picker);},
                                }
                            },
                            Mode::Search(mut doc,mut text)=>{
                                match key.code{KeyCode::Esc=>runtime.state.mode=Mode::Document(doc),KeyCode::Enter=>{doc.set_search(text);runtime.state.mode=Mode::Document(doc);},_=>{edit(&mut text,key);runtime.state.mode=Mode::Search(doc,text);}}
                            },
                            // A key pressed while an async fetch (connect/watch/namespace list) is
                            // still in flight must never fall through to table-action dispatch: the
                            // selected row it would act on belongs to whatever was on screen before
                            // this fetch started, not to what the user is currently waiting for.
                            // Esc still cancels, matching the "Esc cancels" text shown while loading.
                            Mode::Loading=>{
                                runtime.state.mode=Mode::Loading;
                                if key.code==KeyCode::Esc{
                                    runtime.document.cancel();
                                    runtime.state.request+=1;
                                    runtime.state.mode=Mode::Table;
                                }
                            },
                            mode=>{
                                runtime.state.mode=mode;
                                let mode_name=runtime.mode_name();
                                // A per-action validation error (e.g. "select a row first",
                                // "turn wrapping off before horizontal scrolling") is transient
                                // input feedback, not a backend/transport problem -- it must not
                                // reuse state.error, which a real WatchError/ConnectError also
                                // sets and which must keep surviving unrelated successful key
                                // presses until the watch actually recovers. Clearing on success
                                // here (mirroring State::set_filter for input_error) ensures a
                                // one-off rejection doesn't linger and mask this key's own status
                                // (e.g. a document's line/search status) after later succeeding.
                                if let Some(action)=runtime.state.keymap.action(key,mode_name){
                                    runtime.state.input_error=runtime.action(action).err().map(|e|e.to_string());
                                }
                            },
                        }
                        runtime.state.dirty=true;
                    },
                    Some(Ok(TerminalEvent::Resize(..)))=>runtime.state.dirty=true,
                    Some(Err(e))=>return Err(e.into()),None=>break,_=>{}
                }},
                _=frames.tick()=>if runtime.state.dirty {
                    runtime.state.prepare();let suggestions=if let Mode::Command(text)=&runtime.state.mode{runtime.suggestions(text)}else{vec![]};
                    terminal.draw(|frame|crate::ui::render(frame,&mut runtime.state,&suggestions))?;
                    runtime.state.dirty=false;
                },
                _=clock.tick()=>{ runtime.poll_metrics(); runtime.update_forwards(); },
                Some(record)=runtime.sessions.join_next(),if !runtime.sessions.is_empty()=>runtime.session_finished(record),
                Some(event)=rx.recv()=>{runtime.reduce(event);for _ in 0..63{match rx.try_recv(){Ok(event)=>runtime.reduce(event),Err(_)=>break}}},
                Some(result)=runtime.tasks.join_next(),if !runtime.tasks.is_empty()=>{if result.is_err(){runtime.state.error=Some("Background task failed; refresh to retry".into());runtime.state.dirty=true;}},
            }
            if let Some(request) = runtime.take_pending_interactive() {
                let (new_input, status) =
                    run_interactive(&mut terminal, input, &runtime, request).await?;
                input = new_input;
                runtime.state.status = status;
                runtime.state.dirty = true;
                suppress_phantom_enter = true;
            }
        }
        Ok(())
    }.await;
    runtime.shutdown().await;
    result
}
/// Suspends Ratatui's alternate screen, forwards the real terminal to one
/// interactive remote shell until it ends by any means, then restores. Owns the
/// full lifecycle of that handoff: dropping the old `EventStream` before raw
/// stdin/stdout access starts, and always returning a fresh one so `run()`'s loop
/// resumes normal key handling regardless of how the shell ended (clean exit,
/// Ctrl-D, remote disconnect, or a local I/O error -- the `TerminalHandoff` guard
/// restores the alternate screen even on that last, error, path).
async fn run_interactive(
    terminal: &mut ratatui::DefaultTerminal,
    input: EventStream,
    runtime: &Runtime,
    request: Interactive,
) -> Result<(EventStream, String)> {
    drop(input);
    let label = match &request {
        Interactive::Shell(_) => "Shell",
        Interactive::Attach(_) => "Attach",
    };
    let outcome = {
        let _handoff = terminal::TerminalHandoff::enter()?;
        match &runtime.connection {
            None => Err(anyhow::anyhow!("Not connected")),
            Some(connection) => match request {
                Interactive::Shell(request) => {
                    crate::kube::exec::interactive(
                        connection,
                        &request,
                        tokio::io::stdin(),
                        tokio::io::stdout(),
                    )
                    .await
                }
                Interactive::Attach(request) => {
                    crate::kube::exec::attach(
                        connection,
                        &request,
                        tokio::io::stdin(),
                        tokio::io::stdout(),
                    )
                    .await
                }
            },
        }
    };
    // The alternate screen was just re-entered (guard dropped above); Ratatui's
    // own diff buffer does not know that, so force a full repaint next frame
    // rather than only redrawing what it thinks changed since the session started.
    // Deliberately `resize()`, not `Terminal::clear()`: for a Fullscreen viewport
    // resize() clears without querying the backend's cursor position, while
    // clear() always does (to restore it afterward) -- that query sends a DSR
    // escape sequence and blocks reading stdin for a response, which can race a
    // just-ended interactive session's own stdin reader and time out (found live;
    // see docs/EXEC.md).
    let size = terminal.size()?;
    terminal.resize(ratatui::layout::Rect::new(0, 0, size.width, size.height))?;
    let status = match outcome {
        Ok(session::Outcome::Completed) => format!("{label} ended"),
        Ok(other) => format!("{label} {}", session::State::Ended(other).label()),
        Err(e) => format!("{label} failed: {e}"),
    };
    // Root cause (found live, confirmed by tracing every received key event): a
    // byte actually intended for the remote shell can land, mid-session, in
    // crossterm's own process-wide event reader instead of our raw stdin forward
    // -- that reader's background thread only shuts down asynchronously once this
    // scope's `input` is dropped above, and any byte it manages to consume before
    // then is buffered in a *shared, static* queue that survives the drop and is
    // NOT tied to any one `EventStream` instance. A freshly constructed
    // `EventStream` checks that same shared queue first, so it silently replays
    // as a real keypress against the just-resumed table (observed concretely:
    // a stray `Enter`, bound to Yaml in table mode, immediately reopening a
    // document instead of the next typed command). Flushing it here with
    // crossterm's synchronous, non-blocking poll/read (zero timeout: only drains
    // what is *already* buffered, never waits for more) is safe unconditionally
    // -- unlike an async settle-window, it cannot swallow a key the user types
    // after this point, since it does not wait at all.
    while crossterm::event::poll(Duration::ZERO).unwrap_or(false) {
        if crossterm::event::read().is_err() {
            break;
        }
    }
    Ok((EventStream::new(), status))
}
impl Runtime {
    fn suggestions(&self, text: &str) -> Vec<String> {
        let mut names: Vec<String> = crate::command::command_names()
            .into_iter()
            .map(str::to_string)
            .collect();
        if let Some(c) = &self.connection {
            names.extend(c.catalog.resources.iter().map(|r| r.qualified()));
        }
        names.retain(|name| crate::filters::fuzzy(text, name));
        names.sort_by_key(|name| (!name.starts_with(text), name.clone()));
        names.truncate(8);
        names
    }
}
fn edit(text: &mut String, key: crossterm::event::KeyEvent) {
    match key.code {
        KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => text.clear(),
        KeyCode::Backspace => {
            text.pop();
        }
        KeyCode::Char(c)
            if !key
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
                && text.len() < 4096 =>
        {
            text.push(c)
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn metrics_epochs_generations_and_single_inflight_request_are_owned() {
        use crate::evidence::Unknown;
        let mut rt = runtime();
        rt.state.synced = true;
        for _ in 0..20 {
            rt.poll_metrics();
        }
        assert_eq!(rt.metrics_generation, 1);
        assert!(rt.metrics_inflight);
        let old_epoch = rt.state.epoch;
        rt.cancel_scope();
        rt.state.metrics.status = Err(Unknown::Forbidden);
        rt.reduce(Event {
            epoch: old_epoch,
            payload: Payload::Metrics {
                generation: 1,
                pins: Default::default(),
                result: Err(Unknown::Unavailable),
            },
        });
        assert_eq!(rt.state.metrics.status, Err(Unknown::Forbidden));
        rt.metrics_generation = 2;
        rt.state.synced = true;
        rt.reduce(Event {
            epoch: rt.state.epoch,
            payload: Payload::Metrics {
                generation: 1,
                pins: Default::default(),
                result: Err(Unknown::Unavailable),
            },
        });
        assert_eq!(rt.state.metrics.status, Err(Unknown::Forbidden));
        rt.reduce(Event {
            epoch: rt.state.epoch,
            payload: Payload::Metrics {
                generation: 2,
                pins: Default::default(),
                result: Err(Unknown::Unavailable),
            },
        });
        assert_eq!(rt.state.metrics.status, Err(Unknown::Unavailable));
        assert!(rt.metrics_due > std::time::Instant::now());
        rt.reduce(Event {
            epoch: rt.state.epoch,
            payload: Payload::WatchError("watch lost".into()),
        });
        rt.reduce(Event {
            epoch: rt.state.epoch,
            payload: Payload::Metrics {
                generation: 2,
                pins: Default::default(),
                result: Ok(crate::kube::metrics::Batch {
                    samples: Default::default(),
                    coverage: Default::default(),
                }),
            },
        });
        assert_eq!(rt.state.metrics.status, Err(Unknown::Stale));
        tokio::time::timeout(Duration::from_secs(3), rt.shutdown())
            .await
            .expect("cancelled collector cleanup");
        assert!(rt.tasks.is_empty());
    }
    #[tokio::test]
    async fn forwarding_policy_denies_before_target_resolution_and_reload_preserves_cli() {
        let mut rt = runtime();
        rt.connection = None;
        assert!(
            rt.command("forward 8080")
                .expect_err("readonly")
                .to_string()
                .contains("read-only")
        );
        rt.state.settings.readonly = false;
        rt.options.force_readonly = true;
        assert!(
            rt.command("forward 8080")
                .expect_err("forced readonly")
                .to_string()
                .contains("read-only")
        );
        assert!(
            rt.action(Action::Forward)
                .expect_err("picker gate")
                .to_string()
                .contains("read-only")
        );
        assert_eq!(rt.sessions.active_count(), 0);
        rt.config_path = Some("tests/fixtures/operational.toml".into());
        let mut connection = runtime().connection.take().expect("fixture connection");
        connection.context = "kind-sauron-test".into();
        rt.connection = Some(connection);
        rt.command("reload").expect("reload with CLI override");
        assert!(rt.state.settings.readonly);
        assert!(
            rt.connection
                .as_ref()
                .expect("connection")
                .settings
                .readonly
        );
        rt.options.force_readonly = false;
        rt.command("reload").expect("explicit operational context");
        assert!(!rt.state.settings.readonly);
        rt.shutdown().await;
    }
    #[tokio::test]
    async fn background_session_survives_view_cancel_and_completion_cannot_replace_document() {
        let mut rt = runtime();
        rt.state.mode = Mode::Document(Document::new("current document".into(), "current".into()));
        let scope = session::Scope {
            epoch: rt.state.epoch,
            request: rt.state.request,
            context: "origin".into(),
            cluster: "origin".into(),
            resource: "v1/pods".into(),
            namespace: "n".into(),
            name: "p".into(),
            uid: "pinned".into(),
        };
        let cancel = CancellationToken::new();
        let id = rt
            .sessions
            .spawn(
                session::Kind::PortForward,
                scope.clone(),
                cancel.clone(),
                |_| std::future::pending(),
            )
            .expect("spawn");
        let (_tx, rx) = tokio::sync::watch::channel(crate::kube::forward::Progress::default());
        rt.forwards.entries.insert(
            id,
            forwards::Entry {
                scope,
                ports: crate::kube::forward::Ports {
                    local: 0,
                    remote: 80,
                },
                started: std::time::Instant::now(),
                progress: rx,
            },
        );
        for _ in 0..10 {
            rt.cancel_scope();
        }
        assert!(!cancel.is_cancelled());
        assert_eq!(rt.forwards.active_count(&rt.sessions), 1);
        rt.state.mode = Mode::Document(Document::new("new scope".into(), "untouched".into()));
        rt.command(&format!("pf_stop {}", id.number()))
            .expect("stop from any scope");
        let record = rt.sessions.join_next().await.expect("completion");
        rt.session_finished(record);
        assert_eq!(rt.forwards.active_count(&rt.sessions), 0);
        let doc = rt.active_document_mut().expect("doc");
        assert_eq!(doc.title, "new scope");
        assert_eq!(doc.lines[0], "untouched");
        // Policy reload evaluates the *original* context, not the operational
        // context currently displayed. It revokes existing access as well.
        let old_entry = rt.forwards.entries.remove(&id).expect("entry");
        let revoked = CancellationToken::new();
        let id = rt
            .sessions
            .spawn(
                session::Kind::PortForward,
                old_entry.scope.clone(),
                revoked.clone(),
                |_| std::future::pending(),
            )
            .expect("new session");
        rt.forwards.entries.insert(id, old_entry);
        rt.connection.as_mut().expect("connection").context = "kind-sauron-test".into();
        rt.config_path = Some("tests/fixtures/operational.toml".into());
        rt.command("reload").expect("reload");
        assert!(!rt.state.settings.readonly);
        assert!(
            revoked.is_cancelled(),
            "original context is readonly despite current view opt-in"
        );
        rt.shutdown().await;
    }
    #[tokio::test]
    async fn connection_completion_preserves_palette_opened_during_startup() {
        let mut rt = runtime();
        let connection = rt.connection.take().expect("connection");
        rt.state.mode = Mode::Command("logs wor".into());
        rt.reduce(Event {
            epoch: rt.state.epoch,
            payload: Payload::Connected(Box::new(connection)),
        });
        assert!(matches!(&rt.state.mode, Mode::Command(text) if text == "logs wor"));
        // Old document ownership must still be discarded, not restored with input.
        assert!(rt.palette_document.is_none());
        assert!(rt.state.store.objects.is_empty());
        rt.shutdown().await;
    }
    #[tokio::test]
    async fn log_completion_follows_document_through_search_and_palette() {
        let mut rt = runtime();
        for overlay in ["search", "palette"] {
            let scope = session::Scope {
                epoch: rt.state.epoch,
                request: rt.state.request,
                context: "test".into(),
                cluster: "test".into(),
                resource: "v1/pods".into(),
                namespace: "test".into(),
                name: "pod".into(),
                uid: "uid".into(),
            };
            let id = rt
                .sessions
                .spawn(
                    session::Kind::Logs,
                    scope,
                    CancellationToken::new(),
                    |_| async { session::Outcome::Failed("connection closed".into()) },
                )
                .expect("session");
            let mut doc = Document::new("logs".into(), String::new());
            doc.session = Some(id);
            doc.streaming = true;
            doc.session_state = Some(session::State::Running);
            rt.state.mode = Mode::Document(doc);
            if overlay == "search" {
                if let Mode::Document(doc) = std::mem::replace(&mut rt.state.mode, Mode::Table) {
                    rt.state.mode = Mode::Search(doc, String::new());
                }
            } else {
                rt.open_palette(String::new());
            }
            let record = rt.sessions.join_next().await.expect("completed");
            rt.session_finished(record);
            // A queued connection notification must never resurrect an ended session.
            rt.reduce(Event {
                epoch: rt.state.epoch,
                payload: Payload::LogStarted {
                    request: rt.state.request,
                    session: id,
                },
            });
            let doc = rt
                .active_document_mut()
                .expect("same document under overlay");
            assert!(!doc.streaming);
            assert!(doc.status().contains("Failed: connection closed"));
            assert!(rt.state.error.is_none());
        }
        rt.shutdown().await;
    }
    #[tokio::test]
    async fn abandoned_log_identity_never_updates_replacement_view() {
        let mut rt = runtime();
        let old_epoch = rt.state.epoch;
        let old_request = rt.state.request;
        let scope = session::Scope {
            epoch: old_epoch,
            request: old_request,
            context: "test".into(),
            cluster: "test".into(),
            resource: "v1/pods".into(),
            namespace: "test".into(),
            name: "pod".into(),
            uid: "old".into(),
        };
        let old = rt
            .sessions
            .spawn(
                session::Kind::Logs,
                scope.clone(),
                CancellationToken::new(),
                |_| async { session::Outcome::Completed },
            )
            .expect("old");
        let new = rt
            .sessions
            .spawn(session::Kind::Logs, scope, CancellationToken::new(), |_| {
                std::future::pending()
            })
            .expect("new");
        let mut doc = Document::new("new logs".into(), String::new());
        doc.session = Some(new);
        doc.session_state = Some(session::State::Starting);
        rt.state.mode = Mode::Document(doc);
        for (epoch, request, session) in [
            (old_epoch, old_request, old),
            (old_epoch + 1, old_request, new),
            (old_epoch, old_request + 1, new),
        ] {
            rt.reduce(Event {
                epoch,
                payload: Payload::LogLine {
                    request,
                    session,
                    line: "stale".into(),
                },
            });
        }
        let old_record = rt.sessions.join_next().await.expect("old ended");
        rt.session_finished(old_record);
        let doc = rt.active_document_mut().expect("new doc");
        assert!(doc.lines.is_empty());
        assert_eq!(doc.session_state, Some(session::State::Starting));
        rt.reduce(Event {
            epoch: old_epoch,
            payload: Payload::LogLine {
                request: old_request,
                session: new,
                line: "current".into(),
            },
        });
        assert_eq!(rt.active_document_mut().expect("doc").lines[0], "current");
        rt.shutdown().await;
    }
    fn runtime() -> Runtime {
        let resource = entry("pods").resource;
        let (mut rt, _) = Runtime::new(
            ConnectOptions::default(),
            Config::default(),
            None,
            Query {
                resource: "pods".into(),
                namespace: Some("test".into()),
                ..Default::default()
            },
        )
        .expect("runtime");
        rt.state.resource = Some(resource.clone());
        rt.connection = Some(Connection {
            client: ::kube::Client::try_from(::kube::Config::new(
                "http://127.0.0.1:1".parse().expect("uri"),
            ))
            .expect("client"),
            context: "test".into(),
            cluster: "test".into(),
            namespace: "test".into(),
            contexts: vec![],
            catalog: crate::kube::discovery::Catalog {
                resources: vec![resource],
                warnings: vec![],
            },
            settings: crate::config::Settings::default(),
            version: "test".into(),
        });
        rt
    }
    #[tokio::test]
    async fn history_keeps_selectors_and_resolved_identity_without_catalog_lookup() {
        let mut rt = runtime();
        rt.state.query.labels = Some("app=api".into());
        rt.state.query.fields = Some("status.phase=Running".into());
        rt.state.filter_text = "restarts>2".into();
        let entry = rt.current_history_entry().expect("entry");
        rt.connection
            .as_mut()
            .expect("connected")
            .catalog
            .resources
            .clear();
        rt.state.query.labels = None;
        rt.state.query.fields = Some("metadata.name=other".into());
        rt.finish_history(entry, false);
        assert!(rt.state.error.is_none());
        assert_eq!(rt.state.query.resource, "v1/pods");
        assert_eq!(rt.state.query.labels.as_deref(), Some("app=api"));
        assert_eq!(
            rt.state.query.fields.as_deref(),
            Some("status.phase=Running")
        );
        assert_eq!(rt.state.filter_text, "restarts>2");
        rt.shutdown().await;
    }
    #[tokio::test]
    async fn invalid_navigation_is_transactional_and_old_epoch_is_ignored() {
        let mut rt = runtime();
        rt.state.query.labels = Some("app=api".into());
        for command in ["pods / /[/", "missing-resource", "pods / restarts>oops"] {
            assert!(rt.command(command).is_err(), "{command}");
            assert!(rt.history.is_empty());
            assert_eq!(rt.state.query.labels.as_deref(), Some("app=api"));
            assert_eq!(rt.state.epoch, 0);
        }
        let mut invalid_entry = rt.current_history_entry().expect("entry");
        invalid_entry.filter_text = "cpu>".into();
        rt.finish_history(invalid_entry, false);
        assert_eq!(rt.state.query.labels.as_deref(), Some("app=api"));
        assert_eq!(rt.state.epoch, 0);
        rt.watch().expect("watch");
        rt.reduce(Event {
            epoch: 0,
            payload: Payload::Apply(
                crate::resources::Object::new(
                    serde_json::json!({"metadata":{"uid":"stale","name":"old"}}),
                ),
                false,
            ),
        });
        rt.reduce(Event {
            epoch: 0,
            payload: Payload::Ready,
        });
        assert!(rt.state.store.objects.is_empty());
        assert!(!rt.state.synced);
        rt.shutdown().await;
    }
    #[tokio::test]
    async fn sort_and_reverse_do_not_start_tasks_or_pollute_history() {
        let mut rt = runtime();
        let epoch = rt.state.epoch;
        let tasks = rt.tasks.len();
        for spec in [
            "sort name:desc",
            "sort count:field:/spec/count:desc",
            "sort field:/spec/enabled",
        ] {
            rt.command(spec).expect("sort");
            rt.action(Action::Reverse).expect("reverse");
            assert_eq!(rt.state.epoch, epoch);
            assert_eq!(rt.tasks.len(), tasks);
            assert!(rt.history.is_empty());
        }
        let sort = rt.state.sort.clone();
        assert!(rt.command("sort name:wrong").is_err());
        assert_eq!(rt.state.sort, sort);
        rt.shutdown().await;
    }
    fn pod_rows(uids: &[&str]) -> Vec<std::sync::Arc<crate::resources::Object>> {
        uids.iter()
            .map(|uid| {
                std::sync::Arc::new(crate::resources::Object::new(serde_json::json!({
                    "apiVersion":"v1","kind":"Pod",
                    "metadata":{"namespace":"test","name":format!("pod-{uid}"),"uid":uid}
                })))
            })
            .collect()
    }
    #[tokio::test]
    async fn toggle_select_marks_and_unmarks_without_moving_cursor() {
        let mut rt = runtime();
        rt.state.rows = pod_rows(&["a", "b"]);
        rt.state.selected = Some("a".into());
        rt.action(Action::ToggleSelect).expect("toggle on");
        assert!(rt.state.selection.contains("a"));
        assert_eq!(rt.state.selection.len(), 1);
        assert_eq!(
            rt.state.selected.as_deref(),
            Some("a"),
            "toggling selection must never move the cursor"
        );
        rt.action(Action::ToggleSelect).expect("toggle off");
        assert!(!rt.state.selection.contains("a"));
        assert!(rt.state.selection.is_empty());
        rt.shutdown().await;
    }
    #[tokio::test]
    async fn toggle_select_without_a_row_errors_and_selects_nothing() {
        let mut rt = runtime();
        assert!(
            rt.action(Action::ToggleSelect).is_err(),
            "with nothing selected, toggle-select must error, never silently no-op"
        );
        assert!(rt.state.selection.is_empty());
        rt.shutdown().await;
    }
    #[tokio::test]
    async fn select_visible_adds_every_row_once_idempotently() {
        let mut rt = runtime();
        rt.state.rows = pod_rows(&["a", "b", "c"]);
        rt.action(Action::SelectVisible).expect("select visible");
        assert_eq!(rt.state.selection.len(), 3);
        for uid in ["a", "b", "c"] {
            assert!(rt.state.selection.contains(uid));
        }
        // Deselect one, then re-run select_visible: must not re-toggle it off,
        // only add what's missing (add is idempotent, unlike toggle).
        rt.state.selected = Some("a".into());
        rt.action(Action::ToggleSelect).expect("toggle a off");
        assert!(!rt.state.selection.contains("a"));
        rt.action(Action::SelectVisible)
            .expect("select visible again");
        assert_eq!(
            rt.state.selection.len(),
            3,
            "re-running select_visible must not deselect b/c"
        );
        assert!(rt.state.selection.contains("a"));
        rt.shutdown().await;
    }
    #[tokio::test]
    async fn select_visible_refuses_explicitly_past_the_bound_keeping_what_fit() {
        let mut rt = runtime();
        let uids: Vec<String> = (0..crate::app::selection::MAX_SELECTION + 5)
            .map(|i| i.to_string())
            .collect();
        let uid_refs: Vec<&str> = uids.iter().map(String::as_str).collect();
        rt.state.rows = pod_rows(&uid_refs);
        rt.action(Action::SelectVisible).expect("select visible");
        assert_eq!(
            rt.state.selection.len(),
            crate::app::selection::MAX_SELECTION,
            "refusal past the bound must keep exactly what fit, never silently drop it"
        );
        assert!(
            rt.state.status.contains("bound"),
            "the refusal must be reported to the user, not silent: {}",
            rt.state.status
        );
        rt.shutdown().await;
    }
    #[tokio::test]
    async fn clear_selection_empties_it() {
        let mut rt = runtime();
        rt.state.rows = pod_rows(&["a", "b"]);
        rt.action(Action::SelectVisible).expect("select visible");
        assert_eq!(rt.state.selection.len(), 2);
        rt.action(Action::ClearSelection).expect("clear");
        assert!(rt.state.selection.is_empty());
        rt.shutdown().await;
    }
    #[tokio::test]
    async fn selection_never_silently_drops_a_stale_target_on_relist() {
        let mut rt = runtime();
        rt.state.rows = pod_rows(&["a", "b"]);
        rt.action(Action::SelectVisible).expect("select visible");
        // Relist without "a" -- simulates deletion. Selection retains it
        // as an explicit stale entry (never silently forgotten, never
        // silently rebound to a same-name different-UID row).
        rt.state.rows = pod_rows(&["b"]);
        let status = rt.state.selection.status(&rt.state.rows);
        assert_eq!(status.present.len(), 1);
        assert_eq!(status.stale.len(), 1);
        assert_eq!(status.stale[0].uid, "a");
        assert_eq!(
            rt.state.selection.len(),
            2,
            "relisting must never mutate the selection itself"
        );
        rt.shutdown().await;
    }
    #[tokio::test]
    async fn context_or_resource_switch_clears_the_whole_selection() {
        let mut rt = runtime();
        rt.state.rows = pod_rows(&["a", "b"]);
        rt.action(Action::SelectVisible).expect("select visible");
        assert_eq!(rt.state.selection.len(), 2);
        rt.cancel_scope();
        assert!(
            rt.state.selection.is_empty(),
            "a context/resource/namespace switch (cancel_scope) must clear the whole selection, \
             matching the existing single-select clearing invariant"
        );
        rt.shutdown().await;
    }
    #[tokio::test]
    async fn selection_does_not_regress_ordinary_single_row_navigation() {
        let mut rt = runtime();
        rt.state.rows = pod_rows(&["a", "b", "c"]);
        rt.state.selected = Some("a".into());
        rt.action(Action::ToggleSelect).expect("toggle a");
        rt.action(Action::Down).expect("move down");
        assert_eq!(
            rt.state.selected.as_deref(),
            Some("b"),
            "cursor navigation is unaffected by the multi-select set"
        );
        assert!(
            rt.state.selection.contains("a"),
            "moving the cursor must not change the selected set"
        );
        rt.shutdown().await;
    }
    #[tokio::test]
    async fn invert_selection_flips_only_currently_visible_rows() {
        let mut rt = runtime();
        rt.state.rows = pod_rows(&["a", "b", "c"]);
        rt.state.selected = Some("a".into());
        rt.action(Action::ToggleSelect).expect("select a");
        rt.action(Action::InvertSelection).expect("invert");
        assert!(
            !rt.state.selection.contains("a"),
            "a was selected, invert deselects it"
        );
        assert!(rt.state.selection.contains("b"));
        assert!(rt.state.selection.contains("c"));
        assert_eq!(rt.state.selection.len(), 2);
        rt.shutdown().await;
    }
    #[tokio::test]
    async fn invert_selection_never_touches_rows_outside_the_visible_set() {
        let mut rt = runtime();
        // "z" is selected but not in the current (filtered) rows -- a
        // stale entry from a prior broader view. Invert must never
        // touch it: it only flips membership within `rows`.
        rt.state.rows = pod_rows(&["a"]);
        rt.action(Action::SelectVisible).expect("select a");
        rt.state
            .selection
            .add(&pod_rows(&["z"])[0])
            .expect("seed a stale-shaped entry");
        rt.action(Action::InvertSelection).expect("invert");
        assert!(
            !rt.state.selection.contains("a"),
            "a was visible and selected, now deselected"
        );
        assert!(
            rt.state.selection.contains("z"),
            "z is outside the visible set and must be left untouched by invert"
        );
        rt.shutdown().await;
    }
    #[tokio::test]
    async fn inspect_selection_opens_a_bounded_read_only_report() {
        let mut rt = runtime();
        rt.state.rows = pod_rows(&["a", "b"]);
        rt.action(Action::SelectVisible).expect("select visible");
        let tasks_before = rt.tasks.len();
        rt.action(Action::InspectSelection).expect("inspect");
        assert_eq!(
            tasks_before,
            rt.tasks.len(),
            "inspecting the selection issues no network request"
        );
        let doc = rt.active_document_mut().expect("doc open");
        assert_eq!(doc.title, "Selection");
        let text = doc.lines.iter().cloned().collect::<Vec<_>>().join("\n");
        assert!(text.contains("2 selected"));
        rt.shutdown().await;
    }
    #[tokio::test]
    async fn inspect_selection_reports_explicitly_when_empty() {
        let mut rt = runtime();
        rt.action(Action::InspectSelection).expect("inspect");
        let doc = rt.active_document_mut().expect("doc open");
        let text = doc.lines.iter().cloned().collect::<Vec<_>>().join("\n");
        assert!(text.contains("(nothing selected)"));
        rt.shutdown().await;
    }
    #[tokio::test]
    async fn policy_view_is_read_only_and_denies_without_verified_cluster() {
        let mut rt = runtime();
        let object = crate::resources::Object::new(serde_json::json!({
            "apiVersion":"v1","kind":"Pod",
            "metadata":{"namespace":"test","name":"p","uid":"root-uid"}
        }));
        rt.state.rows = vec![std::sync::Arc::new(object)];
        rt.state.selected = Some("root-uid".into());
        let tasks_before = rt.tasks.len();
        rt.action(Action::Policy).expect("policy view");
        assert_eq!(
            rt.tasks.len(),
            tasks_before,
            "policy view issues no network task"
        );
        let doc = rt.active_document_mut().expect("doc open");
        let text = doc.lines.iter().cloned().collect::<Vec<_>>().join("\n");
        assert!(text.contains("UnverifiedCluster"));
        assert!(!text.contains("Modify: Allow"));
        rt.shutdown().await;
    }
    #[tokio::test]
    async fn readonly_false_alone_never_implies_cluster_verified_for_mutation() {
        let mut rt = runtime();
        rt.state.settings.readonly = false;
        rt.options.force_readonly = false;
        assert!(
            !rt.options.mutation_test_cluster_verified,
            "verification must default false and never derive from readonly"
        );
        let object = crate::resources::Object::new(serde_json::json!({
            "apiVersion":"v1","kind":"Pod",
            "metadata":{"namespace":"test","name":"p","uid":"root-uid"}
        }));
        rt.state.rows = vec![std::sync::Arc::new(object)];
        rt.state.selected = Some("root-uid".into());
        rt.action(Action::Policy).expect("policy view");
        let doc = rt.active_document_mut().expect("doc open");
        let text = doc.lines.iter().cloned().collect::<Vec<_>>().join("\n");
        assert!(
            text.contains("UnverifiedCluster"),
            "readonly=false alone must still deny as unverified"
        );
        rt.shutdown().await;
    }
    #[tokio::test]
    async fn cluster_verified_flag_alone_does_not_bypass_readonly() {
        let mut rt = runtime();
        rt.state.settings.readonly = true;
        rt.options.mutation_test_cluster_verified = true;
        let object = crate::resources::Object::new(serde_json::json!({
            "apiVersion":"v1","kind":"Pod",
            "metadata":{"namespace":"test","name":"p","uid":"root-uid"}
        }));
        rt.state.rows = vec![std::sync::Arc::new(object)];
        rt.state.selected = Some("root-uid".into());
        rt.action(Action::Policy).expect("policy view");
        let doc = rt.active_document_mut().expect("doc open");
        let text = doc.lines.iter().cloned().collect::<Vec<_>>().join("\n");
        assert!(
            !text.contains("UnverifiedCluster"),
            "verification flag must be honored independently of readonly"
        );
        assert!(
            text.contains("ReadonlyMode") || text.contains("Deny"),
            "readonly must still deny even when the cluster is verified"
        );
        rt.shutdown().await;
    }
    fn deployment_object(uid: &str, replicas: i64) -> crate::resources::Object {
        crate::resources::Object::new(serde_json::json!({
            "apiVersion":"apps/v1","kind":"Deployment",
            "metadata":{"namespace":"test","name":"d","uid":uid},
            "spec":{"replicas":replicas}
        }))
    }
    fn open_deployment_scale_preview(rt: &mut Runtime, uid: &str) {
        let mut resource = rt.state.resource.clone().expect("resource");
        resource.api.kind = "Deployment".into();
        rt.state.resource = Some(resource);
        rt.state.rows = vec![std::sync::Arc::new(deployment_object(uid, 2))];
        rt.state.selected = Some(uid.into());
        rt.command(":scale 5")
            .expect("preview opens even when denied");
    }

    #[tokio::test]
    async fn mutation_confirm_under_readonly_errors_and_sends_zero_requests() {
        let mut rt = runtime();
        rt.state.settings.readonly = true;
        rt.options.mutation_test_cluster_verified = true;
        open_deployment_scale_preview(&mut rt, "uid-1");
        let workflow = rt
            .active_document_mut()
            .and_then(|d| d.workflow.as_ref())
            .expect("workflow set");
        assert_eq!(
            workflow.evaluation.decision,
            crate::mutation::PolicyDecision::Deny,
            "readonly must still deny even when the test cluster is verified"
        );
        let tasks_before = rt.tasks.len();
        assert!(
            rt.action(Action::MutationConfirm).is_err(),
            "denied confirm must error, never silently no-op"
        );
        assert_eq!(
            rt.tasks.len(),
            tasks_before,
            "a denied confirm must send zero requests"
        );
        rt.shutdown().await;
    }

    #[tokio::test]
    async fn mutation_dry_run_under_readonly_sends_zero_requests() {
        let mut rt = runtime();
        rt.state.settings.readonly = true;
        open_deployment_scale_preview(&mut rt, "uid-1");
        let tasks_before = rt.tasks.len();
        assert!(rt.action(Action::MutationDryRun).is_err());
        assert_eq!(rt.tasks.len(), tasks_before);
        rt.shutdown().await;
    }

    #[tokio::test]
    async fn mutation_strong_confirmation_requires_a_second_press_before_any_request() {
        let mut rt = runtime();
        rt.state.settings.readonly = false;
        rt.options.mutation_test_cluster_verified = true;
        let mut resource = rt.state.resource.clone().expect("resource");
        resource.api.kind = "ConfigMap".into();
        rt.state.resource = Some(resource);
        let object = crate::resources::Object::new(serde_json::json!({
            "apiVersion":"v1","kind":"ConfigMap",
            "metadata":{"namespace":"test","name":"c","uid":"uid-1"}
        }));
        rt.state.rows = vec![std::sync::Arc::new(object)];
        rt.state.selected = Some("uid-1".into());
        rt.command(":delete").expect("delete preview opens");
        let workflow = rt
            .active_document_mut()
            .and_then(|d| d.workflow.as_ref())
            .expect("workflow set");
        assert_eq!(
            workflow.requirement(),
            crate::mutation::ConfirmationRequirement::Strong
        );
        let tasks_before = rt.tasks.len();
        rt.action(Action::MutationConfirm)
            .expect("first press arms, does not error");
        assert_eq!(
            rt.tasks.len(),
            tasks_before,
            "arming a strong confirmation must send zero requests"
        );
        assert!(
            rt.active_document_mut()
                .and_then(|d| d.workflow.as_ref())
                .expect("workflow")
                .armed,
            "first press must arm, not commit"
        );
        rt.action(Action::MutationConfirm)
            .expect("second press commits");
        assert_eq!(
            rt.tasks.len(),
            tasks_before + 1,
            "the second press is the one that actually sends a request"
        );
        rt.shutdown().await;
    }

    #[tokio::test]
    async fn mutation_confirm_rejects_stale_epoch_and_sends_zero_requests() {
        let mut rt = runtime();
        rt.state.settings.readonly = false;
        rt.options.mutation_test_cluster_verified = true;
        open_deployment_scale_preview(&mut rt, "uid-1");
        rt.state.epoch += 1;
        let tasks_before = rt.tasks.len();
        assert!(
            rt.action(Action::MutationConfirm).is_err(),
            "a stale epoch must be rejected, never silently retargeted"
        );
        assert_eq!(rt.tasks.len(), tasks_before);
        rt.shutdown().await;
    }

    #[tokio::test]
    async fn command_scale_rejects_unsupported_kind_before_building_an_intent() {
        let mut rt = runtime();
        let object = crate::resources::Object::new(serde_json::json!({
            "apiVersion":"v1","kind":"ConfigMap",
            "metadata":{"namespace":"test","name":"c","uid":"uid-1"}
        }));
        rt.state.rows = vec![std::sync::Arc::new(object)];
        rt.state.selected = Some("uid-1".into());
        assert!(rt.command(":scale 3").is_err());
        assert!(!matches!(rt.state.mode, Mode::Document(_)));
        rt.shutdown().await;
    }

    #[tokio::test]
    async fn mutation_cordon_under_readonly_denies_and_sends_zero_requests() {
        let mut rt = runtime();
        rt.state.settings.readonly = true;
        rt.options.mutation_test_cluster_verified = true;
        let mut resource = rt.state.resource.clone().expect("resource");
        resource.api.kind = "Node".into();
        resource.namespaced = false;
        rt.state.resource = Some(resource);
        let object = crate::resources::Object::new(serde_json::json!({
            "apiVersion":"v1","kind":"Node",
            "metadata":{"name":"node-1","uid":"uid-1"}
        }));
        rt.state.rows = vec![std::sync::Arc::new(object)];
        rt.state.selected = Some("uid-1".into());
        rt.command(":cordon")
            .expect("cordon preview opens even when denied");
        let workflow = rt
            .active_document_mut()
            .and_then(|d| d.workflow.as_ref())
            .expect("workflow set");
        assert_eq!(
            workflow.evaluation.decision,
            crate::mutation::PolicyDecision::Deny
        );
        let tasks_before = rt.tasks.len();
        assert!(
            rt.action(Action::MutationConfirm).is_err(),
            "denied cordon confirm must error, never silently no-op"
        );
        assert_eq!(rt.tasks.len(), tasks_before);
        rt.shutdown().await;
    }

    fn flux_kustomization_object() -> crate::resources::Object {
        crate::resources::Object::new(serde_json::json!({
            "apiVersion":"kustomize.toolkit.fluxcd.io/v1","kind":"Kustomization",
            "metadata":{"namespace":"sauron-m9","name":"podinfo-kustomize","uid":"uid-1","generation":1},
            "spec":{"sourceRef":{"kind":"GitRepository","name":"podinfo"}},
            "status":{"observedGeneration":1}
        }))
    }
    fn open_flux_kustomization(rt: &mut Runtime) {
        let mut resource = rt.state.resource.clone().expect("resource");
        resource.api.group = "kustomize.toolkit.fluxcd.io".into();
        resource.api.api_version = "kustomize.toolkit.fluxcd.io/v1".into();
        resource.api.kind = "Kustomization".into();
        resource.api.plural = "kustomizations".into();
        rt.state.resource = Some(resource);
        rt.state.rows = vec![std::sync::Arc::new(flux_kustomization_object())];
        rt.state.selected = Some("uid-1".into());
    }

    #[tokio::test]
    async fn mutation_flux_suspend_and_resume_under_readonly_deny_and_send_zero_requests() {
        let mut rt = runtime();
        rt.state.settings.readonly = true;
        rt.options.mutation_test_cluster_verified = true;
        open_flux_kustomization(&mut rt);
        rt.command(":flux_suspend")
            .expect("flux_suspend preview opens even when denied");
        let workflow = rt
            .active_document_mut()
            .and_then(|d| d.workflow.as_ref())
            .expect("workflow set");
        assert_eq!(
            workflow.evaluation.decision,
            crate::mutation::PolicyDecision::Deny
        );
        let tasks_before = rt.tasks.len();
        assert!(
            rt.action(Action::MutationConfirm).is_err(),
            "denied flux_suspend confirm must error, never silently no-op"
        );
        assert_eq!(rt.tasks.len(), tasks_before);

        open_flux_kustomization(&mut rt);
        rt.command(":flux_resume")
            .expect("flux_resume preview opens even when denied");
        assert!(rt.action(Action::MutationConfirm).is_err());
        assert_eq!(rt.tasks.len(), tasks_before);
        rt.shutdown().await;
    }

    #[tokio::test]
    async fn mutation_flux_reconcile_requires_only_standard_confirmation_and_commits_on_first_press()
     {
        let mut rt = runtime();
        rt.state.settings.readonly = false;
        rt.options.mutation_test_cluster_verified = true;
        open_flux_kustomization(&mut rt);
        rt.command(":flux_reconcile")
            .expect("flux_reconcile preview opens");
        let workflow = rt
            .active_document_mut()
            .and_then(|d| d.workflow.as_ref())
            .expect("workflow set");
        assert_eq!(
            workflow.requirement(),
            crate::mutation::ConfirmationRequirement::Standard,
            "a Routine-risk Modify on a non-cluster-critical kind must not require the strong double-press"
        );
        let tasks_before = rt.tasks.len();
        rt.action(Action::MutationConfirm)
            .expect("first press commits, no arm step for Standard confirmation");
        assert_eq!(
            rt.tasks.len(),
            tasks_before + 1,
            "a single press must be enough to send the request"
        );
        rt.shutdown().await;
    }

    #[tokio::test]
    async fn mutation_trigger_under_readonly_denies_and_sends_zero_requests() {
        let mut rt = runtime();
        rt.state.settings.readonly = true;
        rt.options.mutation_test_cluster_verified = true;
        let mut cronjob_resource = rt.state.resource.clone().expect("resource");
        cronjob_resource.api.group = "batch".into();
        cronjob_resource.api.api_version = "batch/v1".into();
        cronjob_resource.api.kind = "CronJob".into();
        cronjob_resource.api.plural = "cronjobs".into();
        rt.state.resource = Some(cronjob_resource.clone());
        let mut job_resource = cronjob_resource.clone();
        job_resource.api.kind = "Job".into();
        job_resource.api.plural = "jobs".into();
        rt.connection.as_mut().expect("connected").catalog.resources =
            vec![cronjob_resource, job_resource];
        let object = crate::resources::Object::new(serde_json::json!({
            "apiVersion":"batch/v1","kind":"CronJob",
            "metadata":{"namespace":"sauron-m8b","name":"nightly","uid":"uid-1"},
            "spec":{"jobTemplate":{"spec":{"template":{"spec":{"containers":[{"name":"worker","image":"busybox:1.37"}]}}}}}
        }));
        rt.state.rows = vec![std::sync::Arc::new(object)];
        rt.state.selected = Some("uid-1".into());
        rt.command(":trigger")
            .expect("trigger preview opens even when denied");
        let workflow = rt
            .active_document_mut()
            .and_then(|d| d.workflow.as_ref())
            .expect("workflow set");
        assert_eq!(
            workflow.evaluation.decision,
            crate::mutation::PolicyDecision::Deny
        );
        let tasks_before = rt.tasks.len();
        assert!(
            rt.action(Action::MutationConfirm).is_err(),
            "denied trigger confirm must error, never silently no-op"
        );
        assert_eq!(rt.tasks.len(), tasks_before);
        rt.shutdown().await;
    }

    #[tokio::test]
    async fn mutation_evict_under_readonly_denies_and_sends_zero_requests() {
        let mut rt = runtime();
        rt.state.settings.readonly = true;
        rt.options.mutation_test_cluster_verified = true;
        let mut resource = rt.state.resource.clone().expect("resource");
        resource.api.kind = "Pod".into();
        rt.state.resource = Some(resource);
        let object = crate::resources::Object::new(serde_json::json!({
            "apiVersion":"v1","kind":"Pod",
            "metadata":{"namespace":"test","name":"m8b-pod","uid":"uid-1"}
        }));
        rt.state.rows = vec![std::sync::Arc::new(object)];
        rt.state.selected = Some("uid-1".into());
        rt.command(":evict")
            .expect("evict preview opens even when denied");
        let workflow = rt
            .active_document_mut()
            .and_then(|d| d.workflow.as_ref())
            .expect("workflow set");
        assert_eq!(
            workflow.evaluation.decision,
            crate::mutation::PolicyDecision::Deny
        );
        let tasks_before = rt.tasks.len();
        assert!(
            rt.action(Action::MutationConfirm).is_err(),
            "denied evict confirm must error, never silently no-op"
        );
        assert_eq!(rt.tasks.len(), tasks_before);
        rt.shutdown().await;
    }

    #[tokio::test]
    async fn mutation_force_delete_under_readonly_denies_and_sends_zero_requests() {
        let mut rt = runtime();
        rt.state.settings.readonly = true;
        rt.options.mutation_test_cluster_verified = true;
        let mut resource = rt.state.resource.clone().expect("resource");
        resource.api.kind = "Pod".into();
        rt.state.resource = Some(resource);
        let object = crate::resources::Object::new(serde_json::json!({
            "apiVersion":"v1","kind":"Pod",
            "metadata":{"namespace":"test","name":"stuck-pod","uid":"uid-1"}
        }));
        rt.state.rows = vec![std::sync::Arc::new(object)];
        rt.state.selected = Some("uid-1".into());
        rt.command(":force_delete")
            .expect("force_delete preview opens even when denied");
        let workflow = rt
            .active_document_mut()
            .and_then(|d| d.workflow.as_ref())
            .expect("workflow set");
        assert_eq!(
            workflow.evaluation.decision,
            crate::mutation::PolicyDecision::Deny
        );
        let tasks_before = rt.tasks.len();
        assert!(
            rt.action(Action::MutationConfirm).is_err(),
            "denied force_delete confirm must error, never silently no-op"
        );
        assert_eq!(rt.tasks.len(), tasks_before);
        rt.shutdown().await;
    }

    fn bulk_pod_rows(uids: &[&str]) -> Vec<std::sync::Arc<crate::resources::Object>> {
        uids.iter()
            .map(|uid| {
                std::sync::Arc::new(crate::resources::Object::new(serde_json::json!({
                    "apiVersion":"v1","kind":"Pod",
                    "metadata":{"namespace":"test","name":format!("pod-{uid}"),"uid":uid}
                })))
            })
            .collect()
    }
    fn select_all(rt: &mut Runtime) {
        rt.action(Action::SelectVisible).expect("select visible");
    }

    #[tokio::test]
    async fn bulk_workflow_empty_selection_is_refused_up_front() {
        let mut rt = runtime();
        rt.state.rows = bulk_pod_rows(&["a", "b"]);
        assert!(
            rt.command(":bulk_delete").is_err(),
            "an empty selection must refuse up front, never a silent no-op"
        );
        rt.shutdown().await;
    }

    #[tokio::test]
    async fn bulk_delete_under_readonly_denies_every_target_and_sends_zero_requests() {
        let mut rt = runtime();
        rt.state.settings.readonly = true;
        rt.options.mutation_test_cluster_verified = true;
        let mut resource = rt.state.resource.clone().expect("resource");
        resource.api.kind = "Pod".into();
        rt.state.resource = Some(resource);
        rt.state.rows = bulk_pod_rows(&["a", "b", "c"]);
        select_all(&mut rt);
        rt.command(":bulk_delete")
            .expect("bulk preview opens even when denied");
        let bulk = rt
            .active_document_mut()
            .and_then(|d| d.bulk.as_ref())
            .expect("bulk workflow set");
        assert_eq!(bulk.items.len(), 3);
        assert_eq!(
            bulk.eligible_count(),
            0,
            "readonly denies every individual target, not just an aggregate bit"
        );
        for item in &bulk.items {
            let workflow = item.workflow.as_ref().expect("delete is supported for Pod");
            assert_eq!(
                workflow.evaluation.decision,
                crate::mutation::PolicyDecision::Deny,
                "target {} must be individually denied",
                item.uid
            );
        }
        let tasks_before = rt.tasks.len();
        assert!(
            rt.action(Action::MutationConfirm).is_err(),
            "no eligible targets: confirm must error, never silently no-op"
        );
        assert_eq!(rt.tasks.len(), tasks_before);
        rt.shutdown().await;
    }

    #[tokio::test]
    async fn bulk_delete_strong_confirmation_requires_a_second_press_before_any_request() {
        let mut rt = runtime();
        rt.state.settings.readonly = false;
        rt.options.mutation_test_cluster_verified = true;
        let mut resource = rt.state.resource.clone().expect("resource");
        resource.api.kind = "Pod".into();
        rt.state.resource = Some(resource);
        rt.state.rows = bulk_pod_rows(&["a", "b"]);
        select_all(&mut rt);
        rt.command(":bulk_delete").expect("bulk preview opens");
        let requirement = rt
            .active_document_mut()
            .and_then(|d| d.bulk.as_ref())
            .map(|b| b.requirement())
            .expect("bulk workflow set");
        assert_eq!(
            requirement,
            crate::mutation::ConfirmationRequirement::Strong,
            "delete is Destructive -- the bulk gesture must require the same strong \
             confirmation every individual delete already requires, never lowered"
        );
        let tasks_before = rt.tasks.len();
        rt.action(Action::MutationConfirm)
            .expect("first press arms, does not error");
        assert_eq!(
            rt.tasks.len(),
            tasks_before,
            "arming must send zero requests -- only the second press commits"
        );
        assert!(
            rt.active_document_mut()
                .and_then(|d| d.bulk.as_ref())
                .expect("bulk workflow set")
                .armed
        );
        rt.shutdown().await;
    }

    #[tokio::test]
    async fn bulk_confirm_rejects_a_context_or_namespace_change_after_preview() {
        let mut rt = runtime();
        rt.state.settings.readonly = false;
        rt.options.mutation_test_cluster_verified = true;
        rt.state.rows = bulk_pod_rows(&["a", "b"]);
        select_all(&mut rt);
        rt.command(":bulk_label team=infra")
            .expect("bulk preview opens");
        // Simulates a context/namespace/resource switch after the preview
        // was built but before confirm -- cancel_scope() is the exact hook
        // every such switch already funnels through (see M10.1).
        rt.cancel_scope();
        assert!(
            rt.action(Action::MutationConfirm).is_err(),
            "a context/namespace switch after preview must invalidate the whole bulk \
             operation, never silently re-scope it"
        );
        rt.shutdown().await;
    }

    #[tokio::test]
    async fn bulk_set_image_partial_unsupported_within_one_homogeneous_kind_selection() {
        // SAUR-ON's table view is always one resource kind at a time (a
        // Selection can never literally span e.g. Pod+Deployment -- see
        // M10_ACCEPTANCE.md's own note on this), so the realistic version
        // of "mixed eligibility within one bulk operation" is: the SAME
        // builder produces different per-target results because the
        // targets' own live content differs, not their kind. set_image's
        // "no containers found" build-time rejection is the clearest case
        // of this already present in the existing single-target builder.
        let mut rt = runtime();
        rt.state.settings.readonly = false;
        rt.options.mutation_test_cluster_verified = true;
        let mut resource = rt.state.resource.clone().expect("resource");
        resource.api.kind = "Deployment".into();
        rt.state.resource = Some(resource);
        let with_container = crate::resources::Object::new(serde_json::json!({
            "apiVersion":"apps/v1","kind":"Deployment",
            "metadata":{"namespace":"test","name":"has-containers","uid":"a"},
            "spec":{"template":{"spec":{"containers":[{"name":"c","image":"old:1"}]}}}
        }));
        let without_containers = crate::resources::Object::new(serde_json::json!({
            "apiVersion":"apps/v1","kind":"Deployment",
            "metadata":{"namespace":"test","name":"no-containers","uid":"b"}
        }));
        rt.state.rows = vec![
            std::sync::Arc::new(with_container),
            std::sync::Arc::new(without_containers),
        ];
        select_all(&mut rt);
        rt.command(":bulk_set_image new:2")
            .expect("bulk preview opens");
        let bulk = rt
            .active_document_mut()
            .and_then(|d| d.bulk.as_ref())
            .expect("bulk workflow set");
        assert_eq!(bulk.items.len(), 2);
        assert_eq!(
            bulk.eligible_count(),
            1,
            "exactly one target is eligible; the other is individually Unsupported"
        );
        let a = bulk.items.iter().find(|i| i.uid == "a").unwrap();
        assert!(a.workflow.is_ok(), "the target with containers is eligible");
        let b = bulk.items.iter().find(|i| i.uid == "b").unwrap();
        assert!(
            b.workflow.is_err(),
            "the target with no containers is individually Unsupported, not silently dropped"
        );
        rt.shutdown().await;
    }

    #[tokio::test]
    async fn bulk_preview_reports_exact_selected_eligible_excluded_counts() {
        let mut rt = runtime();
        rt.state.settings.readonly = true; // denies every target -> all excluded
        rt.options.mutation_test_cluster_verified = true;
        let mut resource = rt.state.resource.clone().expect("resource");
        resource.api.kind = "Pod".into();
        rt.state.resource = Some(resource);
        rt.state.rows = bulk_pod_rows(&["a", "b", "c"]);
        select_all(&mut rt);
        rt.command(":bulk_delete").expect("bulk preview opens");
        let doc = rt.active_document_mut().expect("doc open");
        let text = doc.lines.iter().cloned().collect::<Vec<_>>().join("\n");
        assert!(text.contains("SELECTED: 3 target(s)"));
        assert!(text.contains("ELIGIBLE: 0"));
        assert!(text.contains("EXCLUDED: 3"));
        rt.shutdown().await;
    }

    #[tokio::test]
    async fn bulk_dry_run_is_not_implemented_and_errors_explicitly() {
        let mut rt = runtime();
        rt.state.rows = bulk_pod_rows(&["a", "b"]);
        select_all(&mut rt);
        rt.command(":bulk_delete").expect("bulk preview opens");
        assert!(
            rt.action(Action::MutationDryRun).is_err(),
            "dry-run is not implemented for bulk -- must error explicitly, never silently no-op"
        );
        rt.shutdown().await;
    }

    fn open_node_drain_preview(rt: &mut Runtime) {
        let mut resource = rt.state.resource.clone().expect("resource");
        resource.api.kind = "Node".into();
        resource.namespaced = false;
        rt.state.resource = Some(resource);
        let object = crate::resources::Object::new(serde_json::json!({
            "apiVersion":"v1","kind":"Node",
            "metadata":{"name":"node-1","uid":"uid-1"}
        }));
        rt.state.rows = vec![std::sync::Arc::new(object)];
        rt.state.selected = Some("uid-1".into());
        rt.command(":drain")
            .expect("drain preview opens even when denied");
    }

    #[tokio::test]
    async fn command_drain_rejects_unsupported_kind_before_building_an_intent() {
        let mut rt = runtime();
        let object = crate::resources::Object::new(serde_json::json!({
            "apiVersion":"v1","kind":"ConfigMap",
            "metadata":{"namespace":"test","name":"c","uid":"uid-1"}
        }));
        rt.state.rows = vec![std::sync::Arc::new(object)];
        rt.state.selected = Some("uid-1".into());
        assert!(rt.command(":drain").is_err());
        assert!(!matches!(rt.state.mode, Mode::Document(_)));
        rt.shutdown().await;
    }

    #[tokio::test]
    async fn drain_preview_shows_target_cordon_step_and_pod_plan_semantics_truthfully() {
        let mut rt = runtime();
        rt.state.settings.readonly = false;
        rt.options.mutation_test_cluster_verified = true;
        open_node_drain_preview(&mut rt);
        let text = rt
            .active_document_mut()
            .expect("doc open")
            .lines
            .iter()
            .cloned()
            .collect::<Vec<_>>()
            .join("\n");
        assert!(text.contains("TARGET NODE"));
        assert!(text.contains("node-1"));
        assert!(text.contains("CORDON STEP"));
        assert!(text.contains("PDB-AWARE EVICTION"));
        assert!(text.contains("NO ROLLBACK"));
        assert!(text.contains("CANCELLATION"));
        assert_eq!(rt.mode_name(), "drain");
        rt.shutdown().await;
    }

    #[tokio::test]
    async fn drain_preview_tasks_cancellation_token_is_fresh_not_already_cancelled() {
        // Regression: found live against the real cluster. `mutation_scope()`
        // (called to build the cordon scope) only cancels `self.document`, it
        // never reassigns a fresh child token -- every other spawn site does
        // that refresh itself right before spawning. `start_drain_preview`
        // originally captured the stale, already-cancelled token, so its
        // task returned immediately without ever listing anything, leaving
        // the preview stuck on `Loading` forever with zero visible error.
        let mut rt = runtime();
        rt.state.settings.readonly = false;
        rt.options.mutation_test_cluster_verified = true;
        open_node_drain_preview(&mut rt);
        assert!(
            !rt.document.is_cancelled(),
            "the preview-list task's cancellation token must not already be \
             cancelled at spawn time"
        );
        rt.shutdown().await;
    }

    #[tokio::test]
    async fn mutation_drain_under_readonly_denies_and_sends_zero_requests() {
        let mut rt = runtime();
        rt.state.settings.readonly = true;
        rt.options.mutation_test_cluster_verified = true;
        open_node_drain_preview(&mut rt);
        let drain = rt
            .active_document_mut()
            .and_then(|d| d.drain.as_ref())
            .expect("drain set");
        assert_eq!(
            drain.evaluation.decision,
            crate::mutation::PolicyDecision::Deny
        );
        let tasks_before = rt.tasks.len();
        assert!(
            rt.action(Action::MutationConfirm).is_err(),
            "denied drain confirm must error, never silently no-op"
        );
        assert_eq!(
            rt.tasks.len(),
            tasks_before,
            "a denied drain confirm must send zero requests"
        );
        rt.shutdown().await;
    }

    #[tokio::test]
    async fn drain_confirm_refuses_while_the_pod_list_is_still_loading() {
        let mut rt = runtime();
        rt.state.settings.readonly = false;
        rt.options.mutation_test_cluster_verified = true;
        open_node_drain_preview(&mut rt);
        assert!(matches!(
            rt.active_document_mut()
                .and_then(|d| d.drain.as_ref())
                .expect("drain set")
                .preview,
            crate::mutation::drain::DrainPreview::Loading
        ));
        let tasks_before = rt.tasks.len();
        assert!(
            rt.action(Action::MutationConfirm).is_err(),
            "confirming before the plan loads must be refused"
        );
        assert_eq!(
            rt.tasks.len(),
            tasks_before,
            "a refused confirm while loading must send zero further requests"
        );
        rt.shutdown().await;
    }

    #[tokio::test]
    async fn drain_strong_confirmation_first_press_only_arms_second_press_starts_the_orchestrator()
    {
        let mut rt = runtime();
        rt.state.settings.readonly = false;
        rt.options.mutation_test_cluster_verified = true;
        open_node_drain_preview(&mut rt);
        // The real preview list is loaded asynchronously against a live
        // connection; set it directly so this test is deterministic and
        // does not depend on that unrelated read's real-world timing --
        // `kube::drain::drain` always re-lists fresh at commit time
        // regardless of what this preview snapshot says.
        {
            let doc = rt.active_document_mut().expect("doc");
            let drain = doc.drain.as_mut().expect("drain");
            drain.preview = crate::mutation::drain::DrainPreview::Ready(vec![]);
        }
        assert_eq!(
            rt.active_document_mut()
                .and_then(|d| d.drain.as_ref())
                .expect("drain")
                .requirement(),
            crate::mutation::ConfirmationRequirement::Strong
        );
        let tasks_before = rt.tasks.len();
        rt.action(Action::MutationConfirm)
            .expect("first press arms, does not error");
        assert_eq!(
            rt.tasks.len(),
            tasks_before,
            "arming a strong confirmation must send zero requests"
        );
        assert!(
            rt.active_document_mut()
                .and_then(|d| d.drain.as_ref())
                .expect("drain")
                .armed,
            "first press must arm, not start the orchestrator"
        );
        rt.action(Action::MutationConfirm)
            .expect("second press starts the drain orchestrator");
        assert_eq!(
            rt.tasks.len(),
            tasks_before + 1,
            "the second press is the one that spawns exactly one drain orchestrator task"
        );
        rt.shutdown().await;
    }

    #[tokio::test]
    async fn mutation_commit_verification_never_touches_the_store_or_fabricates_timeline() {
        let mut rt = runtime();
        open_deployment_scale_preview(&mut rt, "uid-1");
        let revision_before = rt.state.store.revision;
        let histories_before = rt.state.store.histories.len();
        rt.reduce(crate::app::event::Event {
            epoch: rt.state.epoch,
            payload: Payload::MutationCommit {
                request: rt.state.request,
                outcome: crate::mutation::MutationOutcome::Committed,
                verification: Some(crate::mutation::Verification::Verified),
            },
        });
        assert_eq!(
            rt.state.store.revision, revision_before,
            "verification is a fresh GET, never written into the watch-derived store"
        );
        assert_eq!(
            rt.state.store.histories.len(),
            histories_before,
            "verification must never fabricate a Timeline entry"
        );
        let workflow = rt
            .active_document_mut()
            .and_then(|d| d.workflow.as_ref())
            .expect("workflow");
        assert_eq!(
            workflow.verification,
            Some(crate::mutation::Verification::Verified)
        );
        rt.shutdown().await;
    }

    #[tokio::test]
    async fn mutation_commit_result_from_a_stale_request_is_ignored() {
        let mut rt = runtime();
        open_deployment_scale_preview(&mut rt, "uid-1");
        let stale_request = rt.state.request;
        rt.state.request += 1;
        rt.reduce(crate::app::event::Event {
            epoch: rt.state.epoch,
            payload: Payload::MutationCommit {
                request: stale_request,
                outcome: crate::mutation::MutationOutcome::Committed,
                verification: Some(crate::mutation::Verification::Verified),
            },
        });
        let workflow = rt
            .active_document_mut()
            .and_then(|d| d.workflow.as_ref())
            .expect("workflow");
        assert!(
            workflow.commit.is_none(),
            "a stale (superseded) request's result must never be applied to the current preview"
        );
        rt.shutdown().await;
    }

    #[tokio::test]
    async fn mutations_view_is_read_only_and_handles_a_missing_journal() {
        let mut rt = runtime();
        let tasks_before = rt.tasks.len();
        rt.action(Action::Mutations).expect("mutations view");
        assert_eq!(
            rt.tasks.len(),
            tasks_before,
            "mutations view issues no task"
        );
        let doc = rt.active_document_mut().expect("doc open");
        assert_eq!(doc.title, "Mutation journal");
        rt.shutdown().await;
    }
    #[tokio::test]
    async fn flux_view_shows_capability_report_when_flux_is_not_installed() {
        let mut rt = runtime();
        let tasks_before = rt.tasks.len();
        rt.command(":flux").expect("flux capability view");
        assert_eq!(
            rt.tasks.len(),
            tasks_before,
            "the capability report issues no network task -- it only reads the already-fetched catalog"
        );
        let doc = rt.active_document_mut().expect("doc open");
        assert_eq!(doc.title, "Flux");
        let text = doc.lines.iter().cloned().collect::<Vec<_>>().join("\n");
        assert!(text.contains("Flux CAPABILITIES"));
        assert!(text.contains("STATE: Unsupported"));
        assert!(text.contains("[absent]  Kustomization"));
        rt.shutdown().await;
    }
    #[tokio::test]
    async fn flux_view_shows_object_status_when_a_flux_kind_is_selected() {
        let mut rt = runtime();
        let mut resource = rt.state.resource.clone().expect("resource");
        resource.api.group = "kustomize.toolkit.fluxcd.io".into();
        resource.api.api_version = "kustomize.toolkit.fluxcd.io/v1".into();
        resource.api.kind = "Kustomization".into();
        resource.api.plural = "kustomizations".into();
        rt.state.resource = Some(resource);
        let object = crate::resources::Object::new(serde_json::json!({
            "apiVersion":"kustomize.toolkit.fluxcd.io/v1","kind":"Kustomization",
            "metadata":{"namespace":"sauron-m9","name":"podinfo-kustomize","uid":"uid-1","generation":1},
            "spec":{"sourceRef":{"kind":"GitRepository","name":"podinfo"}},
            "status":{"observedGeneration":1,"conditions":[{"type":"Ready","status":"True","reason":"ReconciliationSucceeded","message":"Applied revision: master@sha1:abc"}]}
        }));
        rt.state.rows = vec![std::sync::Arc::new(object)];
        rt.state.selected = Some("uid-1".into());
        let tasks_before = rt.tasks.len();
        rt.command(":flux").expect("flux status view");
        assert_eq!(rt.tasks.len(), tasks_before);
        let doc = rt.active_document_mut().expect("doc open");
        assert_eq!(doc.title, "Flux: podinfo-kustomize");
        let text = doc.lines.iter().cloned().collect::<Vec<_>>().join("\n");
        assert!(text.contains("FLUX STATUS: Kustomization/podinfo-kustomize"));
        assert!(text.contains("Ready = True (ReconciliationSucceeded)"));
        rt.shutdown().await;
    }
    #[tokio::test]
    async fn argocd_view_shows_capability_report_when_argocd_is_not_installed() {
        let mut rt = runtime();
        let tasks_before = rt.tasks.len();
        rt.command(":argocd").expect("argocd capability view");
        assert_eq!(rt.tasks.len(), tasks_before);
        let doc = rt.active_document_mut().expect("doc open");
        assert_eq!(doc.title, "Argo CD");
        let text = doc.lines.iter().cloned().collect::<Vec<_>>().join("\n");
        assert!(text.contains("Argo CD CAPABILITIES"));
        assert!(text.contains("STATE: Unsupported"));
        assert!(text.contains("[absent]  Application"));
        rt.shutdown().await;
    }
    #[tokio::test]
    async fn argocd_view_shows_object_status_when_an_application_is_selected() {
        let mut rt = runtime();
        let mut resource = rt.state.resource.clone().expect("resource");
        resource.api.group = "argoproj.io".into();
        resource.api.api_version = "argoproj.io/v1alpha1".into();
        resource.api.kind = "Application".into();
        resource.api.plural = "applications".into();
        rt.state.resource = Some(resource);
        let object = crate::resources::Object::new(serde_json::json!({
            "apiVersion":"argoproj.io/v1alpha1","kind":"Application",
            "metadata":{"namespace":"argocd","name":"guestbook","uid":"uid-1"},
            "spec":{"project":"default","source":{"repoURL":"https://example.com/repo.git","path":"guestbook","targetRevision":"master"},"destination":{"server":"https://kubernetes.default.svc","namespace":"sauron-m9"}},
            "status":{"sync":{"status":"Synced","revision":"abc123"},"health":{"status":"Healthy"}}
        }));
        rt.state.rows = vec![std::sync::Arc::new(object)];
        rt.state.selected = Some("uid-1".into());
        let tasks_before = rt.tasks.len();
        rt.command(":argocd").expect("argocd status view");
        assert_eq!(rt.tasks.len(), tasks_before);
        let doc = rt.active_document_mut().expect("doc open");
        assert_eq!(doc.title, "Argo CD: guestbook");
        let text = doc.lines.iter().cloned().collect::<Vec<_>>().join("\n");
        assert!(text.contains("ARGO CD STATUS: Application/guestbook"));
        assert!(text.contains("SYNC STATUS: Synced (revision: abc123)"));
        assert!(text.contains("HEALTH STATUS: Healthy"));
        rt.shutdown().await;
    }
    fn argocd_application_object(history_revision: Option<&str>) -> crate::resources::Object {
        let history = history_revision
            .map(|rev| serde_json::json!([{"revision": rev}]))
            .unwrap_or_else(|| serde_json::json!([]));
        crate::resources::Object::new(serde_json::json!({
            "apiVersion":"argoproj.io/v1alpha1","kind":"Application",
            "metadata":{"namespace":"argocd","name":"guestbook","uid":"uid-1"},
            "spec":{"project":"default","source":{"repoURL":"https://example.com/repo.git","path":"guestbook","targetRevision":"master"},"destination":{"server":"https://kubernetes.default.svc","namespace":"sauron-m9"}},
            "status":{"sync":{"status":"Synced"},"health":{"status":"Healthy"},"history":history}
        }))
    }
    fn open_argocd_application(rt: &mut Runtime, history_revision: Option<&str>) {
        let mut resource = rt.state.resource.clone().expect("resource");
        resource.api.group = "argoproj.io".into();
        resource.api.api_version = "argoproj.io/v1alpha1".into();
        resource.api.kind = "Application".into();
        resource.api.plural = "applications".into();
        rt.state.resource = Some(resource);
        rt.state.rows = vec![std::sync::Arc::new(argocd_application_object(
            history_revision,
        ))];
        rt.state.selected = Some("uid-1".into());
    }

    #[tokio::test]
    async fn mutation_argocd_sync_and_refresh_under_readonly_deny_and_send_zero_requests() {
        let mut rt = runtime();
        rt.state.settings.readonly = true;
        rt.options.mutation_test_cluster_verified = true;
        open_argocd_application(&mut rt, None);
        rt.command(":argocd_sync")
            .expect("argocd_sync preview opens even when denied");
        let workflow = rt
            .active_document_mut()
            .and_then(|d| d.workflow.as_ref())
            .expect("workflow set");
        assert_eq!(
            workflow.evaluation.decision,
            crate::mutation::PolicyDecision::Deny
        );
        let tasks_before = rt.tasks.len();
        assert!(
            rt.action(Action::MutationConfirm).is_err(),
            "denied argocd_sync confirm must error, never silently no-op"
        );
        assert_eq!(rt.tasks.len(), tasks_before);

        open_argocd_application(&mut rt, None);
        rt.command(":argocd_refresh")
            .expect("argocd_refresh preview opens even when denied");
        assert!(rt.action(Action::MutationConfirm).is_err());
        assert_eq!(rt.tasks.len(), tasks_before);
        rt.shutdown().await;
    }

    #[tokio::test]
    async fn argocd_rollback_rejects_a_revision_not_in_status_history() {
        let mut rt = runtime();
        rt.state.settings.readonly = false;
        rt.options.mutation_test_cluster_verified = true;
        open_argocd_application(&mut rt, Some("known-revision"));
        assert!(
            rt.command(":argocd_rollback unknown-revision").is_err(),
            "rollback must refuse a revision absent from status.history"
        );
        assert!(!matches!(rt.state.mode, Mode::Document(_)));
        rt.shutdown().await;
    }

    #[tokio::test]
    async fn argocd_rollback_accepts_a_revision_present_in_status_history() {
        let mut rt = runtime();
        rt.state.settings.readonly = false;
        rt.options.mutation_test_cluster_verified = true;
        open_argocd_application(&mut rt, Some("known-revision"));
        rt.command(":argocd_rollback known-revision")
            .expect("rollback preview opens for a known history revision");
        let workflow = rt
            .active_document_mut()
            .and_then(|d| d.workflow.as_ref())
            .expect("workflow set");
        assert_eq!(workflow.intent.source_action, "argocd_rollback");
        assert!(workflow.change.contains("known-revision"));
        rt.shutdown().await;
    }

    #[tokio::test]
    async fn helm_view_requires_selecting_a_secret_first() {
        let mut rt = runtime();
        assert!(
            rt.command(":helm").is_err(),
            "with nothing selected, :helm must error, never silently no-op"
        );
        let non_secret = crate::resources::Object::new(serde_json::json!({
            "apiVersion":"v1","kind":"ConfigMap",
            "metadata":{"namespace":"sauron-m9","name":"unrelated","uid":"uid-1"},
        }));
        rt.state.rows = vec![std::sync::Arc::new(non_secret)];
        rt.state.selected = Some("uid-1".into());
        assert!(
            rt.command(":helm").is_err(),
            "a non-Secret object must be rejected without any fetch attempt"
        );
        rt.shutdown().await;
    }

    #[tokio::test]
    async fn helm_view_does_not_trust_the_cached_type_field_the_fresh_fetch_re_verifies_it() {
        // Per the M9.5 security design, the cached (already-redacted)
        // object's own `type` field is presentational only -- it may
        // support the UI's decision to *attempt* a fetch, but it must
        // never be treated as authorization. Whether this Secret is
        // actually a Helm release is decided solely by
        // `kube::helm::read_release`'s own fresh, re-verified GET. So
        // even a plain `Opaque` Secret is allowed to proceed to the
        // bounded fetch here -- it will fail closed downstream (proven
        // by `helm_read_release_rejects_a_secret_that_is_not_a_helm_
        // release_fresh_check_not_cached` in tests/watch_transport.rs),
        // not be silently rejected by trusting a stale local guess.
        let mut rt = runtime();
        let object = crate::resources::Object::new(serde_json::json!({
            "apiVersion":"v1","kind":"Secret","type":"Opaque",
            "metadata":{"namespace":"sauron-m9","name":"unrelated","uid":"uid-1"},
            "data":{"foo":"<redacted>"}
        }));
        rt.state.rows = vec![std::sync::Arc::new(object)];
        rt.state.selected = Some("uid-1".into());
        let tasks_before = rt.tasks.len();
        rt.command(":helm")
            .expect("any Secret may be attempted -- real enforcement is in the fresh fetch");
        assert_eq!(rt.tasks.len(), tasks_before + 1);
        rt.shutdown().await;
    }

    #[tokio::test]
    async fn helm_view_spawns_a_bounded_fetch_never_decodes_the_cached_redacted_object() {
        // `Object::new` has already redacted this Secret's `data` to
        // `<redacted>` by the time it reaches `state.rows` -- exactly
        // like every other Secret in this app (see
        // `kube::helm::read_release`'s own doc comment). `:helm` must
        // therefore never try to decode it locally; it must spawn the
        // dedicated fetch-and-reverify task instead. The real decode/
        // sanitize round trip against a live-shaped Secret is covered by
        // `helm_read_release_decodes_a_real_secret_and_sanitizes_the_view`
        // in tests/watch_transport.rs, against a real HTTP endpoint.
        let mut rt = runtime();
        let object = crate::resources::Object::new(serde_json::json!({
            "apiVersion":"v1","kind":"Secret","type":"helm.sh/release.v1",
            "metadata":{"namespace":"sauron-m9","name":"sh.helm.release.v1.demo.v2","uid":"uid-1"},
            "data":{"release": "irrelevant -- already redacted before this test ever sees it"}
        }));
        rt.state.rows = vec![std::sync::Arc::new(object)];
        rt.state.selected = Some("uid-1".into());
        let tasks_before = rt.tasks.len();
        rt.command(":helm")
            .expect("helm view opens a placeholder and spawns the bounded fetch");
        assert_eq!(
            rt.tasks.len(),
            tasks_before + 1,
            "reading a Helm release body always requires a fresh, bounded, re-verified fetch"
        );
        let doc = rt.active_document_mut().expect("doc open");
        assert_eq!(doc.title, "Helm: sh.helm.release.v1.demo.v2");
        rt.shutdown().await;
    }

    #[tokio::test]
    async fn adjacent_follow_navigates_by_canonical_identity_and_history_returns() {
        let mut rt = runtime();
        let configmap = Resource {
            api: ::kube::core::ApiResource {
                group: String::new(),
                version: "v1".into(),
                api_version: "v1".into(),
                kind: "ConfigMap".into(),
                plural: "configmaps".into(),
            },
            namespaced: true,
            short_names: vec![],
            verbs: vec!["watch".into()],
        };
        rt.connection
            .as_mut()
            .expect("connected")
            .catalog
            .resources
            .push(configmap.clone());
        let root = crate::resources::Object::new(serde_json::json!({
            "apiVersion":"v1","kind":"Pod",
            "metadata":{"namespace":"test","name":"p","uid":"root-uid"}
        }));
        rt.state.selected = Some("root-uid".into());
        let mut doc = Document::new(
            "Adjacent: p".into(),
            "ADJACENT: v1/pods test/p\n\nREFERENCES\n  v1/configmaps n/cm [Healthy] via path\n"
                .into(),
        );
        doc.source = Some(document::Source {
            resource: rt.state.resource.clone().expect("resource"),
            selected: std::sync::Arc::new(root),
            action: Action::Adjacent,
            warning_only: false,
        });
        doc.adjacent = vec![crate::adjacent::Target {
            line: 3,
            resource: configmap,
            namespace: "n".into(),
            name: "cm".into(),
            uid: "cm-uid".into(),
        }];
        rt.state.mode = Mode::Document(doc);
        rt.action(Action::Follow).expect("follow");
        assert_eq!(rt.history.len(), 1, "the pods view is pushed onto history");
        assert!(matches!(rt.state.mode, Mode::Table));
        assert_eq!(rt.state.query.resource, "v1/configmaps");
        assert_eq!(rt.state.query.namespace.as_deref(), Some("n"));
        assert_eq!(
            rt.state.selected.as_deref(),
            Some("cm-uid"),
            "selection is the exact UID, never the name alone"
        );
        rt.action(Action::HistoryBack).expect("back");
        assert_eq!(rt.state.query.resource, "v1/pods");
        assert_eq!(rt.state.query.namespace.as_deref(), Some("test"));
        rt.shutdown().await;
    }
    #[tokio::test]
    async fn follow_without_an_adjacent_target_errors_instead_of_navigating() {
        let mut rt = runtime();
        rt.state.mode = Mode::Document(Document::new("Yaml: p".into(), "kind: Pod".into()));
        assert!(rt.action(Action::Follow).is_err());
        assert!(rt.history.is_empty());
        rt.shutdown().await;
    }
    #[tokio::test]
    async fn adjacent_up_down_move_the_target_cursor_not_raw_scroll() {
        let mut rt = runtime();
        let secret = Resource {
            api: ::kube::core::ApiResource {
                group: String::new(),
                version: "v1".into(),
                api_version: "v1".into(),
                kind: "Secret".into(),
                plural: "secrets".into(),
            },
            namespaced: true,
            short_names: vec![],
            verbs: vec!["watch".into()],
        };
        rt.connection
            .as_mut()
            .expect("connected")
            .catalog
            .resources
            .push(secret.clone());
        let root = crate::resources::Object::new(serde_json::json!({
            "apiVersion":"v1","kind":"Pod",
            "metadata":{"namespace":"test","name":"p","uid":"root-uid"}
        }));
        let mut doc = Document::new(
            "Adjacent: p".into(),
            "ADJACENT: v1/pods test/p\n\nREFERENCES\n  v1/configmaps n/cm [Healthy] via a\n  v1/secrets n/s [Healthy] via b\n"
                .into(),
        );
        doc.source = Some(document::Source {
            resource: rt.state.resource.clone().expect("resource"),
            selected: std::sync::Arc::new(root),
            action: Action::Adjacent,
            warning_only: false,
        });
        let configmap = Resource {
            api: ::kube::core::ApiResource {
                group: String::new(),
                version: "v1".into(),
                api_version: "v1".into(),
                kind: "ConfigMap".into(),
                plural: "configmaps".into(),
            },
            namespaced: true,
            short_names: vec![],
            verbs: vec!["watch".into()],
        };
        doc.adjacent = vec![
            crate::adjacent::Target {
                line: 3,
                resource: configmap,
                namespace: "n".into(),
                name: "cm".into(),
                uid: "cm-uid".into(),
            },
            crate::adjacent::Target {
                line: 4,
                resource: secret,
                namespace: "n".into(),
                name: "s".into(),
                uid: "s-uid".into(),
            },
        ];
        rt.state.mode = Mode::Document(doc);
        assert_eq!(rt.active_document_mut().unwrap().adjacent_selected, 0);
        rt.action(Action::Down)
            .expect("down moves the cursor, never a plain scroll");
        assert_eq!(rt.active_document_mut().unwrap().adjacent_selected, 1);
        rt.action(Action::Down)
            .expect("down clamps at the last target");
        assert_eq!(rt.active_document_mut().unwrap().adjacent_selected, 1);
        rt.action(Action::Follow).expect("follow");
        assert_eq!(rt.state.query.resource, "v1/secrets");
        assert_eq!(
            rt.state.selected.as_deref(),
            Some("s-uid"),
            "Follow must use the selected target, not always the first one"
        );
        rt.shutdown().await;
    }
    #[tokio::test]
    async fn exec_is_denied_before_any_connection_attempt_when_readonly() {
        let mut rt = runtime();
        assert!(rt.state.settings.readonly);
        let sessions_before = rt.sessions.active_count();
        let error = rt.command("exec -- true").expect_err("must deny");
        assert!(error.to_string().contains("read-only"));
        assert_eq!(
            rt.sessions.active_count(),
            sessions_before,
            "denial must happen before any session/connection attempt"
        );
        rt.state.settings.readonly = false;
        // With readonly lifted the gate passes; the very next check (no row
        // selected) is what actually fails now, proving gate ordering is correct.
        let error = rt
            .command("exec -- true")
            .expect_err("still needs a selection");
        assert!(!error.to_string().contains("read-only"));
    }
    #[tokio::test]
    async fn navigate_while_reconnecting_queues_instead_of_erroring() {
        // Reproduces a live combined-acceptance finding: typing a resource command
        // (e.g. `:pods`) in the brief window right after a context switch, before its
        // Payload::Connected has landed, used to fail with "Not connected yet". That
        // error reopened the palette with the failed text still in the edit buffer,
        // so any further keystrokes -- e.g. a next, entirely unrelated `:ns ...`
        // command -- were silently appended to that stale buffer as edits instead of
        // starting a fresh command, corrupting everything typed afterward.
        let mut rt = runtime();
        let connection = rt.connection.take().expect("connection");
        assert!(
            rt.command("pods").is_ok(),
            "must queue, not error, while disconnected"
        );
        assert!(rt.pending.is_some(), "query must be queued as pending");
        assert!(
            rt.history.is_empty(),
            "no history entry for a query that never actually navigated"
        );
        let epoch = rt.state.epoch;
        rt.reduce(Event {
            epoch,
            payload: Payload::Connected(Box::new(connection)),
        });
        assert!(
            rt.pending.is_none(),
            "the queued query must be applied once connected"
        );
        assert_eq!(rt.state.query.resource, "v1/pods");
        rt.shutdown().await;
    }
    #[tokio::test]
    async fn namespace_switch_while_reconnecting_is_applied_once_connected() {
        // Same race as above, via `:ns` instead of a resource query: switch_namespace()
        // used to propagate watch()'s "Not connected" error, which the palette turns
        // into a reopened, stuck edit buffer that swallows subsequent keystrokes.
        let mut rt = runtime();
        let connection = rt.connection.take().expect("connection");
        assert!(
            rt.switch_namespace(Some("kube-system".into())).is_ok(),
            "must not error while disconnected"
        );
        assert_eq!(rt.state.query.namespace.as_deref(), Some("kube-system"));
        let epoch = rt.state.epoch;
        rt.reduce(Event {
            epoch,
            payload: Payload::Connected(Box::new(connection)),
        });
        assert_eq!(
            rt.state.query.namespace.as_deref(),
            Some("kube-system"),
            "rewatch() on Connected must apply the namespace set while disconnected"
        );
        rt.shutdown().await;
    }
    #[test]
    fn namespace_for_context_defers_to_kubeconfig_default_on_first_visit() {
        let memory = std::collections::HashMap::new();
        assert_eq!(
            namespace_for_context(&memory, "prod"),
            Some(String::new()),
            "an unvisited context has no memory to restore, so it must defer to kubeconfig"
        );
    }
    #[test]
    fn namespace_for_context_restores_remembered_namespace_including_all_namespaces() {
        let mut memory = std::collections::HashMap::new();
        memory.insert("prod".to_string(), Some("billing".to_string()));
        memory.insert("staging".to_string(), None);
        assert_eq!(
            namespace_for_context(&memory, "prod"),
            Some("billing".into())
        );
        assert_eq!(
            namespace_for_context(&memory, "staging"),
            None,
            "None means all-namespaces and must be preserved, not confused with unvisited"
        );
    }
    fn entry(resource: &str) -> state::HistoryEntry {
        state::HistoryEntry {
            context: "kind-sauron-test".into(),
            namespace: Some("sauron-fixtures".into()),
            resource: Resource {
                api: ::kube::core::ApiResource {
                    group: String::new(),
                    version: "v1".into(),
                    api_version: "v1".into(),
                    kind: resource.into(),
                    plural: resource.into(),
                },
                namespaced: true,
                short_names: vec![],
                verbs: vec!["watch".into()],
            },
            labels: None,
            fields: None,
            filter_text: String::new(),
            sort: "NAME".into(),
            descending: false,
            selected: None,
        }
    }
    #[test]
    fn push_capped_drops_oldest_once_over_the_limit() {
        let mut stack = std::collections::VecDeque::new();
        for i in 0..5 {
            push_capped(&mut stack, entry(&format!("r{i}")), 3);
        }
        // A fixed cap from the start: never grows past it, and it's the OLDEST entries
        // that go, so "back" still walks the most recent history first.
        assert_eq!(stack.len(), 3);
        assert_eq!(
            stack
                .iter()
                .map(|e| e.resource.api.plural.as_str())
                .collect::<Vec<_>>(),
            vec!["r2", "r3", "r4"]
        );
    }
    #[test]
    fn push_capped_never_exceeds_a_cap_of_one() {
        let mut stack = std::collections::VecDeque::new();
        push_capped(&mut stack, entry("a"), 1);
        push_capped(&mut stack, entry("b"), 1);
        assert_eq!(stack.len(), 1);
        assert_eq!(stack[0].resource.api.plural, "b");
    }
}
