# Long-running ownership (M4.0 implementation)

Current application workers already live in Runtime's JoinSet and use cancellation,
epoch and request gates. Discovery/watches/documents stay there. `app/session.rs` adds
a small owner for log sessions, not a universal protocol abstraction.

Each session has a monotonic ID, kind, immutable scope/target, cancellation token,
start instant, Starting/Running/Stopping/Ended state and final outcome. Completion is
owned separately from view events: abandoning a view rejects its data but still reaps
its task. A full event queue must not prevent cancellation or final accounting.
Limits: 8 active tasks and 64 completed records. Shutdown cancels, waits two seconds,
aborts overdue work and joins it. Dropping the owner cancels tokens and aborts owned tasks.
Completion/panic/abort is attributed through Tokio task identity, never generic watch
error text. `:info` includes active sessions. API failures retain safe contextual messages;
finer error categories for operational protocols remain subsequent M4 work.

Foreground logs belong to epoch + request + session ID and original Pod UID/context.
Closing/replacing their document or changing scope cancels them. Search and palette
over the same document retain ownership. Status belongs to that document/session,
not persistent watch errors. Connecting is not Streaming; EOF is not failure.

Background forwards will own an immutable connection/target UID plus local/remote port
and SessionId, independent of foreground navigation. They must not simply bypass every
identity check. Protocol workers stay separate; exec terminal handoff is foreground.

Cancellation ends local observation/control, **not remote rollback**. An executed
command may already have changed application state. Ending a tunnel closes local
connections, not transactions performed through them. Logs have no mutation authority.

Existing terminal guard restores Ratatui at final exit. `app::terminal::TerminalHandoff`
(RAII, leaves/re-enters only the alternate screen, never touches raw mode) now
provides interactive suspension for the shell, exercised centrally in `run()`'s
loop; attach will reuse it directly. See `docs/EXEC.md` for the one known,
bounded limitation this uncovered (an uncancellable stdin read can occasionally
swallow one input chunk right after a session ends).
