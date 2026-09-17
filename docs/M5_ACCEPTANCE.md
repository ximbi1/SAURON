# M5 evidence-driven inspection acceptance ledger

Starting checkpoint: local annotated `m4-accepted`, `1ea0016`; M1–M4 accepted.
The recorded M4 baseline is 76 unit + 14 fake HTTP tests, full regression and a
75-minute soak. This ledger records new M5 evidence, not a rerun of that audit.
Production remains read-only; fixture writes require explicit `.test-cluster/config`,
`kind-sauron-test` and `scripts/test-cluster.sh` identity verification.

## Slices and verdicts

| Slice | Contract | Implementation | Unit / fake API coverage | Live evidence | Bugs / gaps | Verdict |
| --- | --- | --- | --- | --- | --- | --- |
| M5.0 | Explicit known/unknown/partial, origin, identity and freshness; no invented defaults | IMPLEMENTED | 2 unit tests direct; proven through the M5.1 collector as its only consumer so far | Same live evidence as M5.1 (Evidence/Unknown/Observation carry every sample end to end) | Primitives only proven through one consumer; M5.2-M5.5 must each exercise them independently before this is closed | ACCEPTED (as a foundation; re-verify per consumer) |
| M5.1 | Optional bounded Pod/container/Node Metrics API polling, owned and scope-gated | IMPLEMENTED | 83 unit + 17 fake HTTP green; decode/403/404/500/timeout/bounds/correlation tests | Real metrics-server v0.8.1 live: Pod+Node CPU/memory, ns/ctx round-trip, 32x9, `Ctrl-C` shutdown with exact `stty` restore | Values currently inspectable only via `:info`; table/filter/sort integration is M5.2; live metrics-absent PTY mode retired once the fixture became permanent (fake-HTTP covers it instead, see journal) | ACCEPTED (transport only; not table-integrated) |
| M5.2 | Requests/limits/capacity/QoS; typed metric table/filter/sort integration | DESIGNED | Pending | Pending | Zero/missing denominator stays unknown | NOT ACCEPTED |
| M5.3 | Pure deterministic Pod/workload/Node/storage health with evidence | DESIGNED | Existing basic rules are not M5 acceptance | Pending | Missing controller/container evidence must not imply health | NOT ACCEPTED |
| M5.4 | Fresh UID-pinned Explain; bounded child/Event/metric evidence; independent source failures | DESIGNED | Existing Explain is not M5 acceptance | Pending | No graph, speculative cause or name-prefix correlation | NOT ACCEPTED |
| M5.5 | Bounded UID-scoped meaningful watch/relist transitions | DESIGNED | Existing history is not M5 acceptance | Pending | Equivalent relist must not invent transitions | NOT ACCEPTED |
| M5.6 | Combined adversarial flows, M1–M4 regression and API/performance sanity | NOT STARTED | Pending | Pending | No m5-accepted until demonstrated | NOT ACCEPTED |

## M5.0 — shared evidence semantics

Known values retain origin, source observation time when available, and local receipt
time. Unknown reasons distinguish unavailable, forbidden, not found, not reported,
stale, unsupported, partial, target replaced, malformed, timeout and transport failure.
Partial evidence retains usable observations but explicitly reports missing coverage.
Provenance is bounded and cannot contain arbitrary API bodies/credentials. Test source
age and local expiry, missing/future timestamps, missing values and replacement identity.

## M5.1 — collector

Fake HTTP: Pod/container/Node success; 404; 403; malformed/partial sample; timeout;
cancellation while requesting/sending/sleeping; old epoch/context result; same-name/new
UID; stale recovery. Collector is view-owned, never a port-forward session, never one
request per row/render. API errors must not replace object-watch errors or clear rows.
Pin metrics without UID to verified object incarnations; reject ambiguous correlation.
Live: reproducible isolated metrics-server if practical; demonstrate actual source
timestamps and non-fabricated usage. Fake transport alone is not live acceptance.

**Accepted 2026-09-17** against `kind-sauron-test` with metrics-server v0.8.1 pinned
via `scripts/test-cluster.sh m5-metrics-install` (Apache-2.0, unmodified upstream
manifest plus a fixture-only `--kubelet-insecure-tls` patch; SAURON's own API TLS
validation is unchanged). `python3 scripts/accept-m5.py`, fresh binary: real Pod CPU/
memory sample with a genuine source timestamp (not the receipt time) shown via `:info`;
`:ns`/`:ctx` round-trip preserving the collector; `v1/nodes` switch producing a real
Node sample; 32x9 rendering the diagnostics document without corruption; `Ctrl-C` quit
with exact `stty` state restored. No production calls. The originally-planned
`metrics-absent` live PTY mode was retired after `m5-metrics-install` left
metrics-server permanently on the only isolated live context (`kind-sauron-test` and
its alias `kind-sauron-test-b` share the one physical kind cluster, so there is no
live context left without it); the absent/forbidden/timeout/malformed paths remain
covered by the three fake-HTTP transport tests instead, and the earlier single
metrics-absent PTY pass recorded before the fixture existed stands as historical
evidence, not a repeatable mode.

## M5.2 — accounting and query integration

Unit: shared quantity parser, finite units, CPU cores/memory bytes; regular/init/sidecar
accounting; missing/zero request or limit; Node capacity/allocatable; justified QoS.
Live: known usage displayed, missing usage UNKNOWN, Strong Kleene unknown-excluded,
stable typed known/unknown sorting; recreation, context/namespace, Refresh, history,
32x9. Metrics refresh/expiry invalidates row memoization without disturbing UID selection.

