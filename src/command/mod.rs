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
            Action::Refresh,
            "refresh",
            "table",
            "Restart the current watch",
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
            Action::Fullscreen,
            "fullscreen",
            "document",
            "Toggle fullscreen",
            "F",
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
        for mode in ["table", "document"] {
            let mut used = Vec::new();
            for binding in bindings
                .iter()
                .filter(|b| b.mode == mode || b.mode == "global" || b.mode == "navigation")
            {
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
            .filter(|b| {
                b.mode == "global" || b.mode == mode || (mode != "input" && b.mode == "navigation")
            })
            .find(|b| {
                b.keys.iter().any(|k| {
                    parse_key(k).is_ok_and(|(code, modifiers)| {
                        code == key.code && modifiers == normalize(key.modifiers, key.code)
                    })
                })
            })
            .map(|b| b.action)
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
        "esc" => KeyCode::Esc,
        "up" => KeyCode::Up,
        "down" => KeyCode::Down,
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
    // remain intact for the filter parser. Detect outside quotes only.
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
        } else if c == '/' && previous.is_whitespace() {
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
    let action = match name.as_str() {
        "q" | "quit" => Some(Action::Quit),
        "help" => Some(Action::Help),
        "yaml" => Some(Action::Yaml),
        "describe" => Some(Action::Describe),
        "explain" => Some(Action::Explain),
        "events" => Some(Action::Events),
        "timeline" => Some(Action::Timeline),
        _ => None,
    };
    if let Some(action) = action {
        ensure!(
            tail.is_empty() && filter.is_none(),
            "This command takes no arguments"
        );
        return Ok(Command::Action(action));
    }
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
        "logs" => {
            ensure!(tail.len() <= 1, "Use :logs [container]");
            return Ok(Command::Logs {
                container: tail.first().cloned(),
                previous: false,
            });
        }
        "previous-logs" => {
            ensure!(tail.len() <= 1, "Use :previous-logs [container]");
            return Ok(Command::Logs {
                container: tail.first().cloned(),
                previous: true,
            });
        }
        "sort" => {
            ensure!(tail.len() == 1, "Use :sort column[:asc|:desc]");
            return Ok(Command::Sort(tail[0].clone()));
        }
        _ => {}
    }
    let mut query = ResourceCommand {
        resource: if name == "ns" || name == "namespace" {
            "namespaces".into()
        } else {
            name.clone()
        },
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

pub const COMMANDS: &[&str] = &[
    "ctx",
    "ns",
    "yaml",
    "describe",
    "explain",
    "events",
    "timeline",
    "logs",
    "previous-logs",
    "sort",
    "info",
    "reload",
    "help",
    "q",
];

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
    fn effective_bindings_detect_conflicts() {
        let mut overrides = BTreeMap::new();
        overrides.insert(
            "table".into(),
            BTreeMap::from([("yaml".into(), vec!["j".into()])]),
        );
        assert!(Keymap::compile(&overrides).is_err());
        assert!(Keymap::compile(&BTreeMap::new()).is_ok());
    }
}
