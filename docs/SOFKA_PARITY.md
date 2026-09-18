# Sofka parity matrix

Baseline: Sofka 0.27.3, commit `2024e5921cf9cb06fd02f666630f97bae5e24eb3`, inspected 2026-09-15.
Sources and discrepancies: [research](RESEARCH.md). Each row is a tracked workflow;
combined terms list explicit sub-capabilities. Research is not acceptance. P0 foundation,
P1 operational core, P2 breadth, P3 integrations. Tests listed here are acceptance plans
until an actual test/run is linked. Historical roadmap items are not claimed as shipped.

SAURON status reconciled 2026-09-17: M1–M5 ACCEPTED; local annotated
`m5-accepted`. M5 added evidence-driven metrics (docs/METRICS.md), deterministic
health (docs/HEALTH.md), Explain 2.0 (docs/EXPLAIN.md) and a UID-scoped Timeline
(docs/TIMELINE.md), plus a 12-sequence combined pass and a 75-minute soak (zero
failures) in [M5_ACCEPTANCE.md](M5_ACCEPTANCE.md). M4 evidence and retained
limitations: [M4_ACCEPTANCE.md](M4_ACCEPTANCE.md). The Sofka research baseline
above is unchanged.

| Area | Sofka capability | SAURON equivalent | Priority | Status | Test | Notes |
| ---- | ---------------- | ----------------- | -------- | ------ | ---- | ----- |
| Navigation | Kubeconfig / exec credentials / explicit context | Native config/client | P0 | ACCEPTED | M1/M2 live kind; RUNBOOK | Credential plugins delegated to kube; not independently acceptance-tested |
| Navigation | Context picker, switching, remembered namespaces | Epoch-scoped context navigation | P0 | ACCEPTED | M2 items 1/6 live | Session-local memory |
| Navigation | Namespace picker, all namespaces, active/default labels | Namespace scope | P0 | ACCEPTED | M2 items 2/6 live | Restricted-list fallback is still a gap |
| Navigation | Favorites 1–9, recents, selected-row namespace | Config favorites/session history | P1 | RESEARCHED | Pending: unit + scoped acceptance | Not yet delivered |
| Navigation | Shortnames, aliases, API groups, precedence | Discovered GVR catalog and aliases | P0 | ACCEPTED | M2 item 3 live + resolve regressions | M3 tightens selector/history identity boundaries |
| Navigation | CRDs, custom resources, arbitrary discovered kinds | Dynamic resource watch/render | P0 | ACCEPTED | M2 item 3/6 and M3 item 5 live | Safe printer-column subset accepted; Table negotiation deferred |
| Navigation | Drill-down, breadcrumbs, history, resource cycling | Bounded back/forward + breadcrumbs | P1 | IMPLEMENTING | M2 item 4/6 accepted live | Drill-down/resource cycling not delivered; M3 adds selector history |
| Navigation | Wide columns, horizontal scroll with pinned identity | Typed column layout | P1 | RESEARCHED | Pending: unit + scoped acceptance | Not yet delivered |
| Navigation | Compact mode, hidden header, terminal title | Responsive terminal chrome | P1 | DESIGNED | Pending: unit + scoped acceptance | Not yet delivered |
| Navigation | Global fuzzy object finder with partial RBAC results | Bounded cross-kind finder | P1 | RESEARCHED | Pending: unit + scoped acceptance | Not yet delivered |
| Navigation | Name/cell clipboard and OSC52 fallback | Explicit clipboard actions | P2 | RESEARCHED | Pending: unit + scoped acceptance | Not yet delivered |
| Live data | Watches, relists, reconnects/backoff, expired RV | kube watcher with bounded messages/staged store | P0 | TESTED | watch_transport.rs; M1 live watch; M4.4 75-minute soak | Broader failure/scale campaign remains; soak scope in M4_ACCEPTANCE.md |
| Live data | UID selection, replacement detection, generation tags | UID identity / view epoch | P0 | ACCEPTED | M1–M3 live replacement/rapid navigation | Runtime stores canonical resource identity |
| Live data | Skipped API groups, RBAC errors, partial discovery | Per-group errors and incomplete state | P0 | TESTED | Fake HTTP discovery extension 403 | Complete restricted-RBAC live campaign remains |
| Live data | Cached row projections and batching | Incremental projections / capped redraw cadence | P0 | TESTED | prepare-cache regressions; pipeline bench | No comparative performance claim |
| Views | Pods curated columns | GVK-qualified projection | P0 | FUNCTIONAL (subset) | src/resources/mod.rs; M1 baseline | Basic projection exists; full per-kind Sofka parity not accepted |
| Views | Deployments curated columns | GVK-qualified projection | P0 | FUNCTIONAL (subset) | src/resources/mod.rs; M1 baseline | Basic projection exists; full per-kind Sofka parity not accepted |
| Views | StatefulSets curated columns | GVK-qualified projection | P0 | FUNCTIONAL (subset) | src/resources/mod.rs; M1 baseline | Basic projection exists; full per-kind Sofka parity not accepted |
| Views | DaemonSets curated columns | GVK-qualified projection | P0 | FUNCTIONAL (subset) | src/resources/mod.rs; M1 baseline | Basic projection exists; full per-kind Sofka parity not accepted |
| Views | ReplicaSets curated columns | GVK-qualified projection | P0 | FUNCTIONAL (subset) | src/resources/mod.rs; M1 baseline | Basic projection exists; full per-kind Sofka parity not accepted |
| Views | Services curated columns | GVK-qualified projection | P0 | FUNCTIONAL (subset) | src/resources/mod.rs; M1 baseline | Basic projection exists; full per-kind Sofka parity not accepted |
| Views | Nodes curated columns | GVK-qualified projection | P0 | FUNCTIONAL (subset) | src/resources/mod.rs; M1 baseline | Basic projection exists; full per-kind Sofka parity not accepted |
| Views | Namespaces curated columns | GVK-qualified projection | P0 | DESIGNED | Pending: unit + scoped acceptance | Not yet delivered |
| Views | ConfigMaps curated columns | GVK-qualified projection | P0 | FUNCTIONAL (subset) | src/resources/mod.rs; M1 baseline | Basic projection exists; full per-kind Sofka parity not accepted |
| Views | Secrets curated columns | GVK-qualified projection | P0 | FUNCTIONAL (subset) | src/resources/mod.rs; M1 baseline | Basic projection exists; full per-kind Sofka parity not accepted |
| Views | Jobs curated columns | GVK-qualified projection | P0 | FUNCTIONAL (subset) | src/resources/mod.rs; M1 baseline | Basic projection exists; full per-kind Sofka parity not accepted |
| Views | CronJobs curated columns | GVK-qualified projection | P0 | FUNCTIONAL (subset) | src/resources/mod.rs; M1 baseline | Basic projection exists; full per-kind Sofka parity not accepted |
| Views | PVCs curated columns | GVK-qualified projection | P0 | FUNCTIONAL (subset) | src/resources/mod.rs; M1 baseline | Basic projection exists; full per-kind Sofka parity not accepted |
| Views | PVs curated columns | GVK-qualified projection | P0 | FUNCTIONAL (subset) | src/resources/mod.rs; M1 baseline | Basic projection exists; full per-kind Sofka parity not accepted |
| Views | Ingresses curated columns | GVK-qualified projection | P0 | FUNCTIONAL (subset) | src/resources/mod.rs; M1 baseline | Basic projection exists; full per-kind Sofka parity not accepted |
| Views | Endpoints curated columns | GVK-qualified projection | P0 | FUNCTIONAL (subset) | src/resources/mod.rs; M1 baseline | Basic projection exists; full per-kind Sofka parity not accepted |
| Views | CRDs curated columns | GVK-qualified projection | P0 | FUNCTIONAL (subset) | src/resources/mod.rs; M1 baseline | Basic projection exists; full per-kind Sofka parity not accepted |
| Views | ApplicationSets status/generators/apps | Argo-specific projection | P2 | RESEARCHED | Pending: unit + scoped acceptance | Not yet delivered |
| Views | Pod containers: regular/init/sidecar/ephemeral, probes/ports/images | Container inspection | P1 | RESEARCHED | Pending: unit + scoped acceptance | Not yet delivered |
| Views | Container CPU/memory five-minute trends | Bounded per-container sample ring | P2 | RESEARCHED | Pending: unit + scoped acceptance | Not yet delivered |
| Views | CRD printer columns including condition selectors | Validated dotted-field/numeric-index subset | P1 | ACCEPTED (subset) | M3 item 5 fake HTTP + live combined acceptance | Condition selectors and full JSONPath unavailable |
| Views | Server Tables, watch/poll fallback, UID/RV cell validity | Negotiated Table adapter | P1 | DEFERRED | M3 item 5 research | Live object CRD projection implemented instead |
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
| Events | UID-related events, Warning filtering, correct last-seen | Native Event view | P1 | ACCEPTED | M3 item 4 + combined live flows | 200-result cap, explicit partial/empty/error; related-object navigation deferred |
| Logs | Follow/tail/timestamps/previous/container selection | Cancellable native log stream | P1 | ACCEPTED | M4.0/M4.1 live; LOGS.md | Fixed tail 300; regular/init/ephemeral selection |
| Logs | Multi-container/workload/service/marked-pod combined logs | Bounded multiplexed log sources | P1 | ACCEPTED (subset) | M4.1 live, repeated twice | All containers / visible Pods; workload/Service/marked aggregation deferred |
| Logs | Filter/search/wrap/fullscreen/copy/save/time anchors | Shared document/log UX | P1 | ACCEPTED (subset) | M4.1 live | Search/filter/wrap/fullscreen; copy/save/time anchors deferred |
| Logs | Pause/resume/clear, severity filter, markers, ANSI handling | Bounded log state and sanitized text | P2 | ACCEPTED (subset) | M4.1 live eviction/pause/clear/sanitization | Pause freezes display, not bounded ingestion; severity/markers deferred |
| Documents | Live YAML, describe, Secret decode | Redacted native documents; no Secret reveal | P0 | ACCEPTED (redacted subset) | M3 item 3 live | Native describe is contextual, not kubectl parity; decode deferred |
| Documents | Search/highlight/page/wrap/horizontal/fullscreen/refresh | Reusable document state | P1 | ACCEPTED | M3 item 3 tests + live deletion/replacement/resize/search | UID-pinned refresh; logs have separate lifetime |
| Documents | Last-applied or session-baseline diff/reset baseline | Explicit comparison baseline | P2 | RESEARCHED | Pending: unit + scoped acceptance | Not yet delivered |
| Actions | Exec/shell/attach with terminal suspension | Native transport after readonly gate | P1 | ACCEPTED | M4.2/M4.2b live; EXEC.md | Ctrl-] detach; documented post-session stdin limitation; richer operation policy deferred |
| Actions | EDITOR edit with pinned context | Temporary file + validated API update | P1 | RESEARCHED | Pending: unit + scoped acceptance | Not yet delivered |
| Actions | Delete/force/bulk/cascade options, confirmations | UID/RV preconditions and policy | P1 | DESIGNED | Pending: unit + scoped acceptance | Not yet delivered |
| Actions | Scale including discovered scale subresource | Native scale API | P1 | RESEARCHED | Pending: unit + scoped acceptance | Not yet delivered |
| Actions | Rollout restart / set image | Typed patch preview/service | P1 | DESIGNED | Pending: unit + scoped acceptance | Not yet delivered |
| Actions | CronJob trigger/suspend/resume | Native Job creation / suspend patch | P2 | RESEARCHED | Pending: unit + scoped acceptance | Not yet delivered |
| Actions | ExternalSecret/PushSecret refresh | Documented reconcile annotation | P2 | RESEARCHED | Pending: unit + scoped acceptance | Not yet delivered |
| Nodes | Cordon/uncordon | Native patch with preview | P1 | DESIGNED | Pending: unit + scoped acceptance | Not yet delivered |
| Nodes | Drain options, sequential nodes, PDB retry, progress/cancel | Dedicated eviction state machine | P1 | DESIGNED | Pending: unit + scoped acceptance | Upstream safety/features conflict; follow current API semantics |
| Forwarding | Pod/service declared port picker, custom/local port edits | Loopback native port-forward manager | P1 | ACCEPTED (subset) | M4.3 live TCP/picker/auto/explicit/conflict; PORT_FORWARD.md | Pod only; Service resolution and editing existing forwards deferred |
| Forwarding | Background forwards, indicators, conflict/stop/saved/autostart | Owned forward tasks; explicit startup policy | P1 | ACCEPTED (subset) | M4.3 live context/navigation/UID/cleanup/cycles; M4.4 combined flows + continuously-held soak forward | 4 forwards × 8 clients; saved/autostart/reconnect deferred |
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
| Safety | Readonly global/context/cluster and flags | Layered config + hard CLI override at operation boundary | P0 | TESTED (subset) | M4 exec/attach/forward denial; reload regression + live | Full M7 guardrail/mutation service not implemented |
| Safety | Guardrails deny/confirmation/type context/type name/bulk limits | Combine all restrictions deterministically | P0 | DESIGNED | Pending: deny/conflict/outcome regression | Not yet delivered |
| Safety | Managed object warnings | Evidence of controlling manager in preview | P1 | RESEARCHED | Pending: deny/conflict/outcome regression | Not yet delivered |
| Safety | can-i rules and action review; partial authorizers | SSAR + explicit incomplete reviews | P1 | RESEARCHED | Pending: deny/conflict/outcome regression | Not yet delivered |
| Safety | Action journal and optional rotated export | Started + completed/failed/uncertain outcomes | P1 | DESIGNED | Pending: deny/conflict/outcome regression | Not yet delivered |
| Commands | Fuzzy palette/resources/bookmarks/workspaces/plugins | Central command registry | P0 | ACCEPTED (subset) | M2 item 5/6 live | Commands/resources only; bookmarks/workspaces/plugins deferred |
| Commands | Scope/resource/filter syntax and @context completion | Structured query grammar | P1 | ACCEPTED (subset) | M2/M3 grammar + live | Scope/filter commands; @context completion deferred |
| Commands | Per-mode rebindings, disable/conflicts/effective help | Central compiled keymap | P1 | ACCEPTED | M2 remap/help live + conflict regressions | M4 adds log and forward-manager modes |
| Mouse | Rows/wheel/header sort; release for native text selection | Supplementary mouse modes | P2 | RESEARCHED | Pending: unit + scoped acceptance | Not yet delivered |
| Configuration | XDG base/drop-ins/cluster/context/view layers | TOML first, collision-free exact keys | P1 | DESIGNED | Pending: unit + scoped acceptance | Not yet delivered |
| Configuration | TOML/YAML, reload/validation, migration/Home Manager | TOML first; YAML/Nix deferred | P2 | DEFERRED | Pending: unit + scoped acceptance | Not yet delivered |
| Themes | Dark/light palettes, semantic colors, live override | Original semantic themes | P1 | DESIGNED | Pending: unit + scoped acceptance | Not yet delivered |
| Sessions | Bookmarks/workspaces/view cycling/persistent namespaces | Saved structured queries | P2 | RESEARCHED | Pending: unit + scoped acceptance | Not yet delivered |
| Notifications | Independent single-resource watches, bell/desktop | Bounded notification subscriptions | P2 | RESEARCHED | Pending: unit + scoped acceptance | Not yet delivered |
| Diagnostics | Redacted incident bundle with manifest/limits/preview/export | Local evidence bundle | P1 | DESIGNED | Pending: unit + scoped acceptance | Not yet delivered |
| Diagnostics | Interactive text/JSON/YAML snapshots, browse/delete | Explicit local export inventory | P2 | RESEARCHED | Pending: unit + scoped acceptance | Not yet delivered |
| Headless | --check, --snapshot, info, info --offline | CLI with deterministic non-TTY output | P0 | TESTED | M1 baseline + M3 typed-filter live snapshots | No TTY needed |
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
| Performance | Published startup/filter/view/RSS methodology | Pipeline benchmark and session soak | P0 | TESTED (subset) | benches/pipeline.rs; M4.4 75-minute RSS/fd/thread observations in M4_ACCEPTANCE.md | No comparative performance claim; startup/large-cluster campaign remains |
| SAURON | Eye problem-priority overview (mission addition) | Evidence-ranked current-context view | P1 | DESIGNED | Pending: unit + scoped acceptance | Mission addition; no exclusivity claim |
| SAURON | Blast radius, safety lens, change preview | Labeled inference + typed patch policy | P1 | DESIGNED | Pending: unit + scoped acceptance | Mission addition; no exclusivity claim |
| SAURON | Context diff, navigation replay, explainable score | Read-only comparison/replay; scoring optional | P3 | DEFERRED | Pending: unit + scoped acceptance | Mission addition; no exclusivity claim |

