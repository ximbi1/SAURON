pub mod document;
pub mod event;
pub mod state;

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
    scope: CancellationToken,
    document: CancellationToken,
    pending: Option<ResourceCommand>,
    pending_history: Option<state::HistoryEntry>,
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
                scope: CancellationToken::new(),
                document: CancellationToken::new(),
                pending: None,
                pending_history: None,
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
        self.watch()
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
            Payload::LogLine { request, line } if request == self.state.request => {
                if let Some(doc) = self.active_document_mut() {
                    doc.append(line);
                }
            }
            Payload::LogEnd { request, message } if request == self.state.request => {
                self.state.status = message;
                if let Mode::Document(doc) = &mut self.state.mode {
                    doc.streaming = false;
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
        let connection = self.connection.as_ref().context("Not connected yet")?;
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
                let mode_name = if matches!(self.state.mode, Mode::Document(_)) {
                    "document"
                } else {
                    "table"
                };
                if let Some(binding) = crate::command::registry()
                    .into_iter()
                    .find(|b| b.action == action)
                {
                    anyhow::ensure!(
                        matches!(binding.mode, "global" | "navigation")
                            || binding.mode == mode_name,
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
        let connection = self.connection.clone().context("Not connected")?;
        self.document.cancel();
        self.document = self.scope.child_token();
        self.state.request += 1;
        let mut doc = Document::new(
            format!(
                "Logs: {} · timestamps · {}",
                object.name,
                if previous { "previous" } else { "follow" }
            ),
            String::new(),
        );
        doc.streaming = true;
        doc.follow = true;
        self.state.mode = Mode::Document(doc);
        self.state.status = "Streaming logs · Esc returns".into();
        self.tasks.spawn(crate::kube::evidence::logs(
            connection,
            object,
            crate::kube::evidence::LogOptions {
                container,
                previous,
            },
            self.state.epoch,
            self.state.request,
            self.tx.clone(),
            self.document.clone(),
        ));
        Ok(())
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
        crate::safety::text(&out)
    }
    pub async fn shutdown(&mut self) {
        self.scope.cancel();
        self.document.cancel();
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
    let result=async {
        while !runtime.state.quit {
            tokio::select! {
                biased;
                _=tokio::signal::ctrl_c()=>{runtime.state.quit=true;},
                event=input.next()=>match event {
                    Some(Ok(TerminalEvent::Key(key))) if key.kind!=KeyEventKind::Release=>{
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
                                let mode_name=if matches!(runtime.state.mode,Mode::Document(_)){"document"}else{"table"};
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
                },
                _=frames.tick()=>if runtime.state.dirty {
                    runtime.state.prepare();let suggestions=if let Mode::Command(text)=&runtime.state.mode{runtime.suggestions(text)}else{vec![]};
                    terminal.draw(|frame|crate::ui::render(frame,&mut runtime.state,&suggestions))?;
                    runtime.state.dirty=false;
                },
                _=clock.tick()=>runtime.state.dirty=true,
                Some(event)=rx.recv()=>{runtime.reduce(event);for _ in 0..63{match rx.try_recv(){Ok(event)=>runtime.reduce(event),Err(_)=>break}}},
                Some(result)=runtime.tasks.join_next(),if !runtime.tasks.is_empty()=>{if result.is_err(){runtime.state.error=Some("Background task failed; refresh to retry".into());runtime.state.dirty=true;}},
            }
        }
        Ok(())
    }.await;
    runtime.shutdown().await;
    result
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
