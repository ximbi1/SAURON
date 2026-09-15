pub mod event;
pub mod state;

use crate::{
    command::{Action, Command, Keymap, ResourceCommand},
    config::Config,
    kube::{ConnectOptions, Connection, watch::Query},
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
    namespace_by_context: std::collections::HashMap<String, Option<String>>,
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
                scope: CancellationToken::new(),
                document: CancellationToken::new(),
                pending: None,
                namespace_by_context: std::collections::HashMap::new(),
            },
            rx,
        ))
    }
    /// Switch to a different context, restoring that context's last-viewed namespace
    /// if this session has visited it before, instead of always resetting to the
    /// kubeconfig default. Remembers the outgoing context's current namespace first.
    fn switch_context(&mut self, context: String) {
        if let Some(current) = self.connection.as_ref().map(|c| c.context.clone()) {
            self.namespace_by_context
                .insert(current, self.state.query.namespace.clone());
        }
        self.state.query.namespace = namespace_for_context(&self.namespace_by_context, &context);
        self.connect(Some(context));
    }
    pub fn connect(&mut self, context: Option<String>) {
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
        self.scope.cancel();
        self.document.cancel();
        self.scope = CancellationToken::new();
        self.document = self.scope.child_token();
        self.state.epoch += 1;
        self.state.request += 1;
        self.state.selected = None;
        self.state.rows.clear();
        self.state.store = Store::new(
            self.state.settings.max_objects,
            self.state.settings.max_bytes,
        );
        self.state.mode = Mode::Table;
        self.state.synced = false;
        self.state.dirty = true;
    }
    pub fn watch(&mut self) -> Result<()> {
        let connection = self.connection.clone().context("Not connected")?;
        let resource = connection
            .catalog
            .resolve(&self.state.query.resource, &connection.settings.aliases)?;
        anyhow::ensure!(
            resource.verbs.iter().any(|v| v == "watch"),
            "Resource API does not advertise watch; snapshot polling is not implemented"
        );
        self.cancel_scope();
        self.state.resource = Some(resource.clone());
        self.state.status = format!("Listing {}…", resource.qualified());
        let epoch = self.state.epoch;
        let tx = self.tx.clone();
        let cancel = self.scope.clone();
        let query = self.state.query.clone();
        self.tasks.spawn(crate::kube::watch::run(
            connection, resource, query, epoch, tx, cancel,
        ));
        Ok(())
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
                if let Some(query) = self.pending.take() {
                    if let Err(e) = self.navigate(query) {
                        self.state.error = Some(e.to_string());
                    }
                } else if let Err(e) = self.watch() {
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
                self.state.mode = Mode::Document(Document::new(title, text));
                self.state.status = "Document snapshot · Esc returns".into();
            }
            Payload::DocumentError { request, error } if request == self.state.request => {
                self.state.error = Some(error);
                self.state.mode = Mode::Table;
            }
            Payload::LogLine { request, line } if request == self.state.request => {
                if let Mode::Document(doc) = &mut self.state.mode {
                    doc.append(line);
                }
            }
            Payload::LogEnd { request, message } if request == self.state.request => {
                self.state.status = message;
                if let Mode::Document(doc) = &mut self.state.mode {
                    doc.streaming = false;
                }
            }
            _ => {}
        }
    }
    fn navigate(&mut self, query: ResourceCommand) -> Result<()> {
        if let Some(context) = &query.context
            && self
                .connection
                .as_ref()
                .is_none_or(|c| &c.context != context)
        {
            self.state.query.namespace = Some("".into());
            self.pending = Some(query.clone());
            self.connect(Some(context.clone()));
            return Ok(());
        }
        let connection = self.connection.as_ref().context("Not connected yet")?;
        connection
            .catalog
            .resolve(&query.resource, &connection.settings.aliases)?;
        let filter = crate::filters::Expr::parse(query.filter.as_deref().unwrap_or(""))?;
        if query.all {
            self.state.query.namespace = None;
        } else if let Some(ns) = query.namespace {
            self.state.query.namespace = if ns == "all" || ns == "*" {
                None
            } else {
                Some(ns)
            };
        }
        self.state.query.resource = query.resource;
        self.state.query.labels = query.labels;
        self.state.query.fields = query.fields;
        self.state.filter_text = query.filter.unwrap_or_default();
        self.state.filter = filter;
        self.state.sort = "NAME".into();
        self.state.descending = false;
        self.watch()
    }
    pub fn command(&mut self, text: &str) -> Result<()> {
        self.state.error = None;
        match crate::command::parse(text)? {
            Command::Action(action) => self.action(action),
            Command::Resource(query) => self.navigate(query),
            Command::Namespace(ns) => {
                self.state.query.namespace = if ns == "all" || ns == "*" {
                    None
                } else {
                    Some(ns)
                };
                self.watch()
            }
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
                let (column, direction) = spec.split_once(':').unwrap_or((&spec, "asc"));
                anyhow::ensure!(
                    ["asc", "desc"].contains(&direction),
                    "Sort direction must be asc or desc"
                );
                anyhow::ensure!(
                    self.state
                        .columns()
                        .iter()
                        .any(|c| c.eq_ignore_ascii_case(column)),
                    "Unknown sort column"
                );
                self.state.sort = column.to_uppercase();
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
                } else if !self.state.filter_text.is_empty() {
                    self.state.filter_text.clear();
                    self.state.filter = crate::filters::Expr::All;
                } else {
                    self.state.quit = true;
                }
            }
            Palette => {
                self.document.cancel();
                self.state.request += 1;
                self.state.mode = Mode::Command(String::new());
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
                self.watch()?;
            }
            AllNamespaces => {
                self.state.query.namespace = None;
                self.watch()?;
            }
            Namespaces => {
                self.navigate(ResourceCommand {
                    resource: "namespaces".into(),
                    ..Default::default()
                })?;
            }
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
    fn open_document(&mut self, action: Action) -> Result<()> {
        let object = self.state.selected_object().context("Select a row first")?;
        let connection = self.connection.clone().context("Not connected")?;
        let resource = self
            .state
            .resource
            .clone()
            .context("No resource selected")?;
        self.document.cancel();
        self.document = self.scope.child_token();
        self.state.request += 1;
        let request = self.state.request;
        let epoch = self.state.epoch;
        let tx = self.tx.clone();
        let cancel = self.document.clone();
        self.state.mode = Mode::Loading;
        self.state.status = "Gathering fresh evidence · Esc cancels".into();
        self.tasks.spawn(async move {
            let result=tokio::select!{biased;_=cancel.cancelled()=>return,result=crate::kube::evidence::document(&connection,&resource,&object,action)=>result};
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
                                    KeyCode::Esc=>{},
                                    KeyCode::Enter=>{
                                        let result=if command{runtime.command(&text)}else{crate::filters::Expr::parse(&text).map(|expr|{runtime.state.filter=expr;runtime.state.filter_text=text.clone();})};
                                        if let Err(e)=result {runtime.state.error=Some(e.to_string());runtime.state.mode=if command{Mode::Command(text)}else{Mode::Filter(text)};}
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
                                    KeyCode::Enter=>{if let Some(item)=picker.items.get(picker.cursor).cloned(){runtime.switch_context(item);}},
                                    _=>{runtime.state.mode=Mode::Picker(picker);},
                                }
                            },
                            Mode::Search(mut doc,mut text)=>{
                                match key.code{KeyCode::Esc=>runtime.state.mode=Mode::Document(doc),KeyCode::Enter=>{doc.search=text;doc.search_next(false);runtime.state.mode=Mode::Document(doc);},_=>{edit(&mut text,key);runtime.state.mode=Mode::Search(doc,text);}}
                            },
                            mode=>{
                                runtime.state.mode=mode;
                                let mode_name=if matches!(runtime.state.mode,Mode::Document(_)){"document"}else{"table"};
                                if let Some(action)=runtime.state.keymap.action(key,mode_name)&& let Err(e)=runtime.action(action){runtime.state.error=Some(e.to_string());}
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
        let mut names: Vec<String> = crate::command::COMMANDS
            .iter()
            .map(|s| s.to_string())
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
}
