# Relationship contracts — M6 in progress

Relationships describe topology, not causes. Initial model is metadata only,
not a second object store. No shipped Adjacent/Xray UI yet.

Identity uses discovered `Resource::id()` (canonical API version/plural), runtime
epoch, namespace/name and nonempty UID from `Object`. A different UID is a distinct
incarnation; another epoch cannot insert edges into the graph. Namespace scope and
GVK must match discovery. Human aliases never enter the model.

Edges retain direction and provenance: child→owner (`OwnerReference`),
referencing→referenced (`ExplicitReference`), Service→selected Pod (`SelectorMatch`),
and reported status references (`StatusReference`). Reverse presentation does not
turn selectors into ownership. Names/IP similarity never establishes an edge.

Ordered sets/maps provide deterministic node/edge/evidence ordering. Multiple source
paths deduplicate under one edge without erasing distinct provenance. Defaults:
128 nodes, 256 edges, depth 2, 16 evidence paths/edge, 1024 bytes/path. Reaching
limits records PARTIAL; insertions never leave half an edge. In-memory traversal
uses a visited set, preserving edge directions while walking adjacency both ways.
Fetch and candidate budgets are still to be implemented; this model alone does not
authorize API fanout or claim complete relationships.

Planned resolvers use metadata-only Secret reads, explicit namespace rules, bounded
reverse scans, additive failures and owned cancellable tasks. Arbitrary CRD rules,
global persistent graphs, causal claims and mutation features are not implemented.
See [M6_ACCEPTANCE.md](M6_ACCEPTANCE.md) for actual status and evidence.