## Current gap review — M5 accepted, 2026-09-17

M6 implementation started 2026-09-18: scoped graph identity/provenance/bounds
foundation only. Adjacent, Xray and relationship transport are not yet implemented
or accepted. Per-slice evidence and continuation: [M6_ACCEPTANCE.md](M6_ACCEPTANCE.md).

M1–M4 accepted checkpoints exist; core navigation is not a gap. Basic live watch/store,
curated projections, YAML/describe/Explain/Events/logs, CLI snapshots, keymap and config
are implemented (see HANDBOOK/RUNBOOK for actual live scope). Remaining composite rows
above describe full parity: DESIGNED does not imply every sub-capability is absent.
M3 filtering/sorting/documents/Events/CRD projection are accepted; server Table
negotiation is deliberately deferred, not accepted. M4.0–M4.4 are ACCEPTED against
isolated kind, including ten combined adversarial sequences and regression of prior
milestones. Recorded checks: 76 unit + 14 fake HTTP; fmt/check/clippy clean.

Recorded soak: 75 minutes, 912 cycles, 911 log sessions, one continuously-held forward;
RSS 29000→29488 KiB, fds 17→18, threads stable at 4. One transient timeout recovered
without restarting the run. These are bounded observations from that run, not a general
proof of leak freedom or a comparative performance claim. See [M4 acceptance](M4_ACCEPTANCE.md)
for the evidence and the Escape/Alt input ambiguity fixed in the test harness.

