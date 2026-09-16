# M4 interactive sessions acceptance ledger

Baseline 2026-09-16: annotated m3-accepted, 9eac329. Full fmt/check/clippy/test green:
55 unit + 10 fake HTTP; one opt-in cluster test ignored. Production untouched.
No M4 acceptance implied by M3 tests. Isolated kind was absent and has been recreated
via bootstrap-test-cluster.sh; node Ready/identity verified. Every fixture write uses test-cluster.sh.

| Slice | Intended contract | State | Coverage / live evidence | Gaps / verdict |
| --- | --- | --- | --- | --- |
| M4.0 | Owned identity, cancellation, startup/running/stopping/final outcomes; prove with logs | ACCEPTED | 62 unit + 11 fake HTTP; live accept-m4.py, opt-in cluster test | Protocol-specific operational sessions still subsequent slices |
| M4.1 | Bounded advanced logs, explicit container/source, controls, UID and scope integrity | RESEARCHED | Existing basic logs accepted in M1; new evidence pending | NOT ACCEPTED |
| M4.2 | Native structured exec/shell, readonly boundary, terminal handoff/restore | RESEARCHED | Locked kube-client 4.2.0 inspected | NOT ACCEPTED |
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
