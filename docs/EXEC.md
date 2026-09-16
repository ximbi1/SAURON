# Exec (M4.2/M4.2b contract: one-shot exec, interactive shell, and attach all ACCEPTED)

`:exec [container] -- <command...>` runs one native, non-interactive Kubernetes exec
(no stdin, no TTY) against the currently selected Pod, structured argv only -- never
a shell string. At most one token may precede the mandatory `--` separator (the
container name); everything after `--` is passed verbatim as the remote command's
argv. A multi-container Pod requires the container to be named explicitly, exactly
like bare `:logs`; a single-container Pod defaults to it.

Denied outright under `settings.readonly` (the default), before any connection is
attempted -- the check runs synchronously in the command dispatcher, so a denial
never spends a network round-trip. `--readonly` on the CLI is a hard override: it
forces `readonly = true` regardless of what a cluster/context config layer requests,
refreshed on every reconnect and configuration reload. Lifting it (`readonly = false` in `config.toml`, no
CLI flag to set it, deliberately) enables both `:exec` and the "OPERATIONAL
(exec/shell enabled)" heading in place of "READ ONLY". Never exposed for the
project's production cluster during this development task -- exercised only against
`kind-sauron-test`.

The target Pod's UID is checked before the exec connection and again right after it
opens, the same best-effort pre/post pattern `kube::logs` uses (the exec subresource
has no UID precondition of its own); a replaced Pod fails with "Pod replaced; select
its new incarnation" rather than silently running against the wrong incarnation.

Output lines are tagged `[stdout]`/`[stderr]` and appended to the shared Document
viewer in arrival order, **not merged/sorted by stream** -- both streams are drained
concurrently, so interleaving between them is not deterministic. The final line/
status reflects the remote exit: `Ended` for exit 0, `Failed: <message>` (including
the exit code) otherwise, via the same session-outcome status line every session
type uses. `r` (Refresh) re-runs the exact original frozen request under a fresh
request/session identity, exactly like logs.

Bounds and behavior mirror `kube::logs` deliberately: 16 KiB line clip, sanitized
control sequences (`safety::text`), inherited document keymap (search/scroll/wrap/
fullscreen/pause/clear/matching-line filter -- pause/clear apply the same as for a
live log stream, though a one-shot exec's output is finite).

## Interactive shell (`:shell`)

`:shell [container]` opens one interactive TTY session against `sh` in the target
container (`:shell [container] -- <path>` overrides the shell binary explicitly --
deliberately no bash-then-sh auto-detection chain, which would need a wasted or
ambiguous partial attach; `sh` is present on essentially every real container
image). Same container-selection rule as `:exec`. Denied under `settings.readonly`
exactly like `:exec`, before any connection attempt.

Unlike every other document-opening action, an interactive shell is **foreground
and blocking** by design (see `docs/SESSIONS.md`): nothing else in the app runs
concurrently with it (the whole event loop is inside the one call), so it is not
routed through `app::session::Sessions` -- there is nothing background to "own."
`app::terminal::TerminalHandoff` (new, small, RAII) leaves Ratatui's alternate
screen for the session's duration and restores it on `Drop`, so restoration
happens regardless of how the session ends -- clean exit, Ctrl-D, remote
disconnect, or a local I/O error. Raw mode is never toggled (both Ratatui and a
remote PTY need it enabled continuously); only the alternate screen is
suspended/resumed. Terminal resize is forwarded to the remote PTY by polling the
local terminal size every 250ms and sending a `TerminalSize` message through
`AttachedProcess::terminal_size()` whenever it changes -- confirmed live via
`stty size` inside the remote shell tracking several rapid local resizes exactly.
Ctrl-C is never intercepted locally during a shell session (raw mode means it is
just byte `0x03`, forwarded like any other byte -- confirmed live interrupting a
long-running remote `sleep`, with the shell session still alive afterward, exactly
like a real `kubectl exec -it`). Ctrl-D forwards local EOF to the remote's stdin
and keeps draining its output until it actually closes.

### Known, root-caused, bounded limitation: one input chunk can be lost after a shell ends

`tokio::io::Stdin`'s blocking-pool read cannot be cancelled on Unix (a documented
tokio limitation, not specific to this project). Whenever the interactive
forwarding loop's `select!` resolves via the *other* branch (the remote side
closing) while a local stdin read was still in flight -- observed live after both
a typed `exit` and a local Ctrl-D, so this is not reliably tied to which side
initiates the close -- that read is orphaned: it keeps running on a background
thread, silently consuming the *next* available chunk of real stdin data (byte-
traced live: observed eating an entire subsequently-typed command, including a
Ctrl-C) before self-terminating. Whatever byte arrives *after* that gets consumed
is what a freshly constructed `EventStream` actually sees once the TUI resumes --
byte-traced live to consistently be a bare `Enter` (the tail of whatever multi-key
sequence got eaten).

