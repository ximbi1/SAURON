# M4 interactive sessions acceptance ledger

Baseline 2026-09-16: annotated m3-accepted, 9eac329. Full fmt/check/clippy/test green:
55 unit + 10 fake HTTP; one opt-in cluster test ignored. Production untouched.
No M4 acceptance implied by M3 tests. Isolated kind was absent and has been recreated
via bootstrap-test-cluster.sh; node Ready/identity verified. Every fixture write uses test-cluster.sh.

| Slice | Intended contract | State | Coverage / live evidence | Gaps / verdict |
| --- | --- | --- | --- | --- |
| M4.0 | Owned identity, cancellation, startup/running/stopping/final outcomes; prove with logs | ACCEPTED | 62 unit + 11 fake HTTP; live accept-m4.py, opt-in cluster test | Protocol-specific operational sessions still subsequent slices |
| M4.1 | Bounded advanced logs, explicit container/source, controls, UID and scope integrity | ACCEPTED | 65 unit + 11 fake HTTP; live accept-m4.py logs, twice | Aggregation scope is explicit visible-Pods/all-containers only; see LOGS.md |
| M4.2 | Native structured exec/shell, readonly boundary, terminal handoff/restore | PARTIAL | 69 unit + 11 fake HTTP; live one-shot exec, twice-verified readonly gate | One-shot exec ACCEPTED; interactive TTY shell NOT YET BUILT (needs terminal handoff) |
| M4.2b | Separate native attach semantics or justified deferral | RESEARCHED | Api::attach exists; terminal/fixture acceptance pending | NOT ACCEPTED |
| M4.3 | Loopback background forwarding, manager, durable identity, bounded connections | RESEARCHED | Native API inspected; no implementation | NOT ACCEPTED |
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

**Interactive TTY shell: NOT YET BUILT**, and honestly recorded as such rather than
faked. Researched: `kube` 4.2's `Api::exec`/`Api::attach` + `AttachedProcess` (native
WebSocket, `ws` feature already enabled, no SPDY, no new dependency) fully support
stdin/stdout/TTY and a resize channel. What's missing is a *safe* terminal-handoff
guard: Ratatui's `crossterm::event::EventStream` reads the same stdin the interactive
session would need raw, unshared access to, and this was not attempted blind without
a plan to rule out that contention. Next M4.2 sub-slice, not this one.

Attach (`Api::attach`, distinct from exec: attaches to an already-running process
rather than starting a new one) is likewise researched-only, deferred alongside
interactive shell since it needs the identical terminal-handoff dependency.

**Two real bugs found and fixed while building/testing one-shot exec** (see journal
for full root-cause writeups): (1) the `--readonly` CLI flag and `Settings.readonly`
existed but were completely unwired -- nothing ever read `cli.readonly`, so read-only
enforcement for any future mutating capability was a no-op regardless of the flag;
fixed via a `ConnectOptions.force_readonly` hard override applied on every
`resolve()`, refreshed across reconnects, immune to a cluster/context config layer
trying to lift it. (2) `command::parse()`'s whitespace-slash filter-boundary
heuristic (`resource / filter`) fired on ANY `/` preceded by whitespace anywhere in
a command string, including inside `:exec`'s own argv -- `:exec worker --
/bin/sh` silently lost its command to a misparsed "filter", found live on the very
first absolute-path test. Fixed by skipping that heuristic entirely when the
command name is `exec` (which has its own `--` separator and never uses the `/`
filter suffix).

## M4.3 acceptance

Explicit/auto ports, conflicts, real TCP request, navigate/ns/context/logs while active,
stop, target deletion/replacement, bounded simultaneous forwards, repeated start/stop,
shutdown and OS listener cleanup. No implicit retry onto a replacement UID or context.

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
