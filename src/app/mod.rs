pub mod document;
pub mod event;
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
        self.palette_document = None;
        self.scope.cancel();
        self.document.cancel();
        self.scope = CancellationToken::new();
        self.document = self.scope.child_token();
        self.state.epoch += 1;
        self.state.request += 1;
        self.state.selected = None;
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
                self.state.watch_errors += 1;
                self.state.synced = false;
                self.state.error = Some(error);
                self.state.status = "STALE · recovering watch".into();
            }
            Payload::Document {
                request,
                title,
                text,
            } if request == self.state.request => {
                if let Some(doc) = self.active_document_mut() {
                    doc.title = title;
                    doc.replace(text);
                    doc.freshness = document::Freshness::Snapshot(chrono::Utc::now());
                }
            }
            Payload::DocumentError { request, error } if request == self.state.request => {
                if let Some(doc) = self.active_document_mut() {
                    doc.replace(String::new());
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
                let settings = if let Some(c) = &self.connection {
                    config.resolve(&c.cluster, &c.context)?
                } else {
                    config.resolve("", "")?
                };
                let keymap = Keymap::compile(&settings.keys)?;
                self.config = config;
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
            Yaml | Describe | Explain | Events => self.open_document(action)?,
            Timeline => {
                let object = self.state.selected_object().context("Select a row first")?;
                let text=self.state.store.histories.get(&object.uid).map(|h|h.iter().map(|c|format!("{} {}",c.at.to_rfc3339(),c.summary)).collect::<Vec<_>>().join("\n")).unwrap_or_else(||"No meaningful watch changes observed for this UID during this view. History is session-local and bounded.".into());
                self.open_static("Timeline", text);
            }
            Logs => self.open_logs(None, false)?,
            PreviousLogs => self.open_logs(None, true)?,
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
        }
        self.state.dirty = true;
        Ok(())
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
            Mode::Document(doc) if doc.session.is_some() => "logs",
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
        self.tasks.spawn(async move {
            let document::Source { resource, selected:object, action, warning_only } = source;
            let result=tokio::select!{biased;_=cancel.cancelled()=>return,result=crate::kube::evidence::document(&connection,&resource,&object,action,warning_only)=>result};
            let payload=match result{Ok(text)=>Payload::Document{request,title:format!("{action:?}: {}",object.name),text},Err(e)=>Payload::DocumentError{request,error:e.to_string()}};
            tokio::select!{_=cancel.cancelled()=>{},_=tx.send(Event{epoch,payload})=>{}}
        });
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
            "{} {}\nRead-only: enforced (mutations unavailable)\nContext: {}\nResource: {}\nRows: {}\nCache estimated JSON bytes: {}\nWatch errors: {}\nActive tasks: {}\nQueue capacity: {} / 256\n",
            crate::brand::NAME,
            crate::brand::VERSION,
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
        crate::safety::text(&out)
    }
    pub async fn shutdown(&mut self) {
        self.scope.cancel();
        self.document.cancel();
        self.sessions.shutdown().await;
        self.tasks.abort_all();
        while self.tasks.join_next().await.is_some() {}
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
                _=clock.tick()=>runtime.state.dirty=true,
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