## M5.3 — deterministic health

Precedence fixtures: deletion; phase; init success/failure/restartable sidecars;
CrashLoopBackOff/ImagePullBackOff/ErrImagePull/OOMKilled; readiness gates and scheduling;
successful Pod/Job; observedGeneration lag; Deployment replica mismatch; StatefulSet
partition/OnDelete/revisions; DaemonSet rollout; Job failure/completion; Node condition
polarity/Unknown; PVC/PV phase. Each finding cites actual fields; absent data is not healthy.
Live fixtures must be bounded and reproducible, without disrupting the cluster's Node.

## M5.4 — Explain 2.0

Reuse existing fresh-object GET, UID validation, Event timestamp/cap semantics and
document viewer. Collect bounded direct ownership-verified children, not loose names.
Preserve successful evidence if Events/metrics/children fail; label forbidden/partial.
Metrics describe usage, not an inferred cause such as throttling. Structured findings
retain canonical target, field path, observation time and source; no confidence percent.
Live: healthy/CrashLoop/image-pull Pod; progressing/unavailable Deployment; completed/
failed Job; metrics and Events present/absent; partial RBAC where practical; UID
replacement; rapid repeated reports; context switch mid-collection; 32x9.

## M5.5 — Timeline

Session-only watch observations, not Events or audit logs. UID + connection scope
separates incarnations. Record meaningful field deltas, not resourceVersion churn or
metrics polls. Target bounds: 64 entries/object, 256 objects, deterministic eviction.
Tests/live: phase/reasons/restarts/conditions/generation/replicas/images/deletion;
same-name replacement; unchanged relist emits nothing; changed relist emits only the
observed delta; rapid updates; bounds; context/history; 32x9; process restart clears it.

## M5.6 — combined acceptance

1. Metrics → CPU filter → memory sort → namespace → back; exact known/unknown semantics.
2. CrashLoop → Explain → Events → logs → back; consistent evidence-based health.
3. Deployment rollout → Progressing → Explain → completion → meaningful Timeline delta.
4. Metrics API disappears → stale/unknown, never zero.
5. Metrics recover → fresh samples, no cross-context or duplicate stale values.
6. Explain → immediate context switch; old report rejected.
7. Explain → same-name/new-UID replacement; old report rejected.
8. Watch reconnect/relist; no false Timeline transitions.
9. RBAC partial evidence; Explain remains useful and explicit about missing sources.
10. 32x9 metrics/health/Explain/Timeline transitions; no corruption.
11. M4 forward active during M5 navigation/inspection; original tunnel still works.
12. Quit with collector active; owned tasks and process exit cleanly.

Before tagging: context/namespace pickers, aliases/CRDs, history, palette, filters/sort,
documents, Events, CRD columns, logs, safe kind exec/shell/attach/forward smoke, narrow
terminal and readonly regressions. Measure poll requests, bounded Explain fanout,
CPU/RSS/queue pressure and Timeline memory; no comparative performance claims.

## Bug and verification discipline

For a real live failure: reproduce → classify app/harness/environment → root cause →
regression → full locked fmt/check/clippy/test → rebuild → exact live replay, then resume.
Each slice needs recorded unit/fake/live evidence and limitations before ACCEPTED.
Create local annotated `m5-accepted` only after every slice and combined acceptance;
never push/publish. No M6 graph or M7 mutation-policy expansion.

## Execution journal

- 2026-09-17: ledger created; M5.0 implementation starting. No M5 acceptance claimed.
- M5.0/M5.1: evidence primitives and owned bounded collector implemented; first suite
  81 unit green, 16/17 fake HTTP green. Corrected one fake-API expectation: kube's
  503 retry middleware reaches the collector deadline (TimedOut), unlike direct 500.
  Real absent-API PTY passed without clearing resource rows. Pinned metrics-server
  installation is isolated-kind-only; full M5.1 acceptance still pending.
- M5.0/M5.1 accepted (transport-only): reinstalled/verified the pinned metrics-server
  v0.8.1 fixture against `kind-sauron-test` (`kubectl top nodes`/`top pods` returned
  real, non-fabricated samples before touching SAURON at all), rebuilt, then ran
  `python3 scripts/accept-m5.py` live: genuine Pod/Node CPU/memory with a real source
  timestamp distinct from receipt time, `:ns`/`:ctx` round-trip, `v1/nodes` switch,
  32x9, and a clean `Ctrl-C` quit with exact `stty` restoration. Full suite (83 unit +
  17 fake HTTP) green. One harness-design consequence found and fixed, not an app bug:
  `scripts/accept-m5.py`'s `metrics-absent` PTY mode became permanently untestable
  the moment `m5-metrics-install` made metrics-server a standing fixture on the only
  isolated live context available -- retired that mode from the script (kept the
  fake-HTTP absent/forbidden/malformed/timeout coverage, which does not depend on
  cluster fixture state) rather than leave a live mode that can only ever time out.
  M5.0 promoted to ACCEPTED as a foundation, proven through M5.1 as its first real
  consumer; still to be re-verified independently as each of M5.2-M5.5 becomes its
  own consumer. Next: M5.2 (requests/limits/QoS accounting, then table/filter/sort
  integration) -- metrics remain `:info`-only until that lands.
