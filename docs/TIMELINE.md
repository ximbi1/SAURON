# Timeline contract (M5.5)

Session-local, bounded history of *meaningful* watch-observed state
transitions. It is explicitly **not**: Kubernetes Events, an audit log, or
persistent. It dies with the process; nothing here survives a restart.

## Identity

Keyed by UID (`resources::store::Store::histories: BTreeMap<uid, VecDeque
<Change>>`). Same namespace/name with a different UID is a different
timeline -- name/namespace are display context only, never the key.

## What gets recorded

`resources::timeline::meaningful_diff()` is a curated comparison, not a
generic deep diff -- `resourceVersion` churn with no meaningful change
produces nothing. Compared fields: derived health status; `status.phase`;
`metadata.generation`/`status.observedGeneration`; `spec.replicas`/
`readyReplicas`/`availableReplicas`/`updatedReplicas`/
`desiredNumberScheduled`/`numberReady`/`numberAvailable`; deletion-timestamp
transitioning from unset to set; for Pods specifically, total restart count,
each container's waiting/terminated reason (comparing by container name, not
position), and each container's image.

## Relist must not invent transitions

This is the one rule that actually matters for correctness. Before a relist
(watch reconnect) replaces the live object set, `Store::finish()` diffs
*every* slot present before or after the relist against its prior state,
using the same `meaningful_diff()` as the live incremental path:

- Same UID, same `resourceVersion` (nothing changed while disconnected) →
  nothing recorded.
- Same UID, different `resourceVersion`, meaningful fields differ → exactly
  **one** delta from the last known state straight to the newly observed
  one. Nothing in between is synthesized -- if three intermediate phases
  happened while disconnected, none of them are invented; only "last known →
  now" is ever recorded.
- Different UID under the same slot (recreated while disconnected) →
  "Object replaced by a different UID" against the *old* UID's timeline.
- Present before, absent after (deleted while disconnected) → "Deleted
  (observed by watch)" against the old UID's timeline.

Every recorded entry carries a `Source`: `WatchObserved` (seen live) or
`RelistObserved` (reconstructed by diffing a relist against last known
state -- real, but the gap itself was never observed). The `:timeline`
document shows this per entry (`[watch]`/`[relist]`) so a compressed,
possibly-multi-step gap is never presented as identical to a directly
observed single transition.

## Bounds

64 entries per object, 256 objects total, both deterministic FIFO eviction
(oldest object's whole history evicted first when the object cap is hit;
oldest entry within one object's history evicted first when that object's
own cap is hit). These bounds pre-existed M5 and needed no change.

## UX

`:timeline` / `T` in table mode. Renders newest-first (the most recent
transition is almost always why the document was opened). `Refresh` (`r`)
re-renders directly from the in-memory `Store` -- no network call, matching
the forward manager's own "Refresh reads local session state" convention.
Session-only: a context/namespace switch creates a fresh `Store` and
therefore a fresh (empty) timeline for every UID, by the same isolation
`docs/SESSIONS.md` already establishes for background sessions -- history
never leaks across scope changes.

Implementation/live verdict: [M5_ACCEPTANCE.md](M5_ACCEPTANCE.md).
