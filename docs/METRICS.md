# Metrics and evidence contract (M5 implementation)

Metrics are optional sampled evidence, not watch events. Unknown is not zero and
unavailable metrics never erase resource rows or overwrite watch errors.

Initial budgets: one request at a time per Pod/Node view; poll 15 seconds after a
successful response, exponential failure backoff 30/60/120 seconds. Existing API
timeout applies to headers plus body. Maximum response 8 MiB, 5,000 samples and 128
containers/sample; continuation or limits are explicit PARTIAL, not complete results.
No per-render/per-row calls, no automatic provider installation in a user's cluster.
The shared kube client may internally retry 429/503/504 within that total deadline;
diagnostic request counts count collector polls, not hidden middleware wire attempts.

Runtime owns collector requests in its existing JoinSet, under the view cancellation
token and epoch. Each request snapshots watched UID/creation identity before I/O.
Scope changes clear its cache and invalidate late completions. Not a background Session.
Object names alone never authorize attaching a sample: current UID must still match
the captured UID; a reported metrics UID must match too. Source timestamp minus CPU
window must not predate object creation. Missing timestamp/window/creation identity
means unknown; new rows wait for a new correlated poll. This is best-effort correlation
because the Metrics API often omits UID, not an atomic server UID precondition.

CPU is cores averaged over the reported window; memory is working-set bytes. Reuse
the existing filter quantity parser with finite/nonnegative validation; `%` is not a
resource quantity. Each container/resource field is independently known or unknown;
malformed memory must not erase known CPU. Pod totals require coverage of all current
running regular/restartable-init/ephemeral containers; never sum only a known subset
and label it a complete total.

Samples expire after 60 seconds of source age OR monotonic local residence. Source
timestamps >5 seconds in the future are unusable. Failed polls explicitly replace
current availability; cached samples never become silently current again. Source and
receipt timestamps, unknown reasons and partial coverage are retained. Provenance is
structured and bounded, not arbitrary error text; no response body goes into tracing.

Primary contracts: [Kubernetes resource metrics pipeline](https://kubernetes.io/docs/tasks/debug/debug-cluster/resource-metrics-pipeline/)
and [Metrics API types](https://github.com/kubernetes/metrics/blob/master/pkg/apis/metrics/types.go).
Use discovered served metrics version; v1beta1 is the conventional probe when absent
from discovery, so denied discovery does not silently masquerade as absent metrics.
Implementation/live verdicts: [M5_ACCEPTANCE.md](M5_ACCEPTANCE.md).

## Isolated live fixture

`bash scripts/test-cluster.sh m5-metrics-install` verifies the Docker identity and
loopback endpoint before installing metrics-server v0.8.1 via a pinned Kustomize
manifest. It is compatible with the test cluster's Kubernetes 1.33. The fixture adds
`--kubelet-insecure-tls` solely because kind's kubelet uses a self-signed serving cert;
SAURON's API TLS validation is unchanged. Never apply this fixture to production.
Upstream attribution is in THIRD_PARTY.md. The fixture remains installed for M5 tests.
With metrics-server installed, unqualified `pods`/`nodes` are ambiguous with metrics
resources. Use `v1/pods` and `v1/nodes`. The product's default view is explicitly
`v1/pods`; human ambiguous aliases still error, never silently prefer core.
