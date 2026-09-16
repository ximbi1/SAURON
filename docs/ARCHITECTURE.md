# Architecture

See HANDBOOK for current implementation status and module ownership.

## State and effects

The application owns the selected connection, resource query, store, row order, UID
selection and mode. Input maps to named actions. Reducers change state and request effects.
Network effects own immutable scope plus epoch and CancellationToken. They report through
a bounded channel. A navigation action cancels prior work, increments epoch, and clears
scope-sensitive data. Reports and streams have their own request IDs to reject late data.

Watch lifecycle: Connecting → Listing → Live → Reconnecting/Error. A successful initial
list is not proof that the long-lived watch has been established. Do not call data fresh
after transport error. Relists stage objects and atomically replace the store at completion.
Retain stale rows while recovering; bound both active and staging allocations. Count/byte
limits produce explicit incomplete states. Selection follows UID, never row number alone.

Discovery reads core independently, enumerates served group versions with bounded
concurrency, collects per-group failures, and retains scope/verbs/shortnames. Prefer
server preferred versions, preserve a qualified version path for APIs without that kind.
Discovery support never implies user authorization. Generic resources use dynamic objects;
typed projections and health rules are qualified by GVK. CRD printer/Table cells must be
keyed by GVR + UID + resourceVersion so stale cells never attach to replacement objects.

## Work budgets

Initial contracts: 256 event slots; at most 64 messages processed per input/render loop
iteration; one view watch; discovery concurrency 4; 10-second read deadline; object cap
20,000; log cap 5,000 lines and 4 MiB; timeline 64 changes/object and 256 objects. These
are design defaults and must be checked against code/benchmarks as implemented.
Cancellation includes queue waits, read deadlines, retry waits and stream polling.
No uncontrolled spawning and no shared mutable cluster stores. Rendering has no client.

M4 adds `app/session.rs` alongside (not instead of) the existing view-worker JoinSet.
Long-running logs have an owned task, monotonic SessionId, immutable origin/UID and
bounded retained outcomes. Data uses epoch/request/session gates; task completion is
reaped independently of the UI channel, even after navigation. Session status is local
to its document through search/palette overlays. Limits: 8 active tasks, 64 ended
records, two-second cancellation grace then abort/join. See [SESSIONS.md](SESSIONS.md).

## Safety design

Inspection first. Mutation service later accepts typed Intent with GVR, namespace, name,
UID, observed resourceVersion, action parameters and connection identity. Evaluate global,
cluster and context readonly plus all matching guardrails (deny wins, strongest confirmation
wins, smallest bulk cap wins). Fetch RBAC evidence when available; the API decides final
authorization. Preview exact target/current/proposed values and patch; re-read/check identity
before sending UID/RV preconditions. Journal started and final/unknown outcomes without
sensitive parameters. Accepted mutations cannot be undone by cancelling the local task.

Secret values are removed before cached projections. No secret reveal in initial release.
Treat logs and annotations as untrusted text. No TLS bypass defaults, kubeconfig writes or
telemetry. External plugins are a future trust boundary; never describe them as sandboxed.

## Extensibility and differentiators

Graph edges distinguish verified ownership (UID), explicit references and inferred label
selection. Explain attaches evidence paths/timestamps/objects to findings; partial reads
are findings about missing evidence. Blast radius describes best-effort reachability.
Eye/Pulse summarize bounded collectors with per-kind errors, not hidden aggregate zeros.
Context diff compares independently scoped read-only snapshots. Bundles preview redacted
content and collection omissions before local export. No cross-context mutation batching.
