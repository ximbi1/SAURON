# M4 interactive sessions acceptance ledger

Baseline 2026-09-16: annotated m3-accepted, 9eac329. Full fmt/check/clippy/test green:
55 unit + 10 fake HTTP; one opt-in cluster test ignored. Production untouched.
No M4 acceptance implied by M3 tests. Isolated kind was absent and has been recreated
via bootstrap-test-cluster.sh; node Ready/identity verified. Every fixture write uses test-cluster.sh.

| Slice | Intended contract | State | Coverage / live evidence | Gaps / verdict |
| --- | --- | --- | --- | --- |
| M4.0 | Owned identity, cancellation, startup/running/stopping/final outcomes; prove with logs | ACCEPTED | 62 unit + 11 fake HTTP; live accept-m4.py, opt-in cluster test | Protocol-specific operational sessions still subsequent slices |
| M4.1 | Bounded advanced logs, explicit container/source, controls, UID and scope integrity | ACCEPTED | 65 unit + 11 fake HTTP; live accept-m4.py logs, twice | Aggregation scope is explicit visible-Pods/all-containers only; see LOGS.md |
| M4.2 | Native structured exec/shell, readonly boundary, terminal handoff/restore | ACCEPTED | 72 unit + 11 fake HTTP; live one-shot exec + interactive shell, all 9 requested adversarial endings | One known, bounded, documented limitation: an orphaned stdin read can occasionally swallow one input chunk after a session ends; mitigated to a safe no-op/retry, not fully closed -- see EXEC.md |
| M4.2b | Separate native attach semantics or justified deferral | ACCEPTED | 72 unit + 11 fake HTTP; live attach against kind-sauron-test | Reuses M4.2's terminal guard/forwarding loop exactly, no new mechanics -- see EXEC.md |
| M4.3 | Loopback background forwarding, manager, durable identity, bounded connections | ACCEPTED | 76 unit + 14 fake HTTP; repeated live accept-m4-forward.py; PORT_FORWARD.md | Pod TCP only; no Service resolution/reconnect/autostart; no endurance claim |
| M4.4 | Combined adversarial flows, old-milestone regressions, measured soak | NOT STARTED | None | NOT ACCEPTED |

## M4.0 acceptance

- Unit: unique IDs, legal state transitions, bounded retained outcomes, owner drop,
  cancellation before polling/during connect/full channel, panic attribution, shutdown.
- Regression: completion while searching/in palette updates the same log document;
  old epoch/request/session data never affects a new view; no generic error inheritance.
- Live: basic follow, previous, explicit container; rapid open/close/open; context/ns
  change during startup; clean quit. Fresh binary and terminal restoration required.

## M4.1 acceptance

Single follow; explicit regular/init/ephemeral choice; previous; multiple containers;
bounded concurrent source tags if implemented; cancel/restart; rapid scope changes;
deleted/recreated UID; 32x9; overlong/ANSI input; line/byte eviction visibly reported;
search/filter/pause semantics; return to table/quit; never stale lines. Record actual
implemented aggregation scope rather than claiming all workloads/Services/marks.

## M4.2 / M4.2b acceptance

Isolated fixtures only: expected one-shot output, explicit container, shell, no bash,
command error/missing executable, gone/replaced/non-running target, startup cancellation,
resize/Ctrl-C/disconnect, palette after return, clean final terminal state. Attach needs
its own tests, not exec evidence. Readonly denial required before any connection attempt.

**One-shot exec: ACCEPTED.** `kube::exec` (new module) runs one native, non-interactive
exec (`AttachParams` with `stdin(false)`/`tty(false)`) per `:exec [container] --
<argv>`, structured argv only. Explicit container required on a multi-container Pod
(same convention as bare `:logs`); UID checked pre/post-connect like logs; stdout/
stderr drained concurrently and tagged, arrival order not merge-sorted; exit status
surfaced as the session `Outcome` (`Completed` for exit 0, `Failed: <message>`
otherwise) through the same status-line machinery every session type already uses;
Refresh re-runs the frozen original request under a fresh identity. Contract:
`docs/EXEC.md`.

