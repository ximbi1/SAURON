use anyhow::{Result, bail, ensure};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Quit,
    Down,
    Up,
    First,
    Last,
    PageDown,
    PageUp,
    Back,
    Palette,
    Filter,
    Help,
    Yaml,
    Describe,
    Explain,
    Events,
    Timeline,
    Logs,
    PreviousLogs,
    LogsVisible,
    PauseLogs,
    ClearLogs,
    FilterLogs,
    Refresh,
    Sort,
    Reverse,
    AllNamespaces,
    Namespaces,
    Wide,
    Wrap,
    Fullscreen,
    SearchNext,
    SearchPrevious,
    ScrollLeft,
    ScrollRight,
    HistoryBack,
    HistoryForward,
    ToggleWarnings,
}
#[derive(Clone)]
pub struct Binding {
    pub action: Action,
    pub name: &'static str,
    pub mode: &'static str,
    pub description: &'static str,
    pub keys: Vec<String>,
}
pub fn registry() -> Vec<Binding> {
    let definitions = [
        (Action::Quit, "quit", "global", "Quit", "ctrl-c"),
        (
            Action::Down,
            "down",
            "navigation",
            "Next row/line",
            "j,down",
        ),
        (Action::Up, "up", "navigation", "Previous row/line", "k,up"),
        (
            Action::First,
            "first",
            "navigation",
            "First row/line",
            "g,home",
        ),
        (Action::Last, "last", "navigation", "Last row/line", "G,end"),
        (
            Action::PageDown,
            "page_down",
            "navigation",
            "Page down",
            "pagedown,ctrl-f",
        ),
        (
            Action::PageUp,
            "page_up",
            "navigation",
            "Page up",
            "pageup,ctrl-b",
        ),
        (
            Action::Back,
            "back",
            "navigation",
            "Back / clear filter",
            "esc,q",
        ),
        (
            Action::Palette,
            "palette",
            "navigation",
            "Command palette",
            ":",
        ),
        (
            Action::Help,
            "help",
            "navigation",
            "Effective key bindings",
            "?",
        ),
        (
            Action::Filter,
            "filter",
            "navigation",
            "Filter rows / search document",
            "/",
        ),
        (Action::Yaml, "yaml", "table", "Redacted YAML", "y,enter"),
        (
            Action::Describe,
            "describe",
            "table",
            "Resource description",
            "d",
        ),
        (
            Action::Explain,
            "explain",
            "table",
            "Evidence-based explanation",
            "X",
        ),
        (Action::Events, "events", "table", "Related Events", "E"),
        (
            Action::Timeline,
            "timeline",
            "table",
            "Session watch history",
            "T",
        ),
        (
            Action::Logs,
            "logs",
            "table",
            "Pod logs (container optional)",
            "l",
        ),
        (
            Action::PreviousLogs,
            "previous_logs",
            "table",
            "Previous container logs",
            "p",
        ),
        (
            Action::LogsVisible,
            "logs_visible",
            "table",
            "Logs: all known containers of visible Pods (max 8 sources)",
            "M",
        ),
        (
            Action::PauseLogs,
            "pause_logs",
            "logs",
            "Logs: pause/resume display (bounded ingestion continues)",
            "space",
        ),
        (
            Action::ClearLogs,
            "clear_logs",
            "logs",
            "Logs: clear retained lines",
            "z",
        ),
        (
            Action::FilterLogs,
            "filter_logs",
            "logs",
            "Logs: show only lines matching document search",
            "v",
        ),
        (
            Action::Refresh,
            "refresh",
            "navigation",
            "Refresh watch / refetch document with UID check",
            "r",
        ),
        (Action::Sort, "sort", "table", "Cycle sort column", "S"),
        (
            Action::Reverse,
            "reverse",
            "table",
            "Reverse sort direction",
            "I",
        ),
        (
            Action::AllNamespaces,
            "all_namespaces",
            "table",
            "All namespaces",
            "0",
        ),
        (Action::Namespaces, "namespaces", "table", "Namespaces", "n"),
        (Action::Wide, "wide", "table", "Wide columns", "w"),
        (Action::Wrap, "wrap", "document", "Toggle wrapping", "w"),
        (
            Action::ScrollLeft,
            "scroll_left",
            "document",
            "Scroll left (wrap off)",
            "left,h",
        ),
        (
            Action::ScrollRight,
            "scroll_right",
            "document",
            "Scroll right (wrap off)",
            "right,L",
        ),
        (
            Action::Fullscreen,
            "fullscreen",
            "document",
            "Toggle fullscreen",
            "F",
        ),
        (
            Action::ToggleWarnings,
            "toggle_warnings",
            "document",
            "Events: Warning only (toggle)",
            "W",
        ),
        (
            Action::SearchNext,
            "search_next",
            "document",
            "Next matching line",
            "n",
        ),
        (
            Action::SearchPrevious,
            "search_previous",
            "document",
            "Previous matching line",
            "N",
        ),
        (
            Action::HistoryBack,
            "history_back",
            "navigation",
            "Back to previous scope",
            "[",
        ),
        (
            Action::HistoryForward,
            "history_forward",
            "navigation",
            "Forward to next scope",
            "]",
        ),
    ];
    definitions
        .into_iter()
        .map(|(action, name, mode, description, keys)| Binding {
            action,
            name,
            mode,
            description,
            keys: keys.split(',').map(str::to_owned).collect(),
        })
        .collect()
}
#[derive(Clone)]
pub struct Keymap {
    pub bindings: Vec<Binding>,
}
impl Keymap {
    pub fn compile(overrides: &BTreeMap<String, BTreeMap<String, Vec<String>>>) -> Result<Self> {
        let mut bindings = registry();
        for (mode, entries) in overrides {
            for (name, keys) in entries {
                let entry = bindings
                    .iter_mut()
                    .find(|b| b.mode == mode && b.name == name)
                    .ok_or_else(|| anyhow::anyhow!("Unknown key action {mode}.{name}"))?;
                for key in keys {
                    parse_key(key)?;
                }
                entry.keys = keys.clone();
            }
        }
        for mode in ["table", "document", "logs"] {
            let mut used = Vec::new();
            for binding in bindings.iter().filter(|b| available(b.mode, mode)) {
                for key in &binding.keys {
                    let event = parse_key(key)?;
                    ensure!(
                        !used.contains(&event),
                        "Conflicting key binding {key} in {mode}"
                    );
                    used.push(event);
                }
            }
        }
        Ok(Self { bindings })
    }
    pub fn action(&self, key: KeyEvent, mode: &str) -> Option<Action> {
        self.bindings
            .iter()
            .filter(|b| available(b.mode, mode))
            .find(|b| {
                b.keys.iter().any(|k| {
                    parse_key(k).is_ok_and(|(code, modifiers)| {
                        code == key.code && modifiers == normalize(key.modifiers, key.code)
                    })
                })
            })
            .map(|b| b.action)
    }
    /// The effective primary key for `action` (its first configured key, honoring any
    /// user override), for UI hints that must show what a key actually does now, not
    /// a hardcoded default that could silently drift from a remapped binding.
    pub fn primary_key(&self, action: Action) -> Option<&str> {
        self.bindings
            .iter()
            .find(|b| b.action == action)
            .and_then(|b| b.keys.first())
            .map(String::as_str)
    }
    pub fn help(&self) -> String {
        self.bindings
            .iter()
            .map(|b| {
                format!(
                    "{:<12} {:<22} {}",
                    b.mode,
                    if b.keys.is_empty() {
                        "unbound".into()
                    } else {
                        b.keys.join(" / ")
                    },
                    b.description
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}
pub fn available(binding: &str, mode: &str) -> bool {
    binding == "global"
        || binding == mode
        || (mode != "input" && binding == "navigation")
        || (mode == "logs" && binding == "document")
}
fn normalize(mut m: KeyModifiers, c: KeyCode) -> KeyModifiers {
    if matches!(c, KeyCode::Char(_)) {
        m.remove(KeyModifiers::SHIFT);
    }
    m
}
fn parse_key(s: &str) -> Result<(KeyCode, KeyModifiers)> {
    let mut rest = s;
    let mut m = KeyModifiers::empty();
    for (prefix, flag) in [
        ("ctrl-", KeyModifiers::CONTROL),
        ("alt-", KeyModifiers::ALT),
    ] {
        if let Some(r) = rest.strip_prefix(prefix) {
            m.insert(flag);
            rest = r;
        }
    }
    let code = match rest {
        "enter" => KeyCode::Enter,
        "space" => KeyCode::Char(' '),
        "esc" => KeyCode::Esc,
        "up" => KeyCode::Up,
        "down" => KeyCode::Down,
        "left" => KeyCode::Left,
        "right" => KeyCode::Right,
        "home" => KeyCode::Home,
        "end" => KeyCode::End,
        "pageup" => KeyCode::PageUp,
        "pagedown" => KeyCode::PageDown,
        "tab" => KeyCode::Tab,
        "backtab" => KeyCode::BackTab,
        _ => {
            let mut chars = rest.chars();
            let c = chars.next().ok_or_else(|| anyhow::anyhow!("Empty key"))?;
            ensure!(chars.next().is_none(), "Unsupported key chord");
            KeyCode::Char(c)
        }
    };
    Ok((code, m))
}

#[derive(Clone, Debug, Default)]
pub struct ResourceCommand {
    pub resource: String,
    pub namespace: Option<String>,
    pub all: bool,
    pub context: Option<String>,
    pub labels: Option<String>,
    pub fields: Option<String>,
    pub filter: Option<String>,
}
#[derive(Clone, Debug)]
pub enum Command {
    Resource(ResourceCommand),
    Namespace(Option<String>),
    Context(Option<String>),
    Action(Action),
    Info,
    Reload,
    Logs {
        container: Option<String>,
        previous: bool,
    },
    Sort(String),
    Exec {
        container: Option<String>,
        command: Vec<String>,
    },
}

/// Quoted words have shell-like grouping only. Nothing is executed or expanded.
pub fn words(s: &str) -> Result<Vec<String>> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut quote = None;
    let mut escaped = false;
    for c in s.chars() {
        if escaped {
            current.push(c);
            escaped = false;
            continue;
        }
        if c == '\\' {
            escaped = true;
            continue;
        }
        if let Some(q) = quote {
            if c == q {
                quote = None;
            } else {
                current.push(c);
            }
            continue;
        }
        if c == '\'' || c == '"' {
            quote = Some(c);
        } else if c.is_whitespace() {
            if !current.is_empty() {
                words.push(std::mem::take(&mut current));
            }
        } else {
            current.push(c);
        }
    }
    ensure!(quote.is_none() && !escaped, "Unclosed quote or escape");
    if !current.is_empty() {
        words.push(current);
    }
    Ok(words)
}
pub fn parse(s: &str) -> Result<Command> {
    ensure!(s.len() <= 4096, "Command exceeds 4096 bytes");
    // A whitespace-delimited slash begins the local expression; quotes inside it
    // remain intact for the filter parser. Detect outside quotes only. exec's argv
    // has its own "--" separator and routinely contains absolute paths (a bare
    // "/bin/sh" starts with exactly this same whitespace-slash shape), so it must
    // never be misread as a filter boundary.
    let is_exec = s.trim_start_matches(':').split_whitespace().next() == Some("exec");
    let mut quote = None;
    let mut escaped = false;
    let mut boundary = None;
    let mut previous = ' ';
    for (i, c) in s.char_indices() {
        if escaped {
            escaped = false;
            previous = c;
            continue;
        }
        if c == '\\' {
            escaped = true;
            continue;
        }
        if let Some(q) = quote {
            if c == q {
                quote = None;
            }
        } else if c == '\'' || c == '"' {
            quote = Some(c);
        } else if !is_exec && c == '/' && previous.is_whitespace() {
            boundary = Some(i);
            break;
        }
        previous = c;
    }
    let (head, filter) = boundary
        .map(|i| (&s[..i], Some(s[i + 1..].to_owned())))
        .unwrap_or((s, None));
    let args = words(head.trim_start_matches(':'))?;
    let name = args
        .first()
        .ok_or_else(|| anyhow::anyhow!("Enter a resource or command"))?;
    let tail = &args[1..];
    // Commands that carry their own arguments have no zero-arg Action equivalent and
    // are handled here, before the registry lookup below, so they're never shadowed
    // by (and never shadow) a same-named registered action.
    match name.as_str() {
        "ctx" | "context" => {
            ensure!(tail.len() <= 1, "Use :ctx [context]");
            return Ok(Command::Context(tail.first().cloned()));
        }
        "ns" | "namespace" => {
            ensure!(tail.len() <= 1, "Use :ns [namespace]");
            return Ok(Command::Namespace(tail.first().cloned()));
        }
        "info" => {
            ensure!(tail.is_empty(), "Use :info");
            return Ok(Command::Info);
        }
        "reload" => {
            ensure!(tail.is_empty(), "Use :reload");
            return Ok(Command::Reload);
        }
        // Bare ":logs"/":previous_logs" are the SAME action as their key bindings; an
        // explicit container name is the only reason this isn't just Command::Action.
        "logs" if tail.is_empty() => return Ok(Command::Action(Action::Logs)),
        "logs" => {
            ensure!(tail.len() <= 1, "Use :logs [container]");
            return Ok(Command::Logs {
                container: tail.first().cloned(),
                previous: false,
            });
        }
        "previous_logs" if tail.is_empty() => return Ok(Command::Action(Action::PreviousLogs)),
        "previous_logs" => {
            ensure!(tail.len() <= 1, "Use :previous_logs [container]");
            return Ok(Command::Logs {
                container: tail.first().cloned(),
                previous: true,
            });
        }
        // Bare ":sort" is the SAME action as pressing the "sort" key binding (cycle to
        // the next column) -- a name collision between this and Action::Sort would
        // otherwise mean the key and the typed command with the same name did
        // different things. With an explicit column argument it sets sort directly.
        "sort" if tail.is_empty() => return Ok(Command::Action(Action::Sort)),
        "sort" => {
            ensure!(tail.len() == 1, "Use :sort [column[:asc|:desc]]");
            return Ok(Command::Sort(tail[0].clone()));
        }
        // A structured argv after an explicit "--" separator, never a shell string:
        // ":exec -- ls /" or ":exec worker -- sh -c 'echo hi'". At most one token
        // (the container) may precede "--"; never guessed when a Pod has several.
        "exec" => {
            let split = tail
                .iter()
                .position(|a| a == "--")
                .ok_or_else(|| anyhow::anyhow!("Use :exec [container] -- <command>"))?;
            ensure!(split <= 1, "Use :exec [container] -- <command>");
            let command = tail[split + 1..].to_vec();
            ensure!(!command.is_empty(), "Use :exec [container] -- <command>");
            return Ok(Command::Exec {
                container: tail[..split].first().cloned(),
                command,
            });
        }
        _ => {}
    }
    // Every zero-argument command name resolves through the SAME action registry that
    // drives keybindings and the effective-help listing -- there is exactly one place
    // that knows "quit" means Action::Quit, and it is `registry()`. A name typed here
    // that isn't a registered action name falls through to the resource-query parsing
    // below, matching how an unrecognized bare word was already treated.
    if let Some(binding) = registry().into_iter().find(|b| b.name == name.as_str()) {
        ensure!(
            tail.is_empty() && filter.is_none(),
            "This command takes no arguments"
        );
        return Ok(Command::Action(binding.action));
    }
    let mut query = ResourceCommand {
        resource: name.clone(),
        filter,
        ..Default::default()
    };
    let mut i = 0;
    while i < tail.len() {
        match tail[i].as_str() {
            "-A" | "--all-namespaces" => query.all = true,
            "-n" | "--namespace" | "--context" | "-l" | "--selector" | "-f"
            | "--field-selector" => {
                let value = tail
                    .get(i + 1)
                    .ok_or_else(|| anyhow::anyhow!("Option requires a value"))?
                    .clone();
                match tail[i].as_str() {
                    "-n" | "--namespace" => query.namespace = Some(value),
                    "--context" => query.context = Some(value),
                    "-l" | "--selector" => query.labels = Some(value),
                    _ => query.fields = Some(value),
                }
                i += 1;
            }
            v if v.starts_with('@') => query.context = Some(v[1..].to_owned()),
            v if v.starts_with('-') => bail!("Unknown scope option"),
            v if query.namespace.is_none() => query.namespace = Some(v.into()),
            _ => bail!("Too many resource arguments"),
        }
        i += 1;
    }
    ensure!(
        !(query.all && query.namespace.is_some()),
        "Use either -A or -n"
    );
    if let Some(f) = &query.filter {
        crate::filters::Expr::parse(f)?;
    }
    Ok(Command::Resource(query))
}

/// Every command-palette-suggestible name, all in one place: the handful of
/// argument-taking commands that have no zero-arg `Action` (so `parse` handles them
/// before ever consulting the registry), plus every name the registry itself defines.
/// Adding a binding to `registry()` makes it suggestible automatically -- there is no
/// second list to remember to update.
pub fn command_names() -> Vec<&'static str> {
    let mut names: Vec<&'static str> = vec!["ctx", "ns", "info", "reload", "sort", "exec"];
    names.extend(registry().iter().map(|b| b.name));
    names
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_scoped_query_without_shell_expansion() {
        let Command::Resource(q) =
            parse("pods -n prod -l 'app in (api,worker)' /status=Pending || restarts>3")
                .expect("valid")
        else {
            panic!("query")
        };
        assert_eq!(q.namespace.as_deref(), Some("prod"));
        assert_eq!(q.labels.as_deref(), Some("app in (api,worker)"));
        assert!(parse("pods -A -n prod").is_err());
    }
    #[test]
    fn exec_argv_with_an_absolute_path_is_never_misread_as_a_filter_boundary() {
        // "exec worker -- /bin/sh" has the exact same whitespace-slash shape the
        // filter-boundary heuristic looks for; exec's own "--" must win.
        assert!(matches!(
            parse("exec worker -- /bin/sh -c echo"),
            Ok(Command::Exec { container: Some(c), command })
                if c == "worker" && command == vec!["/bin/sh", "-c", "echo"]
        ));
        assert!(matches!(
            parse("exec -- /nonexistent-binary"),
            Ok(Command::Exec { container: None, command })
                if command == vec!["/nonexistent-binary"]
        ));
    }
    #[test]
    fn effective_bindings_detect_conflicts() {
        let mut overrides = BTreeMap::new();
        overrides.insert(
            "table".into(),
            BTreeMap::from([("yaml".into(), vec!["j".into()])]),
        );
        assert!(Keymap::compile(&overrides).is_err());
        assert!(Keymap::compile(&BTreeMap::new()).is_ok());
    }
    /// The core single-source-of-truth guarantee: every registered action's name,
    /// typed bare in the command palette, resolves to that SAME action -- there is no
    /// second hardcoded name->Action table (like the old `parse` match or `COMMANDS`
    /// list) to drift out of sync with the registry that also drives keybindings and
    /// the effective-help listing. This also covers the "sort"/"logs"/"previous_logs"
    /// collision between a key action and an argument-taking command sharing a name:
    /// bare resolves to the action, only an explicit argument takes the other path.
    #[test]
    fn every_registered_action_name_resolves_via_parse_to_the_same_action() {
        for binding in registry() {
            match parse(&format!(":{}", binding.name)) {
                Ok(Command::Action(a)) => assert_eq!(
                    a, binding.action,
                    "{} resolved to a different action than its own registry entry",
                    binding.name
                ),
                other => panic!(
                    "{}: registered action name did not resolve to Command::Action ({other:?})",
                    binding.name
                ),
            }
        }
    }
    #[test]
    fn logs_and_previous_logs_take_an_explicit_container_but_bare_is_the_action() {
        assert!(matches!(parse(":logs"), Ok(Command::Action(Action::Logs))));
        assert!(matches!(
            parse(":logs worker"),
            Ok(Command::Logs {
                container: Some(c),
                previous: false
            }) if c == "worker"
        ));
        assert!(matches!(
            parse(":previous_logs"),
            Ok(Command::Action(Action::PreviousLogs))
        ));
    }
    #[test]
    fn command_names_never_silently_drops_a_registered_action() {
        // The other half of "a name that resolves must be registered": every name the
        // palette suggests must itself be a real registry name or one of the small
        // fixed argument-taking commands -- never a stray string with no Action/Command
        // behind it.
        let names = command_names();
        for binding in registry() {
            assert!(
                names.contains(&binding.name),
                "{} is registered but not suggested",
                binding.name
            );
        }
    }
}
#[test]
fn logs_inherit_documents_and_validate_conflicts() {
    let keys = Keymap::compile(&BTreeMap::new()).expect("default map");
    assert_eq!(
        keys.action(
            KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE),
            "logs"
        ),
        Some(Action::PauseLogs)
    );
    assert_eq!(
        keys.action(
            KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE),
            "document"
        ),
        None
    );
    assert_eq!(
        keys.action(
            KeyEvent::new(KeyCode::Char('w'), KeyModifiers::NONE),
            "logs"
        ),
        Some(Action::Wrap)
    );
    let overrides = BTreeMap::from([(
        "logs".into(),
        BTreeMap::from([("pause_logs".into(), vec!["w".into()])]),
    )]);
    assert!(Keymap::compile(&overrides).is_err());
}
