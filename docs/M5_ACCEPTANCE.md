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
| M5.2a | Requests/limits/capacity/QoS accounting, table/filter/sort integration | IMPLEMENTED | 4 unit tests (init-container formula, missing-vs-malformed, QoS passthrough); live table/filter/sort | Live `v1/pods -w` shows real CPU/R, MEM/R, CPU/L, MEM/L, QOS; `Node -w` shows CPU/C, MEM/C; typed filter (`cpu/r>1m`) and sort (`mem/l`) verified against real fixture data | Wide-only columns to avoid overwhelming narrow terminals; QoS read from `status.qosClass`, never re-derived | ACCEPTED |
| M5.2b | Live Metrics API usage in table/filter/sort/percentages | IMPLEMENTED | 88 unit + 17 fake HTTP green; zero-denominator/unknown-usage percentage test | Live `CPU`/`MEM` columns (real usage, unknown shows `-`); `cpu>10m` filter correctly excludes 3 unknowns; typed sort by `cpu/%r` orders known ascending then unknown-last | `Metrics` trait (`resources::mod.rs`) threaded through `Field`/`Expr`/`sort::rows` so filters keeps no upward dependency on `app`; percentages use a real zero-denominator check, never fabricated 0%/∞ | ACCEPTED |
| M5.3 | Pure deterministic Pod/workload/Node/storage health with evidence | IMPLEMENTED | 95 unit + 17 fake HTTP green; 5 new precedence tests (10 total in the health module) | Live against `kind-sauron-test`: CrashLoopBackOff/ImagePullBackOff/Unschedulable/OOMKilled/NotReady Pods, Unavailable/Ready Deployment, Progressing StatefulSet, Ready DaemonSet, Completed/Failed Job, Pending PVC, Ready Node -- 13 real cases, all correct | `Health` now carries bounded `evidence: Vec<String>` citing real fields; `ContainerCreating`/`PodInitializing` no longer misclassified as failures; `lastState.terminated` (e.g. OOMKilled after restart) surfaces even when current state is a waiting reason | ACCEPTED |
| M5.4 | Fresh UID-pinned Explain; bounded child/Event/metric evidence; independent source failures | IMPLEMENTED | 98 unit + 19 fake HTTP green; 4 new explain tests, 2 new fake-HTTP ownership/partial tests | Live against `kind-sauron-test`: Deployment (2-hop, real ImagePullBackOff evidence on 2 owned Pods), Job (1-hop, real exit-code evidence + Warning Event), healthy Pod (real metrics, honest "not proven healthy" disclaimer) | `Health.evidence` reused verbatim, no re-derivation; ownership verified via `ownerReferences` UID match only; metrics gated to Pod/Node; a purely-descriptive finding never suppresses the no-fault disclaimer | ACCEPTED |
| M5.5 | Bounded UID-scoped meaningful watch/relist transitions | IMPLEMENTED | 105 unit + 19 fake HTTP green; 4 new relist-diff tests, 4 new curated-field-diff tests | Live against `kind-sauron-test`: real phase/reason/restart-count transitions on a recreated Pod (new UID), newest-first with `[watch]` source tags, 32x9 clean, no-op Refresh, context-switch isolation confirmed | Relist previously replaced `objects` wholesale with zero diffing -- real changes during a disconnect were silently dropped, not just correctly-suppressed false ones; fixed with a shared `diff_and_record` used by both the incremental and relist paths | ACCEPTED |
| M5.6 | Combined adversarial flows, M1–M4 regression and API/performance sanity | IMPLEMENTED | 105 unit + 19 fake HTTP green throughout | Live: all 12 combined sequences (`accept-m5-combined.py`, run twice), full M1-M4 regression, 75-minute soak (1800 cycles, 0 failures) | RBAC scenario needed a temporary limited Role/ServiceAccount/token (cleaned up); rollout-Progressing window sometimes too fast to catch on a 1-node kind cluster (noted, not a defect) | ACCEPTED |

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

