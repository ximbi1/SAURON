//! Runtime branding is centralized to make a future rename tractable.
pub const NAME: &str = "SAURON";
pub const BINARY: &str = env!("CARGO_PKG_NAME");
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const CONFIG_DIR: &str = BINARY;
pub const DATA_DIR: &str = BINARY;
pub const WEBSITE: Option<&str> = None;
pub const MARK: &str = "◉";
pub const USER_AGENT: &str = concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"));