**Interactive TTY shell: ACCEPTED.** Built exactly in the order requested: designed
`app::terminal::TerminalHandoff` (a small RAII guard leaving/re-entering Ratatui's
alternate screen only, never touching raw mode) first; validated its restore
behavior live across every ending below *before* considering the feature done;
then `kube::exec::interactive()` forwards raw bytes both ways over
`AttachParams::interactive_tty()`, polling local terminal size every 250ms to
drive the remote resize channel. `:shell [container]` defaults to `sh` (no
bash-then-sh auto-detection -- explicit override only, per the "avoid a complex
remote shell detector" guidance). Foreground and blocking by design: it does not
go through `Sessions` (see `docs/SESSIONS.md` and `docs/EXEC.md`) since nothing
else runs concurrently with it.

**Attach (M4.2b): ACCEPTED.** `:attach [container]` (`Api::attach`, distinct
from exec: attaches to the already-running process, no argv, no new process)
reuses the identical `TerminalHandoff` guard and `forward_interactive()` byte-
relay loop `:shell` uses -- confirming the earlier prediction that this needed
no new terminal mechanics, only call-site plumbing (container selection,
readonly gate, the `Api::attach` call itself). Only offered when the
container's own Pod spec has both `stdin: true` and `tty: true` (Kubernetes
does not allocate a PTY otherwise); refused explicitly and immediately
pointing at `:logs` when not, rather than silently degrading to a read-only
view nobody asked for.

**A fourth real bug found and fixed, specific to attach:** attaching to an
already-running process that was never started expecting stdin at all (the
realistic common case -- it is not a shell waiting to execute typed input)
hung forever on the very first live test: Ctrl-D (which correctly ends a
`:shell` session, since a real shell explicitly reacts to it) did nothing,
and the forwarding loop's "keep draining remote output until it closes"
tail waited for output that would never stop, freezing the whole TUI. Fixed
by adding a local-only detach key, `Ctrl-]` (`0x1D`, the same byte telnet and
other terminal tools traditionally use for this), that ends the local
session unconditionally without ever signaling the remote -- confirmed live
that detaching this way leaves the attached container completely unaffected
(`kubectl get pod`/`logs` immediately after: still `Running`, 0 restarts,
output continuing). Matches the "cancellation ends local observation, never
remote rollback" distinction `docs/SESSIONS.md` already drew for background
sessions; this is the same distinction applied to a foreground one.

**Four real bugs found and fixed in total** (see journal for full root-cause writeups):
(1) the `--readonly` CLI flag and `Settings.readonly` existed but were completely
unwired -- nothing ever read `cli.readonly`, so read-only enforcement for any
future mutating capability was a no-op regardless of the flag; fixed via a
`ConnectOptions.force_readonly` hard override applied on every `resolve()`,
refreshed across reconnects, immune to a cluster/context config layer trying to
lift it. (2) `command::parse()`'s whitespace-slash filter-boundary heuristic
(`resource / filter`) fired on ANY `/` preceded by whitespace anywhere in a
command string, including inside `:exec`'s/`:shell`'s own arguments -- `:exec
worker -- /bin/sh` silently lost its command to a misparsed "filter", found live
on the very first absolute-path test. Fixed by skipping that heuristic entirely
when the command name is `exec` or `shell` (both have their own `--` separator
and never use the `/` filter suffix). (3) `Terminal::clear()` (used to force a
full repaint after resuming from a shell) internally queries the backend's
cursor position via a DSR escape sequence and a blocking stdin read with a
2-second timeout -- this raced the just-ended interactive session's own stdin
reader and reliably timed out, crashing the whole app with "The cursor position
could not be read within a normal duration" on the very first live shell-exit
test. Root-caused by reading `ratatui-core`'s source directly (traced
`Terminal::clear()` → `get_cursor_position()` → `crossterm::cursor::position()`).
Fixed by calling `Terminal::resize()` with the unchanged current size instead --
for a `Viewport::Fullscreen` terminal this clears via `clear_region(ClearType::
All)` with no cursor query at all, confirmed by reading that code path too.