Resolver references: [Kubernetes owner semantics](https://kubernetes.io/docs/concepts/overview/working-with-objects/owners-dependents/)
and [metadata-only negotiation](https://kubernetes.io/docs/reference/using-api/api-concepts/#metadata-only-fetches).
Namespaced owners must share the dependent's namespace; cluster-scoped dependents
cannot have namespaced owners. Secret resolution will request PartialObjectMetadata
only and must not fall back to fetching Secret bodies when negotiation fails.

## M6.1 implementation in progress

`graph::references` shares one PodSpec extractor across Pods, apps/v1 Deployment,
StatefulSet, DaemonSet, ReplicaSet, batch/v1 Job and CronJob. It preserves JSON
pointer evidence for ordinary/projected ConfigMap and Secret volumes, PVCs,
imagePullSecrets, nodeName, serviceAccountName, env/valueFrom and envFrom in regular,
init and ephemeral containers. Extraction deduplicates targets with multiple paths;
128 targets, 16 paths/target and 4096 inspected array entries bound allocations/work.
Unknown CRD spec fields are never interpreted; generic ownerReferences still apply.

`kube::relationships` resolves exact discovered GVK (not aliases or guessed plurals),
validates namespace rules and expected owner UID, and offers cancellable timed reads.
Core Secret reads use kube's `get_metadata` with no full-object fallback. Errors retain
Forbidden/NotFound/Unsupported/TargetReplaced/TimedOut rather than empty relationships.
These helpers are not yet wired into UI; reverse scans and operation-wide budgets
are now available in `kube::relationships::report`, UI remains pending.

Aggregate report budgets: sequential reads (concurrency 1), at most 49 logical
read attempts including reserved final root validation, 30-second overall deadline;
configured timeout per request. Successful bodies are capped before JSON decode
(2 MiB/object, 8 MiB/list); rejected API bodies are not consumed. One page of 200
metadata candidates and 50 matching children per explicitly selected GVR. Reverse
ownership scans selected GVR plus Pod/ReplicaSet/Job only; coverage is always labeled
PARTIAL, not arbitrary cluster-wide completeness. Maximum 64 source issues with an
explicit omitted-issues marker. Root UID and resourceVersion are revalidated after
collection; a changed root is rejected as replaced/stale, not labeled current.
No permanent graph cache. Request counts are logical attempts, not middleware retries.

## M6.2: network references (ACCEPTED)

`graph::references::network` extracts Service equality selectors (never applied
across namespaces, an empty/absent selector selects nothing), Ingress
defaultBackend/rule-path/TLS Secret references, EndpointSlice's owning-Service
label and every endpoint's explicit `targetRef`, and Endpoints' subset
addresses. An IP address alone never creates an edge — only an explicit
`targetRef` does, and a missing `kind`/`name` on it is `malformed`, not
silently dropped. Kubernetes' EndpointSlice controller commonly omits
`targetRef.apiVersion`; this is tolerated only for `Provenance::StatusReference`
targets, resolved against the discovery catalog only when exactly one resource
of that `kind` exists — a same-kind cross-group collision stays `Unsupported`
rather than guessing. `kube::relationships::report` adds a `network()` pass:
Pod→Service and Service→Pod selector matches (bounded candidate page, reverse
presented but never treated as ownership) and Service→EndpointSlice via the
owning-Service label, with an EndpointSlice-claims-a-different-Service-UID
mismatch rejected as `TargetReplaced`, never silently accepted.

## M6.3: storage references (ACCEPTED)

`graph::references::storage` extracts PVC `spec.volumeName`→PersistentVolume,
PV `spec.claimRef`→PersistentVolumeClaim (the PV-side UID Kubernetes populates
on bind is carried verbatim and later UID-validated by the existing resolver —
`claimRef`'s kind/apiVersion are hardcoded because the field is schema-fixed to
name exactly one PVC, unlike EndpointSlice's targetRef which can point at any
kind), and PVC/PV `spec.storageClassName`→cluster-scoped StorageClass.
`kube::relationships::report::reverse_references()` adds bounded reverse
lookups for ConfigMap/Secret/ServiceAccount/PVC roots: an explicit built-in
workload candidate list (Pods, Deployments, StatefulSets, DaemonSets, Jobs,
CronJobs) is scanned per namespace and linked back via each candidate's own
extractor. A denied kind records an issue but never removes edges already
found via another kind. Still no arbitrary CRD reference inference.

## M6.4: Adjacent (ACCEPTED)

`src/adjacent.rs` renders a `report::Report` as text grouped by intrinsic
direction and provenance: OWNED BY / OWNS (`OwnerReference`, split by which
side the selected object is on), SELECTED BY (`SelectorMatch`, one group
regardless of direction — selection is never ownership), REFERENCES /
REFERENCED BY (`ExplicitReference` and `StatusReference`, split by direction;
a `StatusReference` row is annotated "(status reference)" rather than folded
indistinguishably into a user-authored reference). Each rendered row records
its exact line index plus canonical GVK/namespace/UID as an `adjacent::Target`.

The `:adjacent`/`a` action opens this view through its own resolver call
(`kube::relationships::report::adjacent`), not the generic per-action
`kube::evidence::document` path used by Yaml/Explain/Events. `enter` (`Follow`)
maps the document's current scroll position to the nearest target at or after
the top of the viewport and navigates via the *exact* `Resource` already
resolved for that target (never a re-resolved name string) plus its UID,
reusing the same `push_history`/`apply_history`/`finish_history` stack as
`[`/`]` history navigation — Adjacent navigation is an ordinary, reversible
history entry, not a special case.

## M6.5: Xray (ACCEPTED)

`kube::relationships::report` factors Adjacent's single-hop logic (forward
references, bounded reverse ownership, network selectors/EndpointSlice,
reverse Config/Secret/ServiceAccount/PVC scans) into a shared `expand(...,
center: &Identity, ...)` taking an explicit center instead of always the
report's global root. `adjacent()` calls it once with center = root
(unchanged behavior). `xray()` calls it once per frontier node across a BFS
bounded to depth 1-3 (clamped; the UI opens at depth 2). Before `expand()`
trusts a frontier node's edges, that node is re-fetched fresh and
UID-validated the same way every other reference is — a same-name
replacement mid-traversal is rejected for that node, never silently
inherited into the graph. A `visited` identity set makes the BFS cycle-safe
by construction: re-discovering an already-expanded node (e.g. a real
ownerReference pointing back to the root) produces an edge, never a second
expansion or an infinite loop.

`src/xray.rs` renders the traversal as depth-grouped text (`HOP 1`, `HOP 2`,
...), reusing Adjacent's exact direction/provenance labels via a shared
`adjacent::label()` helper and the same deterministic health per node —
never a second, parallel "graph health", and never a claim that a related
object is the cause of a problem. `:xray`/`x` opens through the same
document path as Adjacent (`start_adjacent`, branching on `Action::Xray`);
`Follow` (`enter`) works identically since both views populate the same
`Document.adjacent` navigable-target list.
