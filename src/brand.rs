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

/// M13.3: the same ASCII eye mark already shown in the project's own
/// marketing website, redrawn (not copy-pasted) so every line's own
/// diagonal is mathematically symmetric within a single fixed-width
/// (9, deliberately odd) field -- an even total width forced an
/// unavoidable 1-column rounding asymmetry on any line whose own
/// content had odd length, found live across two real-screenshot
/// review rounds. Width 9 lets every line's content also be an odd
/// length (7 -> 9, 5 -> 9, 3 -> 9), so every diagonal lands on an exact
/// integer column with equal padding both sides -- no rounding case
/// left to get wrong.
pub const BANNER: [&str; 4] = ["╭───────╮", "  ╲ ◉ ╱  ", "   ╲─╱   ", " SAUR-ON "];
pub const BANNER_WIDTH: u16 = 9;