M5.0–M5.6 are ACCEPTED against isolated kind with a pinned metrics-server
fixture: evidence/freshness primitives, an optional bounded Metrics API
collector, static accounting plus live usage/percentages threaded into
table/filter/sort, deterministic Pod/workload/Node/storage health with cited
evidence, Explain 2.0 (reusing that health evidence, never a parallel
diagnosis, plus verified-ownership workload→Pod correlation and descriptive
metrics), a UID-scoped Timeline with correct relist-diff semantics, and a
12-sequence combined adversarial pass plus a 75-minute soak (1800 cycles,
zero failures). Recorded checks: 105 unit + 19 fake HTTP; fmt/check/clippy
clean. See [M5 acceptance](M5_ACCEPTANCE.md) and docs/METRICS.md,
docs/HEALTH.md, docs/EXPLAIN.md, docs/TIMELINE.md for the per-slice contracts.

Remaining operational gaps include Service/workload log resolution, Service forwards,
saved/reconnecting/autostart forwards, IPv6/public binding and fully cancellation-safe
terminal stdin. The bounded post-shell/attach input loss remains documented in
[EXEC.md](EXEC.md); M4 acceptance does not claim it is fully fixed. The full
relationship graph, Xray/blast-radius, and the mutation/guardrail policy remain
M6/M7 work; M5 explicitly stayed read-only and did not build toward the graph
beyond Explain's own bounded, verified-ownership two-hop Pod correlation.
