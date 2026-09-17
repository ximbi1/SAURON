# Explain 2.0 contract (M5.4)

Explain = health findings + fresh evidence + bounded correlation. It is not a
second, parallel diagnosis: it reuses `resources::health`'s deterministic
rules and their cited evidence directly, never re-scanning conditions/
container states with its own logic. Order of construction:

1. **Fresh GET + UID check** (pre-existing, unchanged): before anything else,
   `kube::evidence::document()` re-fetches the selected object and aborts with
   a replacement error if its UID no longer matches. A stale report is never
   finalized against an incarnation that no longer exists.
2. **`Health.evidence` injected directly.** The top finding's evidence is
   `object.health.evidence.join("; ")` -- the exact field-citing lines M5.3
   already computed -- not a fresh re-derivation. Empty only when the object
   is itself Healthy/Unknown, matching M5.3's own convention.
3. **Events**, reusing M3's exact semantics: UID-scoped, capped at 200, a
   failed/forbidden/timed-out Events fetch degrades to a `PARTIAL EVIDENCE`
   note rather than failing the whole report.
4. **Metrics as descriptive evidence**, never a claimed cause. A snapshot of
   the view's own last successful sample (re-captured fresh on every Refresh,
   never baked into the reused `Document::Source`) is shown for Pods/Nodes
   only -- `Severity::Healthy`, so it never itself counts as "fault found."
5. **Workload → Pods, verified ownership only.** `ownerReferences` matched by
   UID, never a name or label heuristic. StatefulSet/DaemonSet/ReplicaSet/Job
   own Pods directly (one hop). Deployment does not -- it owns ReplicaSets,
   which own Pods -- so that path is a genuine, bounded *two*-hop resolution
   (list owned ReplicaSets, then list Pods owned by any of those), not a step
   toward a general relationship graph (M6's job). Bounded to 200 listed
   before filtering and 50 owned children after, both explicit as `PARTIAL
   EVIDENCE` when hit, matching the Events cap's own convention.
6. **Partial evidence stays explicit and additive.** Metrics-forbidden,
   Events-unavailable, owned-Pods-forbidden/timed-out are all distinct
   `PARTIAL EVIDENCE` lines; whatever evidence *was* collected is never
   discarded because one source failed.
7. **Request/epoch gating** (pre-existing, unchanged): every document type,
   Explain included, is delivered through the same `Payload::Document{request,
   ...}` path already gated by `request == self.state.request`. An old
   Explain can never overwrite a newer one; this needed no new code.

## Fault vs. descriptive findings

A `Finding`'s presence does not by itself mean something is wrong. Metrics
findings and the "N owned Pod(s) checked, none unhealthy" note are always
`Severity::Healthy`/`Unknown` and never suppress the honest "no failure
evidence found... this does not prove the resource is healthy" disclaimer --
that disclaimer fires whenever no *fault* (a Warning+ health finding on the
object or a child, or a Warning Event) was found, regardless of how many
purely descriptive findings exist alongside it.

## Scope

Selected object, verified-ownership child Pods (workloads only), and
UID-related Events. Nodes, storage and cross-kind relationships are not
correlated -- that is M6's relationship-graph work. `Finding` itself was left
alone (`severity`, `finding`, `evidence: String`, `affected`, `next`) --
multiple evidence lines join into the one `evidence` string rather than
motivating a structural change M5.4 does not actually need.

Implementation/live verdict: [M5_ACCEPTANCE.md](M5_ACCEPTANCE.md).
