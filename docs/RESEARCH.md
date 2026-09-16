# Research baseline — 2026-09-15

## Provenance

Public behavior benchmark: [Sofka website](https://sofka.rs/),
[repository](https://github.com/nklmilojevic/sofka), inspected HEAD
`2024e5921cf9cb06fd02f666630f97bae5e24eb3`, package version 0.27.3.
The site embeds older screenshots (0.20.0), a 0.24.9 performance comparison,
and registry availability from 0.26.0. Do not treat screenshot version as current.
No source, prose, artwork, or assets copied into the application.

## Reading inventory and implications

All paths below refer to that pinned repository revision.

| Source | Observations used in design |
| --- | --- |
| `README.md`, `docs/features.md` | Shared built-in/custom object browsing, incident and operational workflows; breadth exceeds initial mission examples |
| `docs/architecture.md` | Generation-tagged updates and cached projections; SAURON instead explicitly bounds queue length |
| `docs/keys.md`, `docs/keybindings.md` | Separate text-input/navigation modes; generated help; conflict detection; preserve native text selection |
| `docs/configuration.md` | Layering, reload, namespace state, TOML/YAML, drop-ins, Home Manager; SAURON starts strict TOML |
| `docs/views.md` | Typed custom/metric columns, CRD printer expressions, Table fallback, explicit relation rules |
| `docs/filtering.md` | Real Boolean grammar; API selectors outside OR; missing metrics must remain unknown |
| `docs/safety.md` | Combined guardrails, readonly, capability reviews; journal records starts rather than outcomes |
| `docs/debugging.md` | Evidence refresh checks UID; timeline, diff, bounded logs, debug helpers, redacted bundles and diagnostics |
| `docs/providers.md` | Prometheus/VictoriaMetrics right-sizing, VictoriaLogs history, opt-in fleet; distinguish provenance and absent providers |
| `docs/plugins.md`, `docs/plugin-authoring.md` | Inline commands, packages/catalog, structured reports and inputs, cancellation/output bounds, no sandbox |
| `docs/benchmark-k9s.md`, `docs/vs-k9s.md` | Methodology matters; reported advantage is workload-specific, not a Rust guarantee |
| `Cargo.toml`, licenses | Dependency versions and MIT OR Apache-2.0 inspiration; avoid copying large dependency surface |
| `src/k8s/discovery.rs` | Shortnames lost by high-level conversions; group failures must be isolated |
| `src/helm.rs` | Native inspection is feasible from Helm storage; rollback is a separate reconciliation workflow |

Additional feature inventory: container trends and probes; scroll indicators; TLS proxy
compatibility flags; faults-only view; marked log merging; clipboard/OSC52; range selection;
CronJob trigger; ExternalSecret refresh; debug containers/nodes; PVC helper lifecycle;
read-only right-sizing previews; plugin catalog checksums/withdrawal/rollback and bundled
pod sanitizer. These are tracked separately from the core in SOFKA_PARITY.

## Documentation conflicts and boundaries

`docs/safety.md` drain text says no unmanaged/emptyDir overrides and permits continuation
to other nodes after preflight failure. `docs/features.md` describes a newer form with
those options, explicit PDB bypass, a shared deadline, and stop-on-failure. SAURON follows
Kubernetes semantics and requires its own acceptance tests, not either text verbatim.
Sofka native Helm inspection does not mean native rollback: documented rollback/uninstall
invoke Helm. Native describe is opt-in with kubectl fallback. These differences must stay
visible in parity tracking.

Roadmap search returned [historical future.md](https://github.com/nklmilojevic/sofka/blob/main/future.md).
It is absent at inspected HEAD. Search-index contents mention trace providers, signing,
SBOM, Windows evaluation and incident cockpit; these are historical aspirations, not
evidence of current shipped functionality. Current features/keys are the parity baseline.

## Ecosystem choices and primary sources

- [kube 4.2](https://docs.rs/kube/4.2.0/kube/): supported client/runtime, dynamic APIs,
  standard Rustls and kubeconfig handling. Use watcher retry semantics with explicit backoff.
- [k8s-openapi 0.28](https://docs.rs/k8s-openapi/0.28.0/k8s_openapi/): schemas 1.32–1.36;
  choose 1.32 for built-in structs and dynamic objects for extension fields.
- [Ratatui 0.30.2](https://docs.rs/ratatui/0.30.2/ratatui/): maintained terminal widgets,
  TestBackend and restore utilities. Crossterm 0.29 event stream integrates with Tokio.
- [Kubernetes API concepts](https://kubernetes.io/docs/reference/using-api/api-concepts/):
  list/watch consistency, pagination, expired resourceVersion/relist, Table negotiation.
- [Helm storage driver](https://github.com/helm/helm/tree/main/pkg/storage/driver):
  inspect releases only after bounded decode and redaction are ready.

Use Cargo.lock for actual resolved versions. No fuzzy dependency initially: simple
subsequence matching is sufficient for a first catalog, measurable before replacement.
Regex uses the Rust regex engine (no backtracking). JSON is canonical internal document
representation; YAML serialization may use a maintained serializer when that slice lands.
Tokio task and channel limits are application contracts, not defaults delegated to crates.

## M4 addendum — 2026-09-16

Re-read pinned [log behavior](https://github.com/nklmilojevic/sofka/blob/2024e5921cf9cb06fd02f666630f97bae5e24eb3/docs/features.md#actions)
and [log controls](https://github.com/nklmilojevic/sofka/blob/2024e5921cf9cb06fd02f666630f97bae5e24eb3/docs/debugging.md#log-controls).
Benchmark includes fixed source sets with per-source labels, bounded history, filtering,
timestamp-aware merging, pause/clear and explicit multi-source selection. SAURON will
state its narrower scope and ordering semantics rather than imply complete parity.

Inspected the actual downloaded locked kube-client 4.2.0 implementation:
`api/remote_command.rs`, `api/portforward.rs`, `api/subresource.rs`.
Native WebSocket exec and attach are separate APIs. AttachedProcess owns and aborts its
task on Drop, offers separate optional pipes, status future and TTY resize sender. A
missing remote Status cannot mean success. Portforwarder exposes one stream per remote
port and explicit abort/join, **but no abort-on-Drop implementation in 4.2.0**: SAURON
must add an owned guard before cancellation can safely drop a forwarding future. Its
duplex buffer is 1 MiB per port; bound concurrent local TCP connections accordingly.
No source copied. Existing `ws` feature already supplies these APIs.

Isolated cluster restoration uses [kind configuration](https://kind.sigs.k8s.io/docs/user/configuration/)
with explicit loopback API address, named cluster and dedicated kubeconfig argument.
