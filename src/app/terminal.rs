//! Reusable terminal-suspension guard for handing the real terminal to an
//! interactive remote process (exec shell today; attach later, same mechanism).
//!
//! Deliberately narrow: it leaves/re-enters the alternate screen only. Raw mode
//! stays enabled throughout -- both Ratatui and a remote PTY need it, and toggling
//! it twice buys nothing. Restoration runs on `Drop`, so it happens whether the
//! interactive session ends by clean exit, a local I/O error, or an early `?`
//! return; the caller cannot forget to restore by missing a cleanup call on some
//! path.
///
/// Known, bounded, documented limitation (see docs/EXEC.md): crossterm's
/// `EventStream` (used for all normal TUI input) can have a background thread
/// blocked reading raw stdin bytes into its own process-wide static buffer at the
/// exact moment a shell session starts. A keystroke typed in that narrow window
/// (before this guard's caller starts forwarding stdin itself) can be captured by
/// that buffer instead of the shell, surfacing later as a stray, delayed TUI
/// keypress once the shell ends and a fresh `EventStream` resumes polling it --
/// not a hang, not corruption, just an occasional misdelivered keystroke in a
/// window measured in the low tens of milliseconds. Never observed live during
/// this project's acceptance passes, but not achievable to fully close without
/// replacing `EventStream`-based input for the whole app, which is out of scope.
pub struct TerminalHandoff;

impl TerminalHandoff {
    pub fn enter() -> std::io::Result<Self> {
        crossterm::execute!(std::io::stdout(), crossterm::terminal::LeaveAlternateScreen)?;
        Ok(Self)
    }
}

impl Drop for TerminalHandoff {
    fn drop(&mut self) {
        // Best-effort: a Drop cannot propagate an error, and failing to restore
        // silently is strictly better than panicking mid-unwind over a screen
        // buffer. If this genuinely fails the terminal was probably already gone
        // (e.g. stdout closed), in which case there is nothing left to restore.
        let _ = crossterm::execute!(std::io::stdout(), crossterm::terminal::EnterAlternateScreen);
    }
}