**One known, bounded, documented limitation, not a bug fixed to zero:** an
orphaned `tokio::io::Stdin` read (a documented Unix tokio limitation -- that
read cannot be cancelled) can occasionally consume an entire subsequently-typed
command right after a shell session ends via the *other* side of its forwarding
loop's `select!` winning the race (observed after both typed `exit` and a local
Ctrl-D, so not reliably tied to which side initiates the close). Root-caused via
live byte-level tracing of every stdin read and every crossterm event across
repeated round-trips. Mitigated (`run()`'s `suppress_phantom_enter`) to a safe
no-op requiring one retry, rather than the wrong action it caused before the fix
(a stray trailing `Enter` opening the wrong document). Does not affect terminal
restoration, which was confirmed correct (`stty` unchanged) in every tested
ending regardless. Fully closing this needs a cancellation-safe stdin reader
(`tokio::io::unix::AsyncFd` over a non-blocking fd) -- future work. See
`docs/EXEC.md` for the full writeup.

**Live-accepted against `kind-sauron-test`, fresh binary, exactly the ordered
adversarial list requested:** (1) normal shell → `exit`; (2) an explicit
nonexistent shell path, failing cleanly with the real API error; (3) the target
Pod force-deleted mid-session (`docker ... delete --force --grace-period=0`),
surfacing `Shell Failed` with exit code 137 (SIGKILL) rather than a hang; (4) the
whole cluster's Docker network disconnected mid-session, ending the session and
degrading the background watch to a clear transport-error status that fully
recovered on `Refresh` after reconnecting; (5) repeated local resize tracked
correctly via `stty size` inside the remote shell across four distinct sizes;
(6) Ctrl-C forwarded to the remote (interrupting a `sleep 60`, session staying
alive) rather than quitting locally; (7) Ctrl-D cleanly ending the session;
(8) eight consecutive open/use/close round-trips, exercising and confirming the
limitation above and its safe-retry behavior rather than any wrong action; (9) a
final quit from SAURON afterward with exact `stty` state preserved. No production
calls -- exercised only against the isolated test cluster.

**Attach live-accepted against `kind-sauron-test`, fresh binary:** explicit
rejection of a container without `stdin: true, tty: true` with zero connection
attempts; real live output from an already-running process (worker container's
own `while true; do echo; sleep 1; done` loop, distinct from exec -- no new
process started); the `Ctrl-]` local detach leaving that container running and
completely unaffected (checked via `kubectl get pod`/`logs` immediately after);
the target Pod force-deleted mid-session ending the attach cleanly rather than
hanging; a final quit from SAURON with exact `stty` state preserved. No
production calls.

## M4.3 acceptance

Explicit/auto ports, conflicts, real TCP request, navigate/ns/context/logs while active,
stop, target deletion/replacement, bounded simultaneous forwards, repeated start/stop,
shutdown and OS listener cleanup. No implicit retry onto a replacement UID or context.

Accepted 2026-09-16 against explicit `.test-cluster/config`, verified Docker endpoint,
`kind-sauron-test` and same-cluster readonly alias `kind-sauron-test-b`. Real HTTP response
`M4_HTTP_OK` verified through each tunnel, not just a successfully bound socket.

| Case | Observed evidence |
| --- | --- |
| Declared picker / automatic / explicit | Actual HTTP; automatic nonzero loopback port; requested port exactly matched |
| Conflict / concurrency limits | Occupied port visibly fails without disturbing owner; four forwards work; fifth refused; eight clients allowed, ninth rejected and counted |
| Background identity | Resource, namespace, readonly context and logs changes preserve original tunnel; manager retains origin/UID |
| Individual stop / repeated cycles | Others continue; 12 request/start/stop cycles; warm fd and thread counts return to baseline in complete runs |
| Startup cancellation | Five no-readiness-pause start/stop cycles converge to zero active |
| Target deletion / recreation | Listener and open client close; old port remains closed after new UID appears; fresh selection explicitly starts another session |
| 32x9 / app exit | Narrow manager usable; exact pre/post stty equality; every recorded local port refuses connection after quit |
| Policy | Default/CLI readonly denial; reload cannot undo CLI override for forward/exec/shell/attach; original-context revocation unit-tested |
| Fault semantics | Fake HTTP 403/404/replacement/terminating/terminal phase before bind; monitor replacement/403 after bind closes listener |

Two initial assertion failures were isolated harness bugs, not app failures: an old
Listening row satisfied the new-start wait, and a retained-history outcome was below
the viewport. Corrected with SessionId-aware readiness and scrolling; exact full flows
repeated successfully. Review found a real pre-existing readonly-on-reload omission,
fixed with unit regression and live readonly → reload → denied actions.

Regression after M4.3: live M3 filter/sort harnesses, M4.0 lifecycle/log harness and
opt-in cluster inspection test pass. This does not replace the full M4.4 matrix.
Short debug-profile resource samples: RSS 29904→29904 KiB, fds 15→15, threads 4→4;
another pass RSS 29904→29860 KiB, fds 14→15, threads 4→4. Not a soak or marketing claim.
Run `python3 scripts/accept-m4-forward.py`; `--policy-only` repeats just the CLI/reload
denial flow. No production calls, published artifacts or m4-accepted tag.

## M4.4 combined and regression acceptance

Logs → namespace → logs → context → history; multi-source → search/filter → pause →
delete → resume; forward → navigate/context/logs → stop; exec startup → cancel → exec;
shell → repeated resize → remote exit → palette; forward/logs → same-name replacement;
rapid context/resource/ns/log changes; manager rapid start/stop; 32x9 logs/sessions/help;
shutdown during logs/forward/manager. Observe no task/listener/terminal/scope leaks.

Recheck context/ns pickers, CRDs/alias ambiguity, history, palette, typed filter/sort,
documents, Events toggle, printer columns, Refresh and narrow terminal. Record soak
duration/RSS/CPU/session counts/reconnects honestly (target 1–2 hours when practical).
Do not create m4-accepted until combined live acceptance and documentation reconcile.

## Execution log

- M4.2b (attach) accepted: extracted the shared `forward_interactive()` byte-
  relay loop out of `interactive()` (shell) so `attach()` could reuse it
  verbatim against `Api::attach` instead of `Api::exec` -- exactly the
  "call-site plumbing, not new mechanics" the ledger predicted for this slice.
  Gated on the container's own Pod spec advertising `stdin: true, tty: true`
  (Kubernetes allocates no PTY otherwise); refused explicitly rather than
  degrading to a silent read-only view. Found and fixed a real hang on the
  very first live test: attaching to a process that never reads stdin (the
  realistic case) left the forwarding loop waiting forever for output that
  would never stop, since Ctrl-D -- which correctly ends a real shell -- does
  nothing to a process that isn't listening for it. Fixed with a local-only
  detach key, `Ctrl-]` (0x1D), that ends the local session unconditionally
  without signaling the remote; confirmed live via `kubectl get pod`/`logs`
  immediately after detaching that the attached container kept running,
  completely unaffected. Live-accepted against `kind-sauron-test`, fresh
  binary: explicit rejection of a non-interactive container, real live output
  from an already-running process, the `Ctrl-]` detach, the target Pod force-
  deleted mid-session ending cleanly rather than hanging, and a final quit
  with exact `stty` state preserved. 72 unit + 11 fake HTTP tests passing
  throughout; fmt/check/clippy clean. No production calls.

- M4.2 (interactive shell) accepted, in the requested order: designed
  `app::terminal::TerminalHandoff` first; live-verified its restore behavior
  across every ending (clean exit, nonexistent shell, Pod force-deleted,
  network disconnected, repeated resize, Ctrl-C, Ctrl-D, eight repeated round-
  trips, final quit) *before* calling the feature done; only then built the
  interactive shell command on top. Found and fixed a crash on the very first
  live shell-exit test: `Terminal::clear()` internally queries the backend's
  cursor position (a DSR escape + blocking stdin read with a 2s timeout),
  which raced the just-ended session's own stdin reader and reliably timed
  out. Root-caused by reading `ratatui-core`'s source directly; fixed by
  calling `Terminal::resize()` with the unchanged size instead (clears without
  any cursor query for a Fullscreen viewport). Found and fixed a second,
  genuinely reproducible bug via live byte-level tracing across repeated
  round-trips: an orphaned `tokio::io::Stdin` read (uncancellable on Unix, a
  documented tokio limitation) can consume an entire subsequently-typed
  command right after a session ends, surfacing as a stray trailing `Enter`
  that opened the wrong document -- traced with temporary logging of every
  key event and every raw stdin/stdout byte, confirmed independent of tmux
  (checked at the raw byte level) and independent of timing (reproduced with
  robust condition-polling, not fixed sleeps). Mitigated to a safe no-op/retry
  rather than the wrong action; documented as a bounded, not-fully-closed
  limitation rather than claimed fixed. Extracted a shared `check_pod_uid`
  helper into `kube::mod` (was about to be duplicated a third time). Full
  suite green throughout (70 unit + 11 fake HTTP); fresh binary rebuilt before
  every live pass. No production calls.

- M4.2 (one-shot exec) accepted: researched `kube` 4.2's exec/attach/portforward
  surface directly from the vendored source (all on the already-enabled `ws`
  feature, no new dependency) before writing any transport code, per the plan.
  Wired the previously-dead `--readonly` CLI flag through a new hard-override
  field on `ConnectOptions`, applied on every settings resolve so a cluster/context
  config layer can never quietly lift it back on. New `kube::exec` mirrors
  `kube::logs`'s session-owned, UID-pinned pattern for a one-shot, non-interactive
  command. Live against `kind-sauron-test` with a fresh binary: denied under
  default readonly with zero connection attempts; `--readonly` forcing denial even
  against a `readonly = false` config; explicit container success with real
  stdout+stderr capture; unknown/missing container errors; a genuine
  nonexistent-executable API failure; non-zero exit code surfaced with its status;
  Pod delete+recreate correctly clearing the stale selection rather than exec'ing
  into the replacement; exec succeeding again after reselecting the fresh UID;
  Refresh restarting under a new identity; 32x9 clipping; exact `stty` terminal
  state preserved end to end (open exec, run, escape, quit). Found and fixed two
  real bugs along the way (readonly being unwired; the `/`-filter-boundary/argv
  collision) -- both detailed above and in HANDBOOK. 69 unit + 11 fake HTTP tests
  passing, including 4 new (2 exec container-selection, 1 readonly-gate-ordering
  regression, 1 command-grammar regression). Interactive shell/attach deliberately
  left researched-not-built this pass -- see the M4.2/M4.2b section above.

- M4.1 accepted: full fmt/check/clippy/test green (65 unit + 11 fake HTTP), fresh
  `cargo build --locked`, `python3 scripts/accept-m4.py logs` PASS twice in a row
  against `m4-fixtures`: explicit container choice (bare `:logs` on a multi-container
  Pod correctly refuses and names the choice), init container (`setup`, runs to
  completion, `Ended`), `:logs *` (worker+web together, source-tagged lines),
  pause (display freezes, bounded ingestion continues), search+matching-line
  filter narrowing to one source, clear, Refresh restarting with fresh
  request/session identity, `:logs_visible` across two different Pods,
  a high-volume burst Pod hitting the bounded-eviction `PARTIAL: viewer limit`
  notice, 32x9 resize, same-name Pod recreation (`m4-recreate`) correctly ending
  the old session and `Refresh` reporting "replaced" rather than silently
  retargeting, ANSI/control-sequence sanitization on a deliberately unsafe line,
  and an ephemeral container (`observer`) added after the Pod was already open.
  No production calls. Two test-harness bugs found and fixed during this pass
  (both in `scripts/accept-m4.py`/`scripts/test-cluster.sh`, not the application
  -- see journal): a stale total-row-count assertion from before the fixture
  namespace had accumulated 6 Pods, and a fixed `sleep(1)` racing an ephemeral
  container's actual startup instead of polling its real status.

- M4.0 accepted: all four checks green, fresh build, accept-m4.py PASS against restored
  kind; exact palette-loss reproduction passed three times, follow/previous/explicit
  container, overlay, rapid cancellation/scope changes, 32x9, zero sessions after close,
  shutdown with logs and exact stty comparison passed. Harness waits two render samples
  to avoid mistaking an outgoing frame for newly ready scope. No production calls.

- M4.0 first PTY pass found palette loss during context completion; minimal live
  `ctx B → : → 100ms → info` opened namespace picker. Runtime regression added;
  preserve current input while resetting abandoned view. Full suite/replay pending.

- 2026-09-16: audited M3 baseline, reconciled docs, identified missing isolated kind
  container. Full baseline suite passed; no M4 implementation/live claims yet.