**M5.2a (accounting) accepted 2026-09-17.** New `src/resources/accounting.rs`:
`pod_effective(pod, "requests"|"limits", cpu)` implements Kubernetes' own documented
init-container effective-resource formula (a restartable/"sidecar" init container adds
to every other container's total for the Pod's whole lifetime; a regular sequential
init container's own request/limit is compared only against the running total at its
own position, never summed against other sequential inits) -- a container that omits
a request/limit contributes nothing to that resource's sum, matching `kubectl
describe`'s own convention, which is distinct from the *object* failing to report
anything. `qos_class()` reads `status.qosClass` verbatim rather than re-deriving it --
Kubernetes already computes and stores this, and re-deriving it would be exactly the
speculative accounting this milestone forbids. `node_amount()` reads capacity/
allocatable directly. New `CPU/R`/`MEM/R`/`CPU/L`/`MEM/L`/`QOS` Pod cells and `CPU/C`/
`MEM/C` Node cells, gated behind wide mode (`w`) alongside the existing `NODE` column
so the default narrow view is not overwhelmed. `Field::parse` gained `cpu/r`, `cpu/l`,
`cpu/c`, `mem/r`, `mem/l`, `mem/c` key aliases so the existing typed filter/sort engine
resolves them with no new grammar. 4 unit tests (init-container formula against the
documented example, missing-vs-malformed-quantity distinction, QoS/Node passthrough).
Live against `kind-sauron-test`: wide-mode table shows real `CPU/R`/`MEM/R`/`CPU/L`/
`MEM/L`/`QOS` for every fixture Pod (`BestEffort` correctly shows `0m`/`0` rather than
UNKNOWN, since "no container specified this resource" is itself known, real data);
typed filter `cpu/r>1m` correctly excludes all three `BestEffort` fixtures; `qos=
BestEffort` and `qos=Burstable` filters each select the correct disjoint 3-Pod subset;
typed sort by `mem/l` orders correctly. This is static, spec/status-only accounting --
no Metrics API usage is involved, so it works identically whether or not metrics-server
is installed.

One real regression found and fixed, affecting M1-M4 acceptance scripts, not new
application code: the M5.1 metrics-server fixture staying permanently installed made
the bare, unqualified `pods` resource name ambiguous with `metrics.k8s.io`'s own
`pods` plural on the shared isolated cluster -- exactly the ambiguity `docs/METRICS.md`
already flagged, but previously only checked against `accept-m5.py`. `scripts/
accept-m3.py`, `accept-m4.py`, `accept-m4-forward.py` and `scripts/soak-m4.py` all
launched with or switched to the bare `pods` plural and broke with `"pods" is
ambiguous across API groups`. Fixed by qualifying every resource-switching `pods`
reference in those four scripts to `v1/pods` (left the rendered-text `expect('pods
[...`) assertions alone, since the UI's own breadcrumb/header still displays the
plural unqualified regardless of how it was addressed). Re-ran all four live: `accept-
m3.py filters`/`sorting`, `accept-m4.py` (foundation)/`logs`, `accept-m4-forward.py`
(full, 7/7) all PASS again with a fresh binary. `accept-m3.py sorting` also hit one
unrelated, pre-existing, non-reproducing timing flake in its `configmaps` typed-sort
retry loop (nothing to do with `pods`/metrics); passed cleanly on immediate retry,
documented here rather than silently rerun.

**M5.2b (live Metrics API usage in table/filter/sort/percentages) accepted
2026-09-17.** `Field::read`/`compare` only took `&Object`; live usage lives in
`app::metrics::Cache`, addressed by UID, not on `Object` itself. Resolved via a new
`resources::Metrics` trait (`usage`/`percentage`), defined in `resources` (not
`filters` or `app`) so this crate's core resource/filter types gain no upward
dependency on the `app` layer; `app::metrics::Cache` implements it by delegating to
its existing inherent `amount`/`percentage` methods. `Field::read`/`compare`,
`Expr::evaluate`/`matches` and `sort::rows` now take `Option<&dyn Metrics>`,
threaded from `State::rebuild()`/`State::cell()` as `Some(&self.metrics)`; every
other call site (tests, printer-column reads) passes `None` and gets the prior
behavior unchanged. Bare `cpu`/`memory` keys (already reserved by `Field::parse`
before this milestone) now resolve to live sampled usage -- distinct from the
`cpu/r`/`cpu/l`/`cpu/a` accounting/allocatable keys from M5.2a, which stay
object-only. New `cpu/%r`, `mem/%r`, `cpu/%l`, `mem/%l` keys resolve usage-as-
percent-of-request/limit via `Cache::percentage`, which checks for a real zero
denominator explicitly (`Unknown::ZeroDenominator`) rather than fabricating 0% or
dividing into infinity. New default-visible `CPU`/`MEM` table columns (Pod and
Node, gated on `kube::metrics::supported`) and new wide-only `CPU/%R`/`MEM/%R`/
`CPU/%L`/`MEM/%L` columns (Pod only -- a Node has no request/limit concept).

New unit test: usage 250m against a 500m request computes exactly 50%; the same
Pod with no limit anywhere gets `ZeroDenominator` for the limit percentage, not a
fabricated value; a `Forbidden` metrics status propagates through unchanged. 89
unit + 17 fake HTTP green throughout.

Live-accepted against `kind-sauron-test`, wide mode: `CPU`/`MEM` show real usage
per Pod, `-` for the three `BestEffort` fixtures currently reporting no sample
(not a fabricated 0); `cpu>10m` correctly matches 1 Pod and excludes exactly the
3 unknown ones (`?3` in the header, Strong Kleene unknown-excluded); typed sort by
`cpu/%r` orders the 3 known Pods ascending by real percentage and places all 3
unknowns last, in both directions. `m4-logburst`'s intentionally CPU-heavy fixture
correctly shows a percentage over 100% (real usage exceeding its small declared
request) -- exactly the kind of signal this feature exists to surface, not
clamped or hidden. Full M1-M4 regression (`accept-m3.py filters`/`sorting`,
`accept-m4.py` foundation/`logs`) and the M5.1 live pass (`accept-m5.py`) all
re-ran clean afterward.

## M5.3 — deterministic health

Precedence fixtures: deletion; phase; init success/failure/restartable sidecars;
CrashLoopBackOff/ImagePullBackOff/ErrImagePull/OOMKilled; readiness gates and scheduling;
successful Pod/Job; observedGeneration lag; Deployment replica mismatch; StatefulSet
partition/OnDelete/revisions; DaemonSet rollout; Job failure/completion; Node condition
polarity/Unknown; PVC/PV phase. Each finding cites actual fields; absent data is not healthy.
Live fixtures must be bounded and reproducible, without disrupting the cluster's Node.

**Accepted 2026-09-17.** Full contract: [HEALTH.md](HEALTH.md). `resources::
health::Health` now carries a bounded `evidence: Vec<String>` alongside status/
severity, citing the actual fields/values that produced the result -- empty for
`Healthy`/`Unknown`, where there is nothing to explain. Two real gaps found and
fixed while implementing, both confirmed by a new unit test before touching the
logic further: (1) `ContainerCreating`/`PodInitializing` waiting reasons were
being treated as generic "failures" (Critical or Warning depending on the
reason string) by the same code path as real crashes -- separated into a
distinct "ordinary startup" case that returns no failure at all, so a container
that is merely still starting up can no longer preempt or be confused with a
genuine crash-loop elsewhere. (2) `OOMKilled` and other terminal reasons were
only read from `state.terminated`, never from `lastState.terminated` -- once a
container has already restarted into `waiting: CrashLoopBackOff`, the fact that
it was OOM-killed moments earlier was silently lost. Both are exactly the kind
of subtle "the formula looked right but Kubernetes does something else" failure
this milestone is expected to surface, not infrastructure bugs.

Live-accepted against `kind-sauron-test` with `bash scripts/test-cluster.sh
m5-health-fixtures` (isolated, new `tests/fixtures/m5-health.yaml`), fresh
binary, 13 real cases: `crashloop` → `CrashLoopBackOff`; `missing-image` →
`ImagePullBackOff`; `unschedulable` → `Unschedulable`; a new `oomkilled` fixture
(`yes | head -c 200000000` against a 20Mi memory limit) → confirmed a genuine
kernel OOM kill via `kubectl` first (`lastState.terminated.reason=OOMKilled`,
`exitCode=137`), then `CrashLoopBackOff` in SAURON with the OOM evidence
preserved; a 2-replica StatefulSet with a 90s readiness-probe delay →
`Progressing` (1/2) while one replica is still starting, with its own not-yet-
ready Pod showing `NotReady`; a DaemonSet → `Ready`; a `backoffLimit: 0` Job
that exits 0 → `Completed`, and one that exits 1 → `Failed`; a 2-replica
Deployment pointed at an unreachable image → `Unavailable`; the pre-existing
`healthy` Deployment → `Ready`; a PVC referencing a nonexistent storage class →
`Pending`; the real cluster Node, inspected read-only → `Ready` (no pressure
conditions tripped, confirming the healthy path and the "read-only Node
inspection only" boundary). 95 unit + 17 fake HTTP green; full M1-M4 and M5.1/
M5.2 regression re-ran clean afterward. Evidence strings are not yet surfaced in
any UI -- that is M5.4's job, which can now reuse these findings directly
instead of inventing its own.

## M5.4 — Explain 2.0

Reuse existing fresh-object GET, UID validation, Event timestamp/cap semantics and
document viewer. Collect bounded direct ownership-verified children, not loose names.
Preserve successful evidence if Events/metrics/children fail; label forbidden/partial.
Metrics describe usage, not an inferred cause such as throttling. Structured findings
retain canonical target, field path, observation time and source; no confidence percent.
Live: healthy/CrashLoop/image-pull Pod; progressing/unavailable Deployment; completed/
failed Job; metrics and Events present/absent; partial RBAC where practical; UID
replacement; rapid repeated reports; context switch mid-collection; 32x9.

**Accepted 2026-09-17.** Full contract: [EXPLAIN.md](EXPLAIN.md). Rewrote
`explain.rs` around the philosophy "Explain = health findings + fresh evidence
+ bounded correlation," not a second diagnosis: the top finding's evidence is
now `object.health.evidence.join("; ")` verbatim -- M5.3's own cited fields --
and the previous redundant re-scan of `containerStatuses`/`conditions` (which
would have duplicated M5.3's rules, exactly what was to be avoided) is gone.
Fresh-GET/UID-check, Events (M3 semantics, 200-cap) and request/epoch gating
were already correct and needed no changes.

New in this slice: a `resources::Metrics`-shaped snapshot -- actually just
`(Result<f64,Unknown>, Result<f64,Unknown>)` for CPU/memory, re-captured fresh
from the view's own collector in `refresh_document()` on every Refresh (never
baked into the reused `Document::Source`, so a later Refresh sees a newer
sample) -- shown as a `Severity::Healthy` descriptive finding for Pods/Nodes
only, never a claimed cause. New verified-ownership Pod correlation in
`kube::evidence::owned_children()`: StatefulSet/DaemonSet/ReplicaSet/Job own
Pods directly (`ownerReferences` UID match, one hop); Deployment does not (it
owns ReplicaSets, which own Pods), so that path is a genuine bounded two-hop
resolution, not a step toward the relationship graph M6 owns. Bounded to 200
listed / 50 owned, both explicit `PARTIAL EVIDENCE` when hit. A real design
bug found and fixed by a live check, not a unit test: the always-present
metrics finding for a healthy Pod made `out` never empty, silently suppressing
the honest "no failure evidence found -- this does not prove healthy"
disclaimer. Fixed with an explicit `has_fault` flag tracking only genuine
Warning+ findings (health, child health, Warning Events) -- purely descriptive
findings (metrics, "N owned Pods checked, none unhealthy") never set it.

4 new unit tests (health evidence reused verbatim; unhealthy child surfaces
while a healthy child alongside an unhealthy *workload* is still explicitly
noted as checked; metrics severity never signals fault) plus 2 new fake-HTTP
tests (`ownerReferences`-only correlation rejecting a same-namespace Pod with
no matching owner reference even though nothing else distinguishes it; a
Forbidden owned-Pods list still yields a usable partial report with the
workload's own health intact and no secret leakage). 98 unit + 19 fake HTTP
green.

Live-accepted against `kind-sauron-test`: a Deployment stuck past its
`progressDeadlineSeconds` (`Stalled`, real condition message) with two owned
Pods correctly resolved through the ReplicaSet hop, both showing real
`ImagePullBackOff` evidence including the actual registry-resolution error
text; a failing Job (`Failed`, backoff-limit message) with its one owned Pod
resolved directly (`Failed`, real exit-code-1 evidence) plus a correlated
`BackoffLimitExceeded` Warning Event; a healthy Pod showing genuine non-
fabricated CPU/memory usage *and*, after the `has_fault` fix, still correctly
showing the "not proven healthy" disclaimer alongside it. Full M1-M4 and
M5.1-M5.3 regression re-ran clean afterward.

## M5.5 — Timeline

Session-only watch observations, not Events or audit logs. UID + connection scope
separates incarnations. Record meaningful field deltas, not resourceVersion churn or
metrics polls. Target bounds: 64 entries/object, 256 objects, deterministic eviction.
Tests/live: phase/reasons/restarts/conditions/generation/replicas/images/deletion;
same-name replacement; unchanged relist emits nothing; changed relist emits only the
observed delta; rapid updates; bounds; context/history; 32x9; process restart clears it.

**Accepted 2026-09-17.** Full contract: [TIMELINE.md](TIMELINE.md). Most of the
UID-keyed storage, the 64/256 bounds, and the `:timeline`/`T` UI already
existed before M5 (built during M3-era store work) -- this slice's real job
was closing a correctness gap, not building from nothing.

New `resources/timeline.rs::meaningful_diff()`: a curated comparison (health
status, phase, generation/observedGeneration, replica-family fields,
deletion-timestamp transition, and for Pods restart totals/per-container
reason-by-name/image) shared by both the incremental watch path and the
relist path, replacing the old inline diff that only detected "`/status/
conditions` changed" without saying what changed.

**One real, load-bearing bug found and fixed, not a cosmetic gap:**
`Store::finish()` (the relist/reconnect path) previously replaced `objects`
with the freshly-staged set *with zero comparison against the prior state*.
This satisfied "an equivalent relist must not invent transitions" only by
accident -- it also silently dropped every *real* transition that happened
while disconnected, which the ledger's own acceptance list explicitly
requires ("changed relist emits only the observed delta"). Fixed with a
shared `diff_and_record()` called from both `apply()` (tagged
`Source::WatchObserved`) and the new relist pass in `finish()` (tagged
`Source::RelistObserved`), diffing every slot present before or after the
relist against its prior state before the wholesale replacement. Also
closes two related gaps the old code never handled during a relist
specifically: a same-slot UID replacement (recreated while disconnected) and
an object present before but absent after (deleted while disconnected) --
both now recorded against the *old* UID's timeline, matching the
already-correct behavior of the live incremental path.

8 new unit tests: 4 in `store.rs` (unchanged relist records nothing; a
changed-while-disconnected relist records exactly one delta tagged
`RelistObserved`; UID replacement and deletion are both observed through a
relist), 4 in `timeline.rs` (identical meaningful fields produce nothing;
phase/restart/container-reason changes are each cited by name; an image
change is cited by container name, not position; a deletion-timestamp
transition is one-directional). 105 unit + 19 fake HTTP green.

Live-accepted against `kind-sauron-test`: force-deleted the `crashloop`
fixture and let `scripts/test-cluster.sh fixtures` recreate it (new UID);
the new incarnation's own `:timeline` showed a clean, newest-first sequence
of real transitions -- `restarts: unknown → 0`, `ContainerCreating →
CrashLoopBackOff`/`Error` oscillation with exact timestamps, `restarts: 0 →
1 → 2 → 3`, `phase: Pending → Running` -- every entry correctly tagged
`[watch]`. 32x9 rendered cleanly; `Refresh` re-rendered with no network call
(same content, since nothing new had happened in that instant); switching
context created a fresh `Store` and therefore a correctly-empty timeline for
every UID in the new context, confirming no cross-context leakage (the
relist-diff unit tests cover the harder-to-trigger-live "changed while
disconnected" and "replaced/deleted via relist" paths directly, which is
consistent with this project's established pattern of using fake-transport/
unit coverage for scenarios a live watch reconnect can't be reliably forced
to reproduce on demand). Full M1-M4 and M5.1-M5.4 regression re-ran clean
afterward.

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

**Accepted 2026-09-17.** New `scripts/accept-m5-combined.py`, fresh binary,
live against `kind-sauron-test`, all 12 sequences plus full M1-M4 regression:

1. **Metrics → filter → sort → namespace → back.** `cpu>0` correctly excludes
   the not-running fixtures (`?N` unknown-excluded count), `sort mem/l`
   orders under wide mode, `:ns` round-trip preserves the collector.
2. **CrashLoop → Explain → Events → logs → back.** `crashloop`'s Explain
   cites its real waiting reason; Events opens; logs opens (this fixture's
   container is a one-shot `exit 1`, so `Ended` rather than `Streaming` --
   confirmed as the fixture's own expected behavior, not a bug); back to
   table intact throughout.
3. **Deployment rollout → Progressing → Explain → completion → Timeline.**
   `kubectl rollout restart deployment/healthy` triggered a real generation
   bump; on this single-node kind cluster reconciliation was fast enough
   that the `Progressing` frame was not always caught mid-flight (noted
   honestly rather than papered over), but the Deployment always
   reconverged to `Ready` and its `:timeline` showed a genuine
   generation/replica delta either way.
4. **Metrics API disappears → explicit UNKNOWN/Stale, never zero.** Scaled
   `metrics-server` to 0 replicas in `kube-system`; `CPU cores: UNKNOWN
   (Stale)` after TTL expiry, confirmed `CPU cores: 0` never appears anywhere
   in the pane.
5. **Metrics recover → fresh real samples, no cross-context bleed.** Scaled
   back to 1, waited for rollout; fresh non-UNKNOWN samples returned
   independently checked on both `kind-sauron-test` and its
   `kind-sauron-test-b` alias context.
6. **Explain → immediate context switch → old report rejected.** Opening
   Explain then switching context closes the document entirely (the
   existing generic document-lifecycle behavior, confirmed still correct
   for Explain specifically) -- the stale `WHY: Pod/healthy` text is gone,
   replaced by the new context's table.
7. **Explain → same-name/new-UID replacement → old report rejected.**
   Force-deleted and recreated `crashloop`; `Refresh` on the still-open
   Explain surfaced "Object was replaced" rather than showing stale content.
8. **Watch reconnect/relist → no false Timeline transitions.** Covered by
   the M5.5 unit tests (`store.rs`'s `unchanged_relist_records_nothing`/
   `changed_while_disconnected_relist_records_exactly_one_delta`), which
   exercise this deterministically; a live watch reconnect cannot be forced
   on demand against a healthy control plane without disrupting other
   in-flight tests, consistent with this project's established pattern for
   this exact class of scenario (see M5.5's own acceptance notes).
9. **RBAC partial evidence → Explain stays useful.** Created a short-lived
   `Role`/`RoleBinding`/`ServiceAccount` granting only `get`/`list`/`watch`
   on Pods and Deployments (no Events, no ReplicaSets), launched a second
   SAURON instance against a token-based kubeconfig for that identity:
   Explain on a Deployment showed `PARTIAL EVIDENCE` citing `Forbidden` for
   both Events and owned-ReplicaSet correlation, while still showing the
   Deployment's own health finding -- no secret leakage, all RBAC objects
   and the temporary kubeconfig cleaned up afterward.
10. **32x9 across metrics/health/Explain/Timeline.** All four render without
    corruption (column headers legitimately abbreviate at 32 columns, which
    is expected, not a defect).
11. **M4 forward alive throughout.** Started a real port-forward on
    `m4-sessions` before any M5 work began, then re-verified real HTTP
    connectivity through it after sequences 1, 3, 6/7, 10, and 4/5 -- the
    tunnel never dropped, confirming M5 work is fully unrelated to M4
    session ownership.
12. **Quit with the collector active.** Clean exit, exact `stty` restoration,
    confirmed via the same before/after comparison every prior milestone's
    scripts use.

Regression: `accept-m3.py filters`/`sorting`, `accept-m4.py`
(foundation)/`logs`, `accept-m4-forward.py` (full, 7/7), `accept-m5.py`
(M5.1 live) all rerun green with a fresh binary. Full suite (105 unit + 19
fake HTTP) green before and after. The combined script was run twice in a
row for reproducibility, both clean.

**Soak: 75 minutes (4499s), `kind-sauron-test`, fresh binary, operational
config.** `scripts/soak-m5.py` continuously cycled: namespace switch round-
trip, a metrics-supported resource switch, an Explain open/close on a real
object (fresh GET/UID check plus health/metrics evidence every time), a
Timeline open/close (local render, no network), and a `v1/nodes` round-trip
-- sampling RSS/fd/thread counts and the `:info` metrics-request counter
every cycle. 1800 cycles, 1800 Explain checks, 1800 Timeline checks, **zero
recoverable assertion failures across the entire run** -- no reconnects, no
flakiness, nothing to note as an incident. 3681 metrics-collector requests
total (`0 → 3681`), a steady cadence consistent with the 15-second poll
interval across repeated resource-view switches. RSS actually *decreased*
slightly over the run (31048 → 30556 KiB, well within noise -- not a leak in
either direction), fds held at 14-15 throughout, threads constant at 4.
Final check suite (`fmt`/`check`/`clippy`/`test`, 105 unit + 19 fake HTTP)
rerun clean after the soak.

## Bug and verification discipline

For a real live failure: reproduce → classify app/harness/environment → root cause →
regression → full locked fmt/check/clippy/test → rebuild → exact live replay, then resume.
Each slice needs recorded unit/fake/live evidence and limitations before ACCEPTED.
Create local annotated `m5-accepted` only after every slice and combined acceptance;
never push/publish. No M6 graph or M7 mutation-policy expansion.

## Execution journal

- M5.6 accepted, M5 fully closed: new `scripts/accept-m5-combined.py` ran
  all 12 combined sequences from this ledger live against `kind-sauron-test`
  with a fresh binary, twice in a row for reproducibility, both clean --
  metrics filter/sort/ns-switch; CrashLoop Explain/Events/logs round-trip; a
  real `kubectl rollout restart` observed through Progressing-or-fast-
  reconverge to Ready with a genuine Timeline delta either way; metrics-
  server scaled to 0 and back, confirming UNKNOWN/Stale never renders as a
  fabricated zero and fresh samples return independently on both context
  aliases afterward; Explain discarded correctly across both an immediate
  context switch and a same-name/new-UID replacement; a real temporary
  RBAC-limited identity (Role/ServiceAccount/token, fully cleaned up
  afterward) proving Explain stays usable with explicit `PARTIAL EVIDENCE`
  and zero secret leakage; 32x9 across metrics/health/Explain/Timeline; an
  M4 port-forward held alive and re-verified with real HTTP through the
  entire pass, confirming M5 work is fully unrelated to M4 session
  ownership; a clean quit with the collector active. Full M1-M4 regression
  (`accept-m3.py filters`/`sorting`, `accept-m4.py` foundation/`logs`,
  `accept-m4-forward.py` full 7/7, `accept-m5.py`) all rerun green.

  New `scripts/soak-m5.py`: 75 minutes, 1800 cycles rotating ns/ctx/resource
  scope with a real Explain and a real Timeline check every single cycle --
  zero recoverable assertion failures across the entire run, not one
  reconnect or flake. 3681 metrics-collector requests at a steady cadence;
  RSS actually *decreased* slightly (31048 → 30556 KiB, noise, not a leak
  in either direction); fds held 14-15; threads constant at 4. Full suite
  (105 unit + 19 fake HTTP) green before and after.

  M5.0 through M5.6 are now all ACCEPTED. Local annotated `m5-accepted`
  created at this commit, never pushed. SAURON has crossed the line this
  milestone set out to cross: it no longer just observes and operates
  Kubernetes -- it interprets state with cited evidence, and keeps
  session-local history without ever inventing it.

- M5.5 accepted: most of the UID-keyed history storage, 64/256 bounds, and
  `:timeline`/`T` UI already existed from M3-era store work -- this slice
  closed a real correctness gap rather than building from nothing. New
  `resources/timeline.rs::meaningful_diff()` (curated comparison: health,
  phase, generation/observedGeneration, replica-family fields, deletion-
  timestamp transition, Pod restart totals/per-container reason-by-name/
  image) replaces the old inline diff that only said "conditions changed"
  without saying what changed. Found and fixed a real, load-bearing bug:
  `Store::finish()`'s relist path replaced `objects` wholesale with zero
  comparison against prior state, which satisfied "no invented transitions"
  only by accident -- it also silently dropped every real transition that
  happened during a disconnect. Fixed with a shared `diff_and_record()`
  used by both the incremental (`WatchObserved`) and relist
  (`RelistObserved`) paths; also now correctly records same-slot UID
  replacement and deletion discovered via a relist, matching the live path's
  existing behavior. 8 new unit tests (unchanged relist → nothing; changed-
  while-disconnected relist → exactly one tagged delta; UID replacement/
  deletion via relist; curated field-diff coverage). 105 unit + 19 fake HTTP
  green. Live-accepted: force-deleted and recreated a fixture Pod, watched
  its fresh UID's own `:timeline` show a clean, newest-first, correctly-
  tagged sequence of real phase/reason/restart transitions; 32x9 clean;
  no-network `Refresh`; context-switch isolation confirmed (fresh `Store` →
  fresh empty timeline, no cross-context leakage). Full M1-M4 and M5.1-M5.4
  regression re-ran clean afterward. Full contract: `docs/TIMELINE.md`.
  M5.0-M5.5 are all now ACCEPTED. Next: M5.6 combined adversarial
  acceptance + full regression + soak, then `m5-accepted`.
- M5.4 accepted: rewrote `explain.rs` around "Explain = health findings +
  fresh evidence + bounded correlation," reusing `object.health.evidence`
  verbatim instead of re-scanning conditions/containers with duplicate logic.
  Added a metrics snapshot (Pod/Node only, descriptive `Severity::Healthy`,
  re-captured fresh every Refresh) and verified-ownership workload → Pod
  correlation (`ownerReferences` UID match; Deployment's genuine two-hop via
  ReplicaSet; 200/50 bounds, explicit PARTIAL on truncation). Fresh-GET/UID-
  check, Events (M3 semantics) and request/epoch gating needed no changes --
  already correct and shared by every document type. Found and fixed a real
  design bug via a live check: the always-present metrics finding made a
  healthy Pod's finding list never empty, silently suppressing the "not
  proven healthy" disclaimer; fixed with an explicit `has_fault` flag that
  only genuine Warning+ findings set. 98 unit + 19 fake HTTP green.
  Live-accepted: a stalled Deployment resolved through the ReplicaSet hop to
  two real `ImagePullBackOff` Pods; a failed Job resolved directly to its one
  real exit-code-1 Pod plus a correlated Warning Event; a healthy Pod showing
  real metrics alongside the honest disclaimer. Full contract: docs/
  EXPLAIN.md. Next: M5.5 Timeline.
- M5.3 accepted: rewrote `resources::health.rs` with an explicit precedence
  (deletion, terminal failure, container failure, scheduling/init, readiness,
  progressing, healthy, unknown) applied per kind (Pod, Deployment/StatefulSet/
  DaemonSet/ReplicaSet, Job, Node, PVC/PV/Namespace), and gave every result a
  bounded `evidence: Vec<String>` citing the fields that produced it. Found and
  fixed two real gaps via new unit tests before touching live fixtures:
  `ContainerCreating`/`PodInitializing` were being classified as generic
  failures by the same code path as real crashes (separated into a distinct
  non-failure case); `OOMKilled` and other terminal reasons were only read from
  `state.terminated`, never `lastState.terminated`, so the evidence vanished
  once a container rolled into `CrashLoopBackOff`. New `tests/fixtures/
  m5-health.yaml` (`m5-health-fixtures` in `test-cluster.sh`): a Pod that
  reliably triggers a genuine kernel OOM kill, a StatefulSet with a delayed
  readiness probe, a DaemonSet, a completing and a failing Job, an unreachable-
  image Deployment, a PVC on a nonexistent storage class. Live-accepted against
  `kind-sauron-test`, 13 real cases across every kind, all correct (see the
  M5.3 section above for the full list). 95 unit + 17 fake HTTP green; full
  M1-M4 and M5.1/M5.2 regression re-ran clean afterward. Full contract:
  `docs/HEALTH.md`. Next: M5.4 Explain 2.0, which can now reuse these evidence
  strings directly.
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
- M5.2a (accounting) accepted: `src/resources/accounting.rs` implements Kubernetes'
  documented init-container effective-request/limit formula and reads `status.
  qosClass` verbatim instead of re-deriving QoS. New wide-only Pod cells (`CPU/R`/
  `MEM/R`/`CPU/L`/`MEM/L`/`QOS`) and Node cells (`CPU/C`/`MEM/C`); new `Field::parse`
  key aliases (`cpu/r`, `cpu/l`, `cpu/c`, `mem/r`, `mem/l`, `mem/c`) let the existing
  typed filter/sort engine resolve them with zero new grammar. 91 unit + 17 fake HTTP
  green. Live against `kind-sauron-test`, wide mode: real per-Pod request/limit/QoS
  values, `cpu/r>1m` correctly excluding the three `BestEffort` fixtures, `qos=
  BestEffort`/`qos=Burstable` each selecting the correct disjoint subset, typed sort
  by `mem/l`. Found and fixed a real regression along the way, in test harnesses, not
  application code: M5.1's metrics-server fixture staying permanently installed made
  bare `pods` ambiguous on the shared isolated cluster, breaking `accept-m3.py`,
  `accept-m4.py`, `accept-m4-forward.py` and `soak-m4.py` wherever they launched with
  or switched to the unqualified plural. Qualified every one of those to `v1/pods`;
  re-ran all four live and all pass again (one unrelated pre-existing `configmaps`
  sort-retry timing flake self-resolved on immediate retry, unrelated to this change,
  noted rather than silently rerun). M5.2b (live Metrics API values threaded into the
  same filter/sort/table pipeline, plus usage/request and usage/limit percentages)
  is the explicit next step -- deferred out of this commit to keep it reviewable, not
  because it is easy or optional.
- M5.2b accepted: added a `resources::Metrics` trait (`usage`/`percentage`) rather
  than have `filters` depend upward on `app::metrics::Cache` directly; threaded
  `Option<&dyn Metrics>` through `Field::read`/`compare`, `Expr::evaluate`/
  `matches` and `sort::rows`, passed as `Some(&self.metrics)` from `State::rebuild`/
  `State::cell` and `None` everywhere else (tests, printer-column reads) with no
  behavior change there. Bare `cpu`/`memory` (a key space `Field::parse` already
  reserved before this milestone) now resolves to live sampled usage; new `cpu/%r`/
  `mem/%r`/`cpu/%l`/`mem/%l` resolve usage-as-percent-of-request/limit via
  `Cache::percentage`, which returns `Unknown::ZeroDenominator` on a real zero
  denominator rather than fabricating 0% or dividing into infinity. New default-
  visible `CPU`/`MEM` columns and wide-only `CPU/%R`/`MEM/%R`/`CPU/%L`/`MEM/%L`
  (Pod only). New unit test: 250m usage against a 500m request is exactly 50%; the
  same Pod with no limit anywhere gets `ZeroDenominator`, not a fabricated value;
  `Forbidden` status propagates unchanged. 88 unit + 17 fake HTTP green. Live
  against `kind-sauron-test` wide mode: real per-Pod `CPU`/`MEM` usage, `-` for the
  three `BestEffort` fixtures with no sample; `cpu>10m` matches 1 and excludes
  exactly the 3 unknowns (`?3`, Strong Kleene); typed sort by `cpu/%r` orders known
  ascending with unknowns last in both directions; `m4-logburst`'s CPU-heavy
  fixture correctly shows over 100% of its small request, uncapped and unclamped.
  Full M1-M4 regression and the M5.1 live pass re-ran clean afterward. M5.2 (both
  halves) is now fully ACCEPTED. Next: M5.3 deterministic health.
