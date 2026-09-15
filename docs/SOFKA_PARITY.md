# Sofka parity matrix

Baseline: Sofka 0.27.3, commit `2024e5921cf9cb06fd02f666630f97bae5e24eb3`, inspected 2026-09-15.
Sources and discrepancies: [research](RESEARCH.md). Each row is a tracked workflow;
combined terms list explicit sub-capabilities. Research is not acceptance. P0 foundation,
P1 operational core, P2 breadth, P3 integrations. Tests listed here are acceptance plans
until an actual test/run is linked. Historical roadmap items are not claimed as shipped.

| Area | Sofka capability | SAURON equivalent | Priority | Status | Test | Notes |
| ---- | ---------------- | ----------------- | -------- | ------ | ---- | ----- |
| Navigation | Kubeconfig / exec credentials / explicit context | Native config/client | P0 | ACCEPTED | M1/M2 live kind; RUNBOOK | Credential plugins delegated to kube; not independently acceptance-tested |
| Navigation | Context picker, switching, remembered namespaces | Epoch-scoped context navigation | P0 | ACCEPTED | M2 items 1/6 live | Session-local memory |
| Navigation | Namespace picker, all namespaces, active/default labels | Namespace scope | P0 | ACCEPTED | M2 items 2/6 live | Restricted-list fallback is still a gap |
| Navigation | Favorites 1–9, recents, selected-row namespace | Config favorites/session history | P1 | RESEARCHED | Pending: unit + scoped acceptance | Not yet delivered |
| Navigation | Shortnames, aliases, API groups, precedence | Discovered GVR catalog and aliases | P0 | ACCEPTED | M2 item 3 live + resolve regressions | M3 tightens selector/history identity boundaries |
| Navigation | CRDs, custom resources, arbitrary discovered kinds | Dynamic resource watch/render | P0 | ACCEPTED | M2 item 3/6 live | Rich printer columns remain M3 work |
| Navigation | Drill-down, breadcrumbs, history, resource cycling | Bounded back/forward + breadcrumbs | P1 | IMPLEMENTING | M2 item 4/6 accepted live | Drill-down/resource cycling not delivered; M3 adds selector history |
| Navigation | Wide columns, horizontal scroll with pinned identity | Typed column layout | P1 | RESEARCHED | Pending: unit + scoped acceptance | Not yet delivered |
| Navigation | Compact mode, hidden header, terminal title | Responsive terminal chrome | P1 | DESIGNED | Pending: unit + scoped acceptance | Not yet delivered |
| Navigation | Global fuzzy object finder with partial RBAC results | Bounded cross-kind finder | P1 | RESEARCHED | Pending: unit + scoped acceptance | Not yet delivered |
| Navigation | Name/cell clipboard and OSC52 fallback | Explicit clipboard actions | P2 | RESEARCHED | Pending: unit + scoped acceptance | Not yet delivered |
| Live data | Watches, relists, reconnects/backoff, expired RV | kube watcher with bounded messages/staged store | P0 | DESIGNED | Pending: fake API + real watch | Not yet delivered |
| Live data | UID selection, replacement detection, generation tags | UID identity / view epoch | P0 | DESIGNED | Pending: fake API + real watch | Not yet delivered |
| Live data | Skipped API groups, RBAC errors, partial discovery | Per-group errors and incomplete state | P0 | DESIGNED | Pending: fake API + real watch | Not yet delivered |
| Live data | Cached row projections and batching | Incremental projections / capped redraw cadence | P0 | DESIGNED | Pending: fake API + real watch | Not yet delivered |
| Views | Pods curated columns | GVK-qualified projection | P0 | DESIGNED | Pending: unit + scoped acceptance | Not yet delivered |
| Views | Deployments curated columns | GVK-qualified projection | P0 | DESIGNED | Pending: unit + scoped acceptance | Not yet delivered |
| Views | StatefulSets curated columns | GVK-qualified projection | P0 | DESIGNED | Pending: unit + scoped acceptance | Not yet delivered |
| Views | DaemonSets curated columns | GVK-qualified projection | P0 | DESIGNED | Pending: unit + scoped acceptance | Not yet delivered |
| Views | ReplicaSets curated columns | GVK-qualified projection | P0 | DESIGNED | Pending: unit + scoped acceptance | Not yet delivered |
| Views | Services curated columns | GVK-qualified projection | P0 | DESIGNED | Pending: unit + scoped acceptance | Not yet delivered |
| Views | Nodes curated columns | GVK-qualified projection | P0 | DESIGNED | Pending: unit + scoped acceptance | Not yet delivered |
| Views | Namespaces curated columns | GVK-qualified projection | P0 | DESIGNED | Pending: unit + scoped acceptance | Not yet delivered |
| Views | ConfigMaps curated columns | GVK-qualified projection | P0 | DESIGNED | Pending: unit + scoped acceptance | Not yet delivered |
| Views | Secrets curated columns | GVK-qualified projection | P0 | DESIGNED | Pending: unit + scoped acceptance | Not yet delivered |
| Views | Jobs curated columns | GVK-qualified projection | P0 | DESIGNED | Pending: unit + scoped acceptance | Not yet delivered |
| Views | CronJobs curated columns | GVK-qualified projection | P0 | DESIGNED | Pending: unit + scoped acceptance | Not yet delivered |
| Views | PVCs curated columns | GVK-qualified projection | P0 | DESIGNED | Pending: unit + scoped acceptance | Not yet delivered |
| Views | PVs curated columns | GVK-qualified projection | P0 | DESIGNED | Pending: unit + scoped acceptance | Not yet delivered |
| Views | Ingresses curated columns | GVK-qualified projection | P0 | DESIGNED | Pending: unit + scoped acceptance | Not yet delivered |
| Views | Endpoints curated columns | GVK-qualified projection | P0 | DESIGNED | Pending: unit + scoped acceptance | Not yet delivered |
| Views | CRDs curated columns | GVK-qualified projection | P0 | DESIGNED | Pending: unit + scoped acceptance | Not yet delivered |
| Views | ApplicationSets status/generators/apps | Argo-specific projection | P2 | RESEARCHED | Pending: unit + scoped acceptance | Not yet delivered |
| Views | Pod containers: regular/init/sidecar/ephemeral, probes/ports/images | Container inspection | P1 | RESEARCHED | Pending: unit + scoped acceptance | Not yet delivered |
| Views | Container CPU/memory five-minute trends | Bounded per-container sample ring | P2 | RESEARCHED | Pending: unit + scoped acceptance | Not yet delivered |
| Views | CRD printer columns including condition selectors | Validated printer expression subset | P1 | RESEARCHED | Pending: unit + scoped acceptance | Not yet delivered |
| Views | Server Tables, watch/poll fallback, UID/RV cell validity | Negotiated Table adapter | P1 | RESEARCHED | Pending: unit + scoped acceptance | Not yet delivered |
| Views | Custom columns, types, image tags, quantities, namespaces | Declarative typed views | P2 | DESIGNED | Pending: unit + scoped acceptance | Not yet delivered |
| Filtering | Fuzzy, quoted substring, regex, inverse | Bounded AST text predicates | P0 | ACCEPTED | M3 item 1 unit + live accept-m3.py filters | FILTERS.md |
| Filtering | Label key/value local search | Case-sensitive labels + existence AST | P1 | ACCEPTED | M3 item 1 unit + live | Missing comparisons UNKNOWN |
| Filtering | Server label/field selectors, combined scopes | Separate query selector fields + history | P0 | ACCEPTED | Fake HTTP list/watch; M3 live selector/history flow | No automatic pushdown |
| Filtering | Typed comparisons: CPU/memory/age/count/percent | Typed scalar values with explicit unknown | P0 | ACCEPTED | M3 unit + live CRD/Pod comparisons | Metrics collection unavailable; finite f64 quantities |
| Filtering | AND/OR/parentheses/negation, invalid-input errors | Lexer + recursive descent AST | P0 | ACCEPTED | M3 unit + live recovery including regression replay | Strong Kleene; unknown count visible |
| Filtering | Faults-only pod toggle | Health predicate | P1 | RESEARCHED | Pending: AST/unknown/selector unit + integration | Not yet delivered |
| Sorting | Column picker, ascending/descending, age shortcut | Stable typed cycle/explicit command | P0 | ACCEPTED | M3 item 2 unit + live accept-m3.py sorting | Unknown last; no picker/age shortcut, use :sort age; generic Table types item 5 |
| Sorting | Configured defaults and remembered per-kind sort | Config/session persistence | P2 | RESEARCHED | Pending: unit + scoped acceptance | Not yet delivered |
| Metrics | Pod/node/container CPU and memory | Optional Metrics API collector | P1 | DESIGNED | Pending: unit + scoped acceptance | Not yet delivered |
| Metrics | Requests/limits, percentages, allocatable, QoS | Quantity/accounting projection | P1 | RESEARCHED | Pending: unit + scoped acceptance | Not yet delivered |
| Metrics | Threshold colors by resource/context | Semantic threshold configuration | P2 | RESEARCHED | Pending: unit + scoped acceptance | Not yet delivered |
| Health | Pod phase/reason/init/sidecar/gate/failure precedence | Deterministic Pod state model | P0 | DESIGNED | Pending: unit + scoped acceptance | Not yet delivered |
| Health | Workload rollout/observedGeneration/partition/OnDelete | Workload health rules | P1 | DESIGNED | Pending: unit + scoped acceptance | Not yet delivered |
| Health | Jobs, storage deletion, Node condition polarity | GVK health rules | P1 | DESIGNED | Pending: unit + scoped acceptance | Not yet delivered |
| Explain | Fresh selected object, UID checks, cancellable latest report | Evidence collection + pure rules | P0 | DESIGNED | Pending: unit + scoped acceptance | Not yet delivered |
| Explain | Workload/Pod/container/event evidence, finding navigation | Evidence report and related targets | P1 | DESIGNED | Pending: unit + scoped acceptance | Not yet delivered |
| Timeline | Bounded per-UID meaningful watch transitions | Session change ring | P1 | DESIGNED | Pending: unit + scoped acceptance | Not yet delivered |
| Events | UID-related events, Warning filtering, correct last-seen | Native Event view | P1 | DESIGNED | Pending: unit + scoped acceptance | Not yet delivered |
| Logs | Follow/tail/timestamps/previous/container selection | Cancellable native log stream | P1 | DESIGNED | Pending: unit + scoped acceptance | Not yet delivered |
| Logs | Multi-container/workload/service/marked-pod combined logs | Bounded multiplexed log sources | P1 | RESEARCHED | Pending: unit + scoped acceptance | Not yet delivered |
| Logs | Filter/search/wrap/fullscreen/copy/save/time anchors | Shared document/log UX | P1 | RESEARCHED | Pending: unit + scoped acceptance | Not yet delivered |
| Logs | Pause/resume/clear, severity filter, markers, ANSI handling | Bounded log state and sanitized text | P2 | RESEARCHED | Pending: unit + scoped acceptance | Not yet delivered |
| Documents | Live YAML, describe, Secret decode | Redacted native documents; reveal separately gated | P0 | DESIGNED | Pending: unit + scoped acceptance | Not yet delivered |
| Documents | Search/highlight/page/wrap/horizontal/fullscreen/refresh | Reusable document state | P1 | DESIGNED | Pending: unit + scoped acceptance | Not yet delivered |
| Documents | Last-applied or session-baseline diff/reset baseline | Explicit comparison baseline | P2 | RESEARCHED | Pending: unit + scoped acceptance | Not yet delivered |
| Actions | Exec/shell/attach with terminal suspension | Native transport after policy gate | P1 | RESEARCHED | Pending: unit + scoped acceptance | Not yet delivered |
| Actions | EDITOR edit with pinned context | Temporary file + validated API update | P1 | RESEARCHED | Pending: unit + scoped acceptance | Not yet delivered |
| Actions | Delete/force/bulk/cascade options, confirmations | UID/RV preconditions and policy | P1 | DESIGNED | Pending: unit + scoped acceptance | Not yet delivered |
| Actions | Scale including discovered scale subresource | Native scale API | P1 | RESEARCHED | Pending: unit + scoped acceptance | Not yet delivered |
| Actions | Rollout restart / set image | Typed patch preview/service | P1 | DESIGNED | Pending: unit + scoped acceptance | Not yet delivered |
| Actions | CronJob trigger/suspend/resume | Native Job creation / suspend patch | P2 | RESEARCHED | Pending: unit + scoped acceptance | Not yet delivered |
| Actions | ExternalSecret/PushSecret refresh | Documented reconcile annotation | P2 | RESEARCHED | Pending: unit + scoped acceptance | Not yet delivered |
| Nodes | Cordon/uncordon | Native patch with preview | P1 | DESIGNED | Pending: unit + scoped acceptance | Not yet delivered |
| Nodes | Drain options, sequential nodes, PDB retry, progress/cancel | Dedicated eviction state machine | P1 | DESIGNED | Pending: unit + scoped acceptance | Upstream safety/features conflict; follow current API semantics |
| Forwarding | Pod/service declared port picker, custom/local port edits | Loopback native port-forward manager | P1 | DESIGNED | Pending: unit + scoped acceptance | Not yet delivered |
| Forwarding | Background forwards, indicators, conflict/stop/saved/autostart | Owned forward tasks; explicit startup policy | P1 | RESEARCHED | Pending: unit + scoped acceptance | Not yet delivered |
| Files | Pod upload/download, progress | Bounded transfer with path checks and policy | P2 | RESEARCHED | Pending: unit + scoped acceptance | Not yet delivered |
| Files | PVC two-pane browser, mounted Pod/helper, cleanup, confined paths | Deferred until exec/transfer lifecycle proven | P2 | DEFERRED | Pending: unit + scoped acceptance | Not yet delivered |
| Debug | Ephemeral container, target container | Explicit irreversible debug action | P2 | RESEARCHED | Pending: unit + scoped acceptance | Not yet delivered |
| Debug | Privileged node debug pod and cleanup | High-risk preview and session ownership checks | P2 | RESEARCHED | Pending: unit + scoped acceptance | Not yet delivered |
| Flux | Kustomizations/HelmReleases/Git/Helm/OCI/Bucket resources | Native generic + curated GitOps views | P2 | RESEARCHED | Pending: unit + scoped acceptance | Not yet delivered |
| Flux | Image automation/notifications, suspend/resume/reconcile | Per-GVK native actions | P2 | RESEARCHED | Pending: unit + scoped acceptance | Not yet delivered |
| Flux | HelmRelease force reconcile / release history navigation | Controller-specific annotations and release resolution | P2 | RESEARCHED | Pending: unit + scoped acceptance | Not yet delivered |
| Flux | Ownership/source/dependency chain, refresh and UID checks | GitOps evidence graph | P2 | RESEARCHED | Pending: unit + scoped acceptance | Not yet delivered |
| Argo | Applications status/sources/revisions/health/managed resources | Native CRD inspector | P2 | RESEARCHED | Pending: unit + scoped acceptance | Not yet delivered |
| Argo | Application sync, suspend/resume with exact original restoration | Explicit policy preview / not a native suspended field | P2 | RESEARCHED | Pending: unit + scoped acceptance | Not yet delivered |
| Argo | ApplicationSet generators/children and create-only suspend | Document limited suspension semantics | P2 | RESEARCHED | Pending: unit + scoped acceptance | Not yet delivered |
| Argo | Tracking metadata, multiple installations, remote destinations | Unambiguous context/ownership resolution | P2 | RESEARCHED | Pending: unit + scoped acceptance | Not yet delivered |
| Helm | Native release/revision/history/values/manifest/NOTES | Bounded Secret/ConfigMap decoder | P2 | RESEARCHED | Pending: unit + scoped acceptance | Not yet delivered |
| Helm | Rollback/uninstall (Sofka uses helm executable) | Native feasibility study, no fake patch-only rollback | P2 | DEFERRED | Pending: unit + scoped acceptance | Not yet delivered |
| Relationships | Owners/children/Pod node/config/Secret/PVC/SA | Verified UID edges / explicit refs | P1 | DESIGNED | Pending: unit + scoped acceptance | Not yet delivered |
| Relationships | PVC/PV/StorageClass/volume attributes class/reverse mounts | Storage relationship rules | P1 | RESEARCHED | Pending: unit + scoped acceptance | Not yet delivered |
| Relationships | Service selector/Endpoints/Ingress backends/TLS | Label-selection edges marked as inference | P1 | RESEARCHED | Pending: unit + scoped acceptance | Not yet delivered |
| Relationships | Configurable CRD children/refs/kind and namespace paths | Validated declarative graph rules | P2 | RESEARCHED | Pending: unit + scoped acceptance | Not yet delivered |
| Relationships | On-demand generic CRD children search with budgets | Paged bounded owner-UID discovery | P2 | RESEARCHED | Pending: unit + scoped acceptance | Not yet delivered |
| Relationships | Xray ownership hierarchy | Graph traversal view | P1 | DESIGNED | Pending: unit + scoped acceptance | Not yet delivered |
| Overview | Pulse refreshed health tiles | Bounded asynchronous overview | P1 | DESIGNED | Pending: unit + scoped acceptance | Not yet delivered |
| Safety | Readonly global/context/cluster and flags | Central monotonic policy; explicit opt-in write later | P0 | DESIGNED | Pending: deny/conflict/outcome regression | Not yet delivered |
| Safety | Guardrails deny/confirmation/type context/type name/bulk limits | Combine all restrictions deterministically | P0 | DESIGNED | Pending: deny/conflict/outcome regression | Not yet delivered |
| Safety | Managed object warnings | Evidence of controlling manager in preview | P1 | RESEARCHED | Pending: deny/conflict/outcome regression | Not yet delivered |
| Safety | can-i rules and action review; partial authorizers | SSAR + explicit incomplete reviews | P1 | RESEARCHED | Pending: deny/conflict/outcome regression | Not yet delivered |
| Safety | Action journal and optional rotated export | Started + completed/failed/uncertain outcomes | P1 | DESIGNED | Pending: deny/conflict/outcome regression | Not yet delivered |
| Commands | Fuzzy palette/resources/bookmarks/workspaces/plugins | Central command registry | P0 | DESIGNED | Pending: unit + scoped acceptance | Not yet delivered |
| Commands | Scope/resource/filter syntax and @context completion | Structured query grammar | P1 | DESIGNED | Pending: unit + scoped acceptance | Not yet delivered |
| Commands | Per-mode rebindings, disable/conflicts/effective help | Central compiled keymap | P1 | DESIGNED | Pending: unit + scoped acceptance | Not yet delivered |
| Mouse | Rows/wheel/header sort; release for native text selection | Supplementary mouse modes | P2 | RESEARCHED | Pending: unit + scoped acceptance | Not yet delivered |
| Configuration | XDG base/drop-ins/cluster/context/view layers | TOML first, collision-free exact keys | P1 | DESIGNED | Pending: unit + scoped acceptance | Not yet delivered |
| Configuration | TOML/YAML, reload/validation, migration/Home Manager | TOML first; YAML/Nix deferred | P2 | DEFERRED | Pending: unit + scoped acceptance | Not yet delivered |
| Themes | Dark/light palettes, semantic colors, live override | Original semantic themes | P1 | DESIGNED | Pending: unit + scoped acceptance | Not yet delivered |
| Sessions | Bookmarks/workspaces/view cycling/persistent namespaces | Saved structured queries | P2 | RESEARCHED | Pending: unit + scoped acceptance | Not yet delivered |
| Notifications | Independent single-resource watches, bell/desktop | Bounded notification subscriptions | P2 | RESEARCHED | Pending: unit + scoped acceptance | Not yet delivered |
| Diagnostics | Redacted incident bundle with manifest/limits/preview/export | Local evidence bundle | P1 | DESIGNED | Pending: unit + scoped acceptance | Not yet delivered |
| Diagnostics | Interactive text/JSON/YAML snapshots, browse/delete | Explicit local export inventory | P2 | RESEARCHED | Pending: unit + scoped acceptance | Not yet delivered |
| Headless | --check, --snapshot, info, info --offline | CLI with deterministic non-TTY output | P0 | DESIGNED | Pending: unit + scoped acceptance | Not yet delivered |
| Observability | Tracing/redaction, request latency, watch/reconnect counts | Safe event metadata and counters | P0 | DESIGNED | Pending: unit + scoped acceptance | Not yet delivered |
| Compatibility | TLS resumption flag, v1 cert opt-in, Teleport cert workaround | Standard TLS first; exceptions deferred pending own tests | P2 | DEFERRED | Pending: unit + scoped acceptance | Not yet delivered |
| Compatibility | HTTP proxy/NO_PROXY semantics | kube supported proxy behavior; test separately | P1 | RESEARCHED | Pending: unit + scoped acceptance | Not yet delivered |
| Providers | Prometheus/VictoriaMetrics rightsize preview | Opt-in historical metrics adapter | P3 | DEFERRED | Pending: unit + scoped acceptance | Not yet delivered |
| Providers | VictoriaLogs autodiscovery/history/tail/field detection | Opt-in historical logs adapter | P3 | DEFERRED | Pending: unit + scoped acceptance | Not yet delivered |
| Fleet | Opt-in cross-context summary, saved membership, Argo drill | Read-only bounded multi-cluster Eye | P3 | DEFERRED | Pending: unit + scoped acceptance | Not yet delivered |
| Plugins | Inline commands/placeholders/bulk/output/trust/timeouts | Structured external command contract | P3 | DEFERRED | Pending: unit + scoped acceptance | Not yet delivered |
| Plugins | Package commands/typed inputs/JSON reports/managed forwards | Versioned adapter protocol | P3 | DEFERRED | Pending: unit + scoped acceptance | Not yet delivered |
| Plugins | Activity panel/stderr tail/process-group cancellation | Owned subprocess tasks | P3 | DEFERRED | Pending: unit + scoped acceptance | Not yet delivered |
| Plugins | Catalog search/describe/install/update/rollback/offline/checksums/withdrawal/remove | Explicit future registry design | P3 | DEFERRED | Pending: unit + scoped acceptance | Not yet delivered |
| Plugins | Bundled sanitize; external Popeye/Trivy catalog integrations | Extensions after safety and process controls | P3 | DEFERRED | Pending: unit + scoped acceptance | Not yet delivered |
| Distribution | Linux/macOS x86_64/aarch64, Cargo/Homebrew/Nix | Build and release automation | P2 | DEFERRED | Pending: unit + scoped acceptance | Not yet delivered |
| Performance | Published startup/filter/view/RSS methodology | Independent reproducible measured harness | P0 | DESIGNED | Pending: unit + scoped acceptance | Not yet delivered |
| SAURON | Eye problem-priority overview (mission addition) | Evidence-ranked current-context view | P1 | DESIGNED | Pending: unit + scoped acceptance | Mission addition; no exclusivity claim |
| SAURON | Blast radius, safety lens, change preview | Labeled inference + typed patch policy | P1 | DESIGNED | Pending: unit + scoped acceptance | Mission addition; no exclusivity claim |
| SAURON | Context diff, navigation replay, explainable score | Read-only comparison/replay; scoring optional | P3 | DEFERRED | Pending: unit + scoped acceptance | Mission addition; no exclusivity claim |

## Current gap review — M3 start, 2026-09-15

M1 and M2 accepted checkpoints exist; navigation is not a gap. Basic live watch/store,
curated projections, YAML/describe/Explain/Events/logs, CLI snapshots, keymap and config
are implemented (see HANDBOOK/RUNBOOK for actual live scope). Remaining composite rows
above describe full parity: DESIGNED does not imply every sub-capability is absent.
Full M3 filtering/sorting/documents/Events/Tables acceptance is still outstanding.
Highest risks: selector history widening, missing values coerced to zero, stale async
completion and cells on replaced objects. Prioritize M3_ACCEPTANCE.md in order; no
comparative performance claims or later-milestone expansion.
