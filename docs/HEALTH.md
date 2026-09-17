# Deterministic health contract (M5.3)

`resources::health::derive()` is a pure function over already-known
spec/status fields. No AI, no fuzzy scoring, no hidden weighting. Every
result is one of `Healthy`/`Warning`/`Critical`/`Unknown` plus a short status
string and a bounded `Vec<String>` of evidence lines citing the actual
fields/values that produced it. `Healthy`/`Unknown` carry no evidence -- there
is nothing to explain either way. Missing or ambiguous critical evidence
resolves to `Unknown`, never a default `Healthy`.

## Precedence

Deletion, then terminal failure, then container failure, then scheduling/init
problems, then readiness degradation, then progressing, then healthy, then
unknown. This is a design principle applied per resource kind, not one literal
rule table -- what counts as "terminal" or "progressing" differs by kind (a
Pod's terminal state is its own phase; a workload's is a stalled/failed
rollout condition; a Job's is a `Failed` condition).

## Pods

1. `metadata.deletionTimestamp` set → `Terminating` (Warning), regardless of
   anything else.
2. `status.phase == Failed` → the reported reason (Critical), with any
   container failure evidence attached as supporting detail, not discarded.
3. `status.phase == Succeeded` → `Completed` (Healthy).
4. Container failure -- checked before scheduling/init problems, since once a
   Pod has any `containerStatuses` at all, `PodScheduled` almost never flips
   back to `False`, and a real crash is more actionable than a stale
   scheduling note:
   - Regular containers first.
   - Then any init container that is *currently relevant*: a restartable
     ("sidecar", `restartPolicy: Always`) init container, which runs for the
     Pod's whole lifetime and can crash long after `Initialized`, or a
     non-restartable init container while `Initialized != True`.
   - A failure is `state.waiting.reason` other than `ContainerCreating`/
     `PodInitializing` (those are ordinary startup, not failures -- treating
     them as failures would let "just starting" preempt a real problem
     elsewhere), or `state.terminated` with a nonzero exit code.
   - Evidence always includes the container name, the waiting/terminated
     reason, `restartCount`, and -- critically -- `lastState.terminated.reason`
     when present, so an `OOMKilled` that has already rolled into
     `CrashLoopBackOff` is never lost.
5. `PodScheduled == False` → the condition's own reason (default
   `Unschedulable`, Warning) with its message as evidence.
6. Not yet `Initialized`, no failure → `Init:<complete>/<total>` (Warning).
7. `phase == Running`:
   - `Ready == True` → `Running` (Healthy).
   - Otherwise → `NotReady` (Warning), evidence naming which conditions are
     not `True` and which containers report `ready: false`.
8. Anything else (`Pending`, etc.) → the phase itself (Warning), or `Unknown`
   for `phase == Unknown`.

## Deployment / StatefulSet / DaemonSet / ReplicaSet

Shared base: `desired == 0` → `ScaledDown` (Healthy) for scalable workloads,
`NoEligibleNodes` (Unknown) for a DaemonSet with none. `Progressing == False`
→ `Stalled` (Critical). `ReplicaFailure == True` → `Degraded` (Critical).
Zero ready replicas → `Unavailable` (Critical). A controller whose
`status.observedGeneration` lags `metadata.generation` is `Progressing`
(Warning) regardless of how many old replicas are still running -- it has not
reconciled the current desired state yet, so it is never called healthy on
the strength of stale replicas alone.

- **Deployment**: additionally `Progressing` while `updatedReplicas <
  desired`.
- **DaemonSet**: uses its own fields throughout (`desiredNumberScheduled`,
  `numberReady`, `updatedNumberScheduled`, `numberAvailable`) rather than
  Deployment semantics; `Progressing` while updated or available lag desired.
- **StatefulSet**: `RollingUpdate` with a `partition` only requires ordinals
  at or above the partition to be updated -- holding back the lowest-ordinal
  Pods is deliberate, not a stall. `OnDelete` never auto-rolls; a
  `currentRevision`/`updateRevision` mismatch under `OnDelete` is reported as
  `Progressing` evidence (waiting for manual Pod deletion), not silently
  ignored and not a fault.
- Fully converged → `Ready` (Healthy).

## Job

A `Failed` (or `FailureTarget`) condition → `Failed` (Critical), with the
condition's message as evidence. `Complete == True` → `Completed` (Healthy)
*unconditionally* -- a successful Job is healthy no matter how many attempts
it took; backoff history alone is not a fault once it has actually completed.
`spec.suspend == true` → `Suspended` (Warning). `status.failed > 0` without a
`Failed` condition yet → `Retrying` (Warning, distinct from `Failed`), citing
the failure count. Otherwise `Running`/`Pending` (Warning) from
`status.active`.

## Node

Condition polarity is not uniform and must not be treated as if it were:
`Ready == True` is good evidence; `MemoryPressure`/`DiskPressure`/
`PIDPressure`/`NetworkUnavailable == True` are bad evidence, each reported by
name with the condition as evidence. Any of those `== Unknown` is `Unknown`,
never silently treated as good or bad. `Ready`'s own `Unknown`/absent is
`Unknown`. Otherwise `Ready`/`NotReady`, with `,SchedulingDisabled` appended
when `spec.unschedulable` is set; `NotReady` carries the condition's message
as evidence.

## Storage (PVC / PV / Namespace)

`status.phase` maps directly: `Bound`/`Active`/`Available` → Healthy,
`Lost`/`Failed` → Critical (with `status.message` as evidence when present),
anything else (`Pending`, etc.) → Warning. Deliberately shallow: this reports
the phase Kubernetes itself assigned, not an inferred cause (binding delay,
provisioner failure, capacity). Correlating storage failures to a cause is
M6's relationship-graph work, not this milestone's.

## Fixtures

`bash scripts/test-cluster.sh m5-health-fixtures` (isolated `kind-sauron-test`
only) adds: a Pod that reliably triggers a real kernel OOM kill (`yes | head
-c 200000000` against a 20Mi memory limit); a 2-replica StatefulSet with a
90-second readiness-probe delay so it observably sits `Progressing` for that
window; a DaemonSet; a Job that completes and one that fails
(`backoffLimit: 0`); a 2-replica Deployment with an unreachable image so it
stays `Unavailable`; a PVC referencing a nonexistent storage class so it stays
`Pending`. Combined with the pre-existing `crashloop`/`missing-image`/
`unschedulable` fixtures, this exercises every precedence branch above against
real cluster state, not synthetic JSON alone.

Live-accepted 2026-09-17 against `kind-sauron-test`: see
[M5_ACCEPTANCE.md](M5_ACCEPTANCE.md) for the recorded evidence.
