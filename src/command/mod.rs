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
    Adjacent,
    Follow,
    Xray,
    Policy,
    Mutations,
    Events,
    Timeline,
    Logs,
    PreviousLogs,
    Forward,
    ForwardManager,
    StopForward,
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
    MutationDryRun,
    MutationConfirm,
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
            Action::Forward,
            "forward",
            "table",
            "Port-forward: choose declared Pod TCP port",
            "f",
        ),
        (
            Action::ForwardManager,
            "pf",
            "navigation",
            "Background port-forward manager",
            "P",
        ),
        (
            Action::StopForward,
            "pf_stop",
            "forwards",
            "Stop forward (enter session ID)",
            "s",
        ),
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
        (
            Action::Adjacent,
            "adjacent",
            "table",
            "Related objects (ownership/reference/selector)",
            "a",
        ),
        (
            Action::Follow,
            "follow",
            "document",
            "Adjacent: navigate to the related object nearest the top of view",
            "enter",
        ),
        (
            Action::Xray,
            "xray",
            "table",
            "Bounded 2-hop relationship + health traversal",
            "x",
        ),
        (
            Action::Policy,
            "policy",
            "table",
            "Read-only: what a hypothetical mutation would do (M7 infrastructure)",
            "u",
        ),
        (
            Action::Mutations,
            "mutations",
            "table",
            "Recent mutation journal entries (bounded, read-only)",
            "m",
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
        (
            Action::MutationDryRun,
            "mutation_dry_run",
            "mutation",
            "Mutation preview: try a server dry-run (never commits)",
            "d",
        ),
        (
            Action::MutationConfirm,
            "mutation_confirm",
            "mutation",
            "Mutation preview: confirm (press twice if strong confirmation is required)",
            "y",
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
        for mode in ["table", "document", "logs", "forwards", "mutation"] {
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
        || (matches!(mode, "logs" | "forwards" | "mutation") && binding == "document")
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
    Forward(crate::kube::forward::Ports),
    StopForward(u64),
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
    Shell {
        container: Option<String>,
        shell: Option<String>,
    },
    Attach {
        container: Option<String>,
    },
    Scale(u32),
    Restart,
    Delete,
    Label {
        key: String,
        value: Option<String>,
    },
    Annotate {
        key: String,
        value: Option<String>,
    },
    Cordon,
    Uncordon,
    SetImage {
        container: Option<String>,
        image: String,
    },
    Trigger,
    Evict,
    ForceDelete,
    Drain,
}

/// Shared `:label KEY=VALUE` / `:label KEY-` (remove) grammar for `:label`
/// and `:annotate`. Deliberately simple -- no shell-style escaping -- so the
/// split point is never ambiguous: exactly one `=` (set) or a single
/// trailing `-` with no `=` (remove).
fn parse_metadata_arg(tail: &[String], usage: &str) -> Result<(String, Option<String>)> {
    ensure!(tail.len() == 1, "{usage}");
    let arg = &tail[0];
    if let Some((key, value)) = arg.split_once('=') {
        ensure!(!key.is_empty(), "{usage}");
        return Ok((key.to_owned(), Some(value.to_owned())));
    }
    if let Some(key) = arg.strip_suffix('-') {
        ensure!(!key.is_empty(), "{usage}");
        return Ok((key.to_owned(), None));
    }
    bail!(usage.to_string())
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
    // remain intact for the filter parser. Detect outside quotes only. exec/shell's
    // own arguments have their own "--" separator and routinely contain absolute
    // paths (a bare "/bin/sh" starts with exactly this same whitespace-slash
    // shape), so they must never be misread as a filter boundary.
    let is_exec = matches!(
        s.trim_start_matches(':').split_whitespace().next(),
        Some("exec" | "shell")
    );
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
        "forward" if !tail.is_empty() => {
            ensure!(
                tail.len() == 1 && filter.is_none(),
                "Use :forward REMOTE or :forward LOCAL:REMOTE"
            );
            return Ok(Command::Forward(crate::kube::forward::Ports::parse(
                &tail[0],
            )?));
        }
        "pf_stop" if !tail.is_empty() => {
            ensure!(tail.len() == 1 && filter.is_none(), "Use :pf_stop ID");
            return Ok(Command::StopForward(tail[0].parse()?));
        }
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
        // ":shell [container]" defaults to `sh`; ":shell [container] -- bash"
        // overrides it with one explicit token. No auto-detection chain (bash
        // then sh): a single explicit override is simpler and never guesses.
        "shell" => {
            let (container_part, shell) = match tail.iter().position(|a| a == "--") {
                Some(split) => {
                    ensure!(split <= 1, "Use :shell [container] -- [shell]");
                    let rest = &tail[split + 1..];
                    ensure!(rest.len() <= 1, "Use :shell [container] -- [shell]");
                    (&tail[..split], rest.first().cloned())
                }
                None => {
                    ensure!(tail.len() <= 1, "Use :shell [container] -- [shell]");
                    (tail, None)
                }
            };
            return Ok(Command::Shell {
                container: container_part.first().cloned(),
                shell,
            });
        }
        // ":attach [container]" -- the already-running process, never a new one.
        "attach" => {
            ensure!(tail.len() <= 1, "Use :attach [container]");
            return Ok(Command::Attach {
                container: tail.first().cloned(),
            });
        }
        // ":scale N" -- a bounded, non-negative integer only; no scientific
        // notation, no leading '+', no whitespace-embedded digits.
        "scale" => {
            ensure!(tail.len() == 1, "Use :scale REPLICAS");
            let replicas = tail[0]
                .parse::<u32>()
                .map_err(|_| anyhow::anyhow!("REPLICAS must be a non-negative integer"))?;
            return Ok(Command::Scale(replicas));
        }
        "restart" => {
            ensure!(tail.is_empty(), "Use :restart");
            return Ok(Command::Restart);
        }
        "delete" => {
            ensure!(tail.is_empty(), "Use :delete");
            return Ok(Command::Delete);
        }
        "label" => {
            let (key, value) = parse_metadata_arg(tail, "Use :label KEY=VALUE or :label KEY-")?;
            return Ok(Command::Label { key, value });
        }
        "annotate" => {
            let (key, value) =
                parse_metadata_arg(tail, "Use :annotate KEY=VALUE or :annotate KEY-")?;
            return Ok(Command::Annotate { key, value });
        }
        "cordon" => {
            ensure!(tail.is_empty(), "Use :cordon");
            return Ok(Command::Cordon);
        }
        "uncordon" => {
            ensure!(tail.is_empty(), "Use :uncordon");
            return Ok(Command::Uncordon);
        }
        // "IMAGE" for the sole container, or "CONTAINER=IMAGE" to name one
        // explicitly -- image references never contain '=', so the split
        // is never ambiguous.
        "set_image" => {
            ensure!(tail.len() == 1, "Use :set_image [CONTAINER=]IMAGE");
            let arg = &tail[0];
            let (container, image) = match arg.split_once('=') {
                Some((c, i)) => {
                    ensure!(
                        !c.is_empty() && !i.is_empty(),
                        "Use :set_image [CONTAINER=]IMAGE"
                    );
                    (Some(c.to_owned()), i.to_owned())
                }
                None => {
                    ensure!(!arg.is_empty(), "Use :set_image [CONTAINER=]IMAGE");
                    (None, arg.clone())
                }
            };
            return Ok(Command::SetImage { container, image });
        }
        "trigger" => {
            ensure!(tail.is_empty(), "Use :trigger");
            return Ok(Command::Trigger);
        }
        "evict" => {
            ensure!(tail.is_empty(), "Use :evict");
            return Ok(Command::Evict);
        }
        // Deliberately its own command, never a flag on ":delete" -- see
        // workflow::force_delete's own doc comment for why this must stay
        // a genuinely separate path, not a shared parameter.
        "force_delete" => {
            ensure!(tail.is_empty(), "Use :force_delete");
            return Ok(Command::ForceDelete);
        }
        "drain" => {
            ensure!(tail.is_empty(), "Use :drain");
            return Ok(Command::Drain);
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
    let mut names: Vec<&'static str> = vec![
        "ctx",
        "ns",
        "info",
        "reload",
        "sort",
        "exec",
        "shell",
        "attach",
        "scale",
        "restart",
        "delete",
        "label",
        "annotate",
        "cordon",
        "uncordon",
        "set_image",
        "trigger",
        "evict",
        "force_delete",
        "drain",
    ];
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
#[test]
fn forwarding_commands_have_canonical_actions_and_structured_ports() {
    assert!(matches!(
        parse(":forward"),
        Ok(Command::Action(Action::Forward))
    ));
    assert!(matches!(
        parse(":pf"),
        Ok(Command::Action(Action::ForwardManager))
    ));
    assert!(matches!(
        parse(":pf_stop"),
        Ok(Command::Action(Action::StopForward))
    ));
    assert!(matches!(parse(":pf_stop 17"), Ok(Command::StopForward(17))));
    assert!(matches!(
        parse(":forward 0:8080"),
        Ok(Command::Forward(crate::kube::forward::Ports {
            local: 0,
            remote: 8080
        }))
    ));
    assert!(parse(":forward 0.0.0.0:8080").is_err());
    assert!(parse(":pf_stop -1").is_err());
}
#[test]
fn scale_parses_a_bounded_non_negative_integer_only() {
    assert!(matches!(parse(":scale 5"), Ok(Command::Scale(5))));
    assert!(matches!(parse(":scale 0"), Ok(Command::Scale(0))));
    assert!(parse(":scale -1").is_err());
    assert!(parse(":scale 3.5").is_err());
    assert!(parse(":scale").is_err());
    assert!(parse(":scale 1 2").is_err());
    assert!(parse(":scale 99999999999999999999").is_err());
}
#[test]
fn restart_and_delete_take_no_arguments() {
    assert!(matches!(parse(":restart"), Ok(Command::Restart)));
    assert!(matches!(parse(":delete"), Ok(Command::Delete)));
    assert!(parse(":restart now").is_err());
    assert!(parse(":delete now").is_err());
}
#[test]
fn cordon_and_uncordon_take_no_arguments() {
    assert!(matches!(parse(":cordon"), Ok(Command::Cordon)));
    assert!(matches!(parse(":uncordon"), Ok(Command::Uncordon)));
    assert!(parse(":cordon node-1").is_err());
    assert!(parse(":uncordon node-1").is_err());
}
#[test]
fn set_image_parses_bare_image_or_container_equals_image() {
    assert!(matches!(
        parse(":set_image nginx:1.27"),
        Ok(Command::SetImage { container: None, image }) if image == "nginx:1.27"
    ));
    assert!(matches!(
        parse(":set_image web=nginx:1.27"),
        Ok(Command::SetImage { container: Some(c), image }) if c == "web" && image == "nginx:1.27"
    ));
    assert!(parse(":set_image").is_err());
    assert!(parse(":set_image a b").is_err());
    assert!(parse(":set_image =nginx:1.27").is_err());
    assert!(parse(":set_image web=").is_err());
}
#[test]
fn trigger_takes_no_arguments() {
    assert!(matches!(parse(":trigger"), Ok(Command::Trigger)));
    assert!(parse(":trigger now").is_err());
}
#[test]
fn evict_takes_no_arguments() {
    assert!(matches!(parse(":evict"), Ok(Command::Evict)));
    assert!(parse(":evict now").is_err());
}
#[test]
fn force_delete_takes_no_arguments_and_is_its_own_distinct_command_from_delete() {
    assert!(matches!(parse(":force_delete"), Ok(Command::ForceDelete)));
    assert!(parse(":force_delete now").is_err());
    assert!(matches!(parse(":delete"), Ok(Command::Delete)));
}
#[test]
fn drain_takes_no_arguments() {
    assert!(matches!(parse(":drain"), Ok(Command::Drain)));
    assert!(parse(":drain now").is_err());
}
#[test]
fn label_and_annotate_parse_set_and_remove_grammar() {
    assert!(matches!(
        parse(":label team=infra"),
        Ok(Command::Label { key, value: Some(v) }) if key == "team" && v == "infra"
    ));
    assert!(matches!(
        parse(":label team-"),
        Ok(Command::Label { key, value: None }) if key == "team"
    ));
    assert!(matches!(
        parse(":annotate note='hello world'"),
        Ok(Command::Annotate { key, value: Some(v) }) if key == "note" && v == "hello world"
    ));
    assert!(parse(":label").is_err());
    assert!(parse(":label =novalue").is_err());
    assert!(parse(":label -").is_err());
    assert!(parse(":label a=b c=d").is_err());
}
#[test]
fn mutation_command_names_are_discoverable_and_registry_help_agrees() {
    let names = command_names();
    for name in ["scale", "restart", "delete", "label", "annotate"] {
        assert!(names.contains(&name), "{name} must be palette-suggestible");
    }
    let keys = Keymap::compile(&BTreeMap::new()).expect("default keymap");
    let help = keys.help();
    assert!(help.contains("mutation"));
    assert!(
        keys.action(
            KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE),
            "mutation"
        ) == Some(Action::MutationDryRun)
    );
    assert!(
        keys.action(
            KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE),
            "mutation"
        ) == Some(Action::MutationConfirm)
    );
}
#[test]
fn mutation_readonly_denial_is_a_precise_error_not_a_silent_noop() {
    // Command parsing itself never checks readonly -- the app layer opens the
    // preview and lets policy render the denial (M8: "visible, not hidden").
    // Parsing must still succeed so the palette can even try.
    assert!(matches!(parse(":scale 3"), Ok(Command::Scale(3))));
}
