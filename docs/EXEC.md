# Exec (M4.2 contract: one-shot exec ACCEPTED; interactive shell NOT YET BUILT)

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
refreshed on every reconnect. Lifting it (`readonly = false` in `config.toml`, no
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

## Deliberately not built yet

**Interactive TTY shell** (`:shell`) needs a terminal-suspension mechanism (hand the
real terminal to the remote process's stdin/stdout, leaving Ratatui's alternate
screen without racing its own stdin `EventStream` reader) that this slice does not
attempt. Researched: `kube` 4.2's `AttachedProcess`/`AttachParams::interactive_tty()`
support stdin/stdout/TTY and a resize channel over the same `ws` feature already
enabled for this project (`Api::exec`/`Api::attach`, native WebSocket protocol, no
SPDY, no new dependency needed). Building it safely is the next M4.2 sub-slice, not
shipped speculatively here.

**Attach** (`Api::attach`, distinct from exec) is researched but not implemented;
same terminal-handoff dependency as interactive shell.

## Live-accepted (`kind-sauron-test`, fresh binary)

Denied under default readonly with zero connection attempts; `--readonly` CLI
override forces denial even when config requests `readonly = false`; explicit
container success with real stdout+stderr capture; unknown container name; missing
container choice on a multi-container Pod; a real nonexistent-executable failure
from the API; non-zero exit code surfaced with the exit status; Pod delete+recreate
correctly clearing the stale selection (`Select a Pod first`) rather than exec'ing
into a replaced incarnation; exec against the fresh UID after reselecting; Refresh
restarting under a new identity; 32x9 clipping; exact `stty` terminal state
preserved end to end. See `docs/M4_ACCEPTANCE.md` for the two real bugs this pass
found and fixed (a command-grammar collision between `/`-filter syntax and
absolute-path argv, and readonly being entirely unwired before this slice).
