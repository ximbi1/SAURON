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
