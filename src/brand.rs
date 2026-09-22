//! Runtime branding is centralized to make a future rename tractable.
//! `BINARY` is a literal, not `env!("CARGO_PKG_NAME")`: the crates.io
//! publish identifier (`Cargo.toml`'s `[package].name`, "saur-on" --
//! "sauron" was already taken) is intentionally decoupled from the
//! actual binary/config-dir name, which stays "sauron".
pub const NAME: &str = "SAURON";
pub const BINARY: &str = "sauron";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const CONFIG_DIR: &str = BINARY;
pub const DATA_DIR: &str = BINARY;
pub const WEBSITE: Option<&str> = None;
pub const MARK: &str = "◉";
pub const USER_AGENT: &str = concat!("sauron", "/", env!("CARGO_PKG_VERSION"));

/// M13.3: the same 4-line ASCII mark already shown in the project's own
/// marketing website (`sauron-s-command-center`'s decorative
/// `.terminal-eye` block) -- reused verbatim here so the real TUI and the
/// website agree on one look, not two. Lines are ragged on purpose
/// (matching the website's own markup); the renderer centers each one
/// within `BANNER_WIDTH`, the widest line's own character count.
pub const BANNER: [&str; 4] = ["╭──────╮", "╲  ◉  ╱", " ╲──╱ ", "SAUR-ON"];
pub const BANNER_WIDTH: u16 = 8;
