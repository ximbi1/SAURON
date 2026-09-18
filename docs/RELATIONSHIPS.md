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