The mitigation shipped here (`run()`'s `suppress_phantom_enter` flag, set right
after any shell session ends, cleared on the very next key) discards exactly one
bare `Enter` if it is the first key seen after resuming. This does not recover the
swallowed input -- it converts the failure mode from *wrong action* (a stray Enter
opening the wrong document, confirmed live before this fix) into a *safe no-op*
(nothing visibly happens; the user notices and simply retries the same command,
confirmed live to succeed immediately on retry). **Terminal state itself (raw
mode, `stty`) is unaffected by this and was confirmed correct in every tested
ending** -- this limitation is about an occasional lost keystroke/command
immediately after a shell session, never about terminal corruption.

Fully closing this gap needs a genuinely cancellation-safe local stdin reader
(e.g. a non-blocking fd wrapped in `tokio::io::unix::AsyncFd`, polled via epoll
readiness rather than a blocking thread-pool read) -- future work, not shipped
speculatively here.

## Attach (`:attach`)

`:attach [container]` attaches to the container's already-running main process
(`Api::attach`, distinct from exec/shell: no argv, no new process -- there is
nothing to start). Same container-selection rule as `:exec`/`:shell`; denied
under `settings.readonly` before any connection attempt, same as both.

Only offered as interactive when the container's own Pod spec was created with
both `stdin: true` and `tty: true` -- Kubernetes does not allocate a PTY for the
container otherwise, so a bare attach would be a one-way view with no stdin
channel to type into. Refused explicitly and immediately (`Container <name> was
not started with stdin+tty (required for an interactive attach); use :logs to
view its output instead`) rather than silently degrading to a read-only mode
nobody asked for -- confirmed live against a container without these fields.

Reuses the identical `TerminalHandoff` guard and `forward_interactive()` byte-
relay loop `:shell` uses (extracted into a shared function once attach needed
it too) -- same resize polling, same Ctrl-C-forwarded-raw behavior, same
terminal restoration guarantees.

### Detaching: Ctrl-] (0x1D), local-only

Found live on the very first attach test: the already-running process an
attach connects to was very likely **not started expecting stdin at all** (it
is not a shell waiting to execute what you type) -- Ctrl-D, which works for
`:shell` because a real shell explicitly reacts to it, does nothing here, and
the forwarding loop would otherwise wait forever for output that never stops,
hanging the whole TUI. `Ctrl-]` (a single byte, `0x1D`, chosen for the same
reason telnet and other terminal tools traditionally use it) ends the local
session unconditionally, **without ever signaling the remote process** --
exactly the same "cancellation ends local observation, never remote rollback"
distinction `docs/SESSIONS.md` already draws for background sessions, just
needed here too. Confirmed live: detaching this way leaves the attached
container running, completely unaffected (checked via `kubectl get pod` and
`kubectl logs` immediately after -- still `Running`, 0 restarts, output
continuing). This key applies to `:shell` as well (harmless there; Ctrl-D
already covers the common case).

## Live-accepted (`kind-sauron-test`, fresh binary)

**One-shot exec:** denied under default readonly with zero connection attempts;
`--readonly` CLI override forces denial even when config requests `readonly =
false`; explicit container success with real stdout+stderr capture; unknown
container name; missing container choice on a multi-container Pod; a real
nonexistent-executable failure from the API; non-zero exit code surfaced with the
exit status; Pod delete+recreate correctly clearing the stale selection (`Select a
Pod first`) rather than exec'ing into a replaced incarnation; exec against the
fresh UID after reselecting; Refresh restarting under a new identity; 32x9
clipping; exact `stty` terminal state preserved end to end.

**Interactive shell:** a real remote prompt with full bidirectional byte
forwarding (typed commands and their real output round-tripped correctly);
explicit nonexistent-shell-path failure surfaced from the real API; the target
Pod force-deleted mid-session, surfaced as `Shell Failed` with the real exit code
(137, matching SIGKILL) rather than a hang or crash; the whole cluster network
disconnected mid-session (`docker network disconnect`), ending the session
cleanly and degrading the background watch to a clear transport-error status that
fully recovered on `Refresh` after reconnecting; repeated local resize tracked
correctly via `stty size` inside the remote shell; Ctrl-C forwarded to the remote
without ending the session; Ctrl-D ending it cleanly; eight consecutive open/use/
close round-trips (normal exit and Ctrl-D both exercised), confirming the
known limitation above and its safe-no-op/retry mitigation rather than any wrong
action or corruption; a full quit from SAURON afterward with exact `stty` state
preserved.

**Attach:** an explicit rejection of a container without `stdin: true, tty: true`
in its spec, with zero connection attempts; a real attach to an already-running
process's live output (confirmed distinct from exec: no new process started,
just observed); the local-only `Ctrl-]` detach leaving the remote container
running and unaffected (`kubectl get pod`/`logs` checked immediately after);
the target Pod force-deleted mid-session ending the attach cleanly rather than
hanging; a final quit from SAURON afterward with exact `stty` state preserved.

See `docs/M4_ACCEPTANCE.md` for the real bugs this pass found and fixed: a
command-grammar collision between `/`-filter syntax and absolute-path argv,
readonly being entirely unwired before this slice, `Terminal::clear()`'s
internal cursor-position query racing the just-ended session's own stdin
reader and timing out, and attach hanging forever against a process that
never reads stdin (fixed by the `Ctrl-]` local detach above).
