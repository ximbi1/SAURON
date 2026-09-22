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
| Metrics | Pod/node/container CPU and memory | Optional Metrics API collector | P1 | ACCEPTED | M5.1/M5.6 unit, fake HTTP and real metrics-server | Optional sampled metrics; stale/absent remain UNKNOWN |
| Metrics | Requests/limits, percentages, allocatable, QoS | Quantity/accounting projection | P1 | ACCEPTED | M5.2 accounting and live filter/sort/table acceptance | Static accounting and usage percentages; zero denominator remains unknown |
| Metrics | Threshold colors by resource/context | Semantic threshold configuration | P2 | RESEARCHED | Pending: unit + scoped acceptance | Not yet delivered |
| Health | Pod phase/reason/init/sidecar/gate/failure precedence | Deterministic Pod state model | P0 | ACCEPTED (M5 scope) | M5.3 real failure fixtures and precedence tests | See HEALTH.md for supported per-kind semantics |
| Health | Workload rollout/observedGeneration/partition/OnDelete | Workload health rules | P1 | ACCEPTED (M5 scope) | M5.3 real failure fixtures and precedence tests | See HEALTH.md for supported per-kind semantics |
| Health | Jobs, storage deletion, Node condition polarity | GVK health rules | P1 | ACCEPTED (M5 scope) | M5.3 real failure fixtures and precedence tests | See HEALTH.md for supported per-kind semantics |
| Explain | Fresh selected object, UID checks, cancellable latest report | Evidence collection + pure rules | P0 | ACCEPTED | M5.4/M5.6 UID, partial RBAC and context races | Bounded fresh-object evidence; no invented diagnosis |
| Explain | Workload/Pod/container/event evidence, finding navigation | Evidence report and related targets | P1 | ACCEPTED (subset) | M5.4 bounded ownership/Event/metric evidence | Report accepted; full finding-target navigation not claimed |
| Timeline | Bounded per-UID meaningful watch transitions | Session change ring | P1 | ACCEPTED | M5.5/M5.6 relist, UID and meaningful-change tests | Bounded, scope/session-local; not an audit log |
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
| Actions | Delete/force/bulk/cascade options, confirmations | UID/RV preconditions and policy | P1 | ACCEPTED (subset) | M8 delete; M8B force delete; live combined flows | Single-object guarded actions; bulk/cascade-option parity not claimed |
| Actions | Scale including discovered scale subresource | Native scale API | P1 | ACCEPTED (subset) | M8 scale workflow and live verification | Supported workload scaling; arbitrary discovered scale-subresource parity not claimed |
| Actions | Rollout restart / set image | Typed patch preview/service | P1 | ACCEPTED | M8 restart / M8B set-image live acceptance | Typed preview, policy, confirmation, commit and separate verification |
| Actions | CronJob trigger/suspend/resume | Native Job creation / suspend patch | P2 | ACCEPTED (subset) | M8B trigger live acceptance | Trigger implemented; CronJob suspend/resume parity not claimed |
| Actions | ExternalSecret/PushSecret refresh | Documented reconcile annotation | P2 | RESEARCHED | Pending: unit + scoped acceptance | Not yet delivered |
| Nodes | Cordon/uncordon | Native patch with preview | P1 | ACCEPTED | M8B guarded cordon/uncordon live acceptance | Isolated test cluster only during development |
| Nodes | Drain options, sequential nodes, PDB retry, progress/cancel | Dedicated eviction state machine | P1 | ACCEPTED (subset) | M8B drain planner/orchestrator and live cancellation | Bounded sequential evictions through gateway; not full kubectl drain/options parity |
| Forwarding | Pod/service declared port picker, custom/local port edits | Loopback native port-forward manager | P1 | ACCEPTED (subset) | M4.3 live TCP/picker/auto/explicit/conflict; PORT_FORWARD.md | Pod only; Service resolution and editing existing forwards deferred |
| Forwarding | Background forwards, indicators, conflict/stop/saved/autostart | Owned forward tasks; explicit startup policy | P1 | ACCEPTED (subset) | M4.3 live context/navigation/UID/cleanup/cycles; M4.4 combined flows + continuously-held soak forward | 4 forwards × 8 clients; saved/autostart/reconnect deferred |
| Files | Pod upload/download, progress | Bounded transfer with path checks and policy | P2 | RESEARCHED | Pending: unit + scoped acceptance | Not yet delivered |
| Files | PVC two-pane browser, mounted Pod/helper, cleanup, confined paths | Deferred until exec/transfer lifecycle proven | P2 | DEFERRED | Pending: unit + scoped acceptance | Not yet delivered |
| Debug | Ephemeral container, target container | Explicit irreversible debug action | P2 | RESEARCHED | Pending: unit + scoped acceptance | Not yet delivered |
| Debug | Privileged node debug pod and cleanup | High-risk preview and session ownership checks | P2 | RESEARCHED | Pending: unit + scoped acceptance | Not yet delivered |
| Flux | Kustomizations/HelmReleases/Git/Helm/OCI/Bucket resources | Native generic + curated GitOps views | P2 | ACCEPTED (M9 scope) | M9.1 real Flux v2.9.5 | Capability discovery and curated supported status reports |
| Flux | Image automation/notifications, suspend/resume/reconcile | Per-GVK native actions | P2 | ACCEPTED (subset) | M9.0 discovery / M9.2 guarded actions | Supported per-kind suspend/resume/reconcile; full optional-controller workflow parity not claimed |
| Flux | HelmRelease force reconcile / release history navigation | Controller-specific annotations and release resolution | P2 | RESEARCHED | Pending: unit + scoped acceptance | Not yet delivered |
| Flux | Ownership/source/dependency chain, refresh and UID checks | GitOps evidence graph | P2 | ACCEPTED (subset) | M9.1 source/dependency status evidence | Read inspection only; full interactive GitOps graph parity not claimed |
| Argo | Applications status/sources/revisions/health/managed resources | Native CRD inspector | P2 | ACCEPTED (M9 scope) | M9.3 real Argo CD v3.5.3 | Application status inspection; exact supported fields in M9_ACCEPTANCE.md |
| Argo | Application sync, suspend/resume with exact original restoration | Explicit policy preview / not a native suspended field | P2 | ACCEPTED (subset) | M9.4 sync/refresh/rollback via shared gateway | Suspend/resume restoration semantics not implemented by these actions |
| Argo | ApplicationSet generators/children and create-only suspend | Document limited suspension semantics | P2 | RESEARCHED | Pending: unit + scoped acceptance | Not yet delivered |
| Argo | Tracking metadata, multiple installations, remote destinations | Unambiguous context/ownership resolution | P2 | RESEARCHED | Pending: unit + scoped acceptance | Not yet delivered |
| Helm | Native release/revision/history/values/manifest/NOTES | Bounded Secret/ConfigMap decoder | P2 | ACCEPTED (subset) | M9.5 native Secret-backed release decoding, fake/live tests | Selected revision, masked values, manifest identities only; NOTES omitted; full history/ConfigMap-driver parity not claimed |
| Helm | Rollback/uninstall (Sofka uses helm executable) | Native feasibility study, no fake patch-only rollback | P2 | DEFERRED | M9.6 native-action feasibility investigation | No safe supported execution path chosen; no rollback/uninstall implementation |
| Relationships | Owners/children/Pod node/config/Secret/PVC/SA | Verified UID edges / explicit refs | P1 | ACCEPTED | M6 combined live acceptance and soak | Bounded Adjacent with provenance and UID-safe navigation |
| Relationships | PVC/PV/StorageClass/volume attributes class/reverse mounts | Storage relationship rules | P1 | ACCEPTED (subset) | M6.3/M6.6 live acceptance | No VolumeAttributesClass parity claimed |
| Relationships | Service selector/Endpoints/Ingress backends/TLS | Label-selection edges marked as inference | P1 | ACCEPTED | M6.2/M6.6 live acceptance | Selectors distinct from ownership; no IP-only inferred Pod |
| Relationships | Configurable CRD children/refs/kind and namespace paths | Validated declarative graph rules | P2 | RESEARCHED | Pending: unit + scoped acceptance | Explicitly out of M6 core scope (optional P2, generic CRD ownerReferences already work) |
| Relationships | On-demand generic CRD children search with budgets | Paged bounded owner-UID discovery | P2 | RESEARCHED | Pending: unit + scoped acceptance | Not yet delivered |
| Relationships | Xray ownership hierarchy | Graph traversal view | P1 | ACCEPTED | M6.5/M6.6 live acceptance | Bounded depth 1–3; not cluster-wide completeness |
| Overview | Pulse refreshed health tiles | Bounded asynchronous overview | P1 | ACCEPTED (M11.3) | M11.8 unit + live accept-m11.py/soak-m11.py | Zero new Kubernetes requests; deliberately narrower than :info |
| Safety | Readonly global/context/cluster and flags | Layered config + hard CLI override at operation boundary | P0 | TESTED | M4 exec/attach/forward denial; M7 policy readonly/override gates; reload regression + live | Delivered; M7 adds the central policy layer, M8 still owns the actual mutation UX |
| Safety | Guardrails deny/confirmation/type context/type name/bulk limits | Combine all restrictions deterministically | P0 | TESTED | M7.0-M7.3 unit+fake+live: `mutation::policy::evaluate` (deterministic, structured `PolicyReason`s, UNKNOWN never Allow) + `Confirmation` binding + `kube::mutation::commit`'s TOCTOU revalidation | Delivered as infrastructure (no user-facing mutation command yet — M8) |
| Safety | Managed object warnings | Evidence of controlling manager in preview | P1 | RESEARCHED | Pending: deny/conflict/outcome regression | Not yet delivered |
| Safety | can-i rules and action review; partial authorizers | SSAR + explicit incomplete reviews | P1 | RESEARCHED | Pending: deny/conflict/outcome regression | Not yet delivered |
| Safety | Action journal and optional rotated export | Started + completed/failed/uncertain outcomes | P1 | ACCEPTED (subset) | M7 journal; M8/M8B/M9 outcome/verification records | Redacted append-only local journal; rotated export parity not claimed |
| Commands | Fuzzy palette/resources/bookmarks/workspaces/plugins | Central command registry | P0 | ACCEPTED (subset) | M2 item 5/6 live | Commands/resources only; bookmarks/workspaces/plugins deferred |
| Commands | Scope/resource/filter syntax and @context completion | Structured query grammar | P1 | ACCEPTED (subset) | M2/M3 grammar + live | Scope/filter commands; @context completion deferred |
| Commands | Per-mode rebindings, disable/conflicts/effective help | Central compiled keymap | P1 | ACCEPTED | M2 remap/help live + conflict regressions | M4 adds log and forward-manager modes |
| Mouse | Rows/wheel/header sort; release for native text selection | Supplementary mouse modes | P2 | RESEARCHED | Pending: unit + scoped acceptance | Not yet delivered |
| Configuration | XDG base/drop-ins/cluster/context/view layers | TOML first, collision-free exact keys | P1 | DESIGNED | Pending: unit + scoped acceptance | Not yet delivered |
| Configuration | TOML/YAML, reload/validation, migration/Home Manager | TOML first; YAML/Nix deferred | P2 | DEFERRED | Pending: unit + scoped acceptance | Not yet delivered |
| Themes | Dark/light palettes, semantic colors, live override | Original semantic themes | P1 | ACCEPTED (M13.1/M13.2) | M13's own unit + live-terminal + restart-persistence proof | Live `:theme NAME`/bare `:theme` list, explicit `:theme_save`; 3 fixed themes (ember/light/mono), no custom palette editor |
| Sessions | Bookmarks/workspaces/view cycling/persistent namespaces | Saved structured queries | P2 | RESEARCHED | Pending: unit + scoped acceptance | Not yet delivered |
| Notifications | Independent single-resource watches, bell/desktop | Bounded notification subscriptions | P2 | RESEARCHED | Pending: unit + scoped acceptance | Not yet delivered |
| Diagnostics | Redacted incident bundle with manifest/limits/preview/export | Local evidence bundle | P1 | ACCEPTED (M11.4) | M11.8 unit + live sensitive-fixture-absence proof against real exported bytes | Bounded sections, atomic 0600/0700 writes, refuse/--force overwrite; no raw logs/env vars |
| Diagnostics | Interactive text/JSON/YAML snapshots, browse/delete | Explicit local export inventory | P2 | RESEARCHED | Pending: unit + scoped acceptance | Not yet delivered |
| Headless | --check, --snapshot, info, info --offline | CLI with deterministic non-TTY output | P0 | TESTED | M1 baseline + M3 typed-filter live snapshots; M12.4/M12.8 added `schemaVersion` (additive, presence/value guaranteed, not byte position — `serde_json::Value` has no `preserve_order`) and live no-ANSI/exit-code proof | No TTY needed |
| Observability | Tracing/redaction, request latency, watch/reconnect counts | Safe event metadata and counters | P0 | DESIGNED | Pending: unit + scoped acceptance | Not yet delivered |
| Compatibility | TLS resumption flag, v1 cert opt-in, Teleport cert workaround | Standard TLS first; exceptions deferred pending own tests | P2 | DEFERRED | Pending: unit + scoped acceptance | Not yet delivered |
| Compatibility | HTTP proxy/NO_PROXY semantics | kube supported proxy behavior; test separately | P1 | RESEARCHED | Pending: unit + scoped acceptance | Not yet delivered |
| Providers | Prometheus/VictoriaMetrics rightsize preview | Opt-in historical metrics adapter | P3 | DEFERRED | Pending: unit + scoped acceptance | Not yet delivered |
| Providers | VictoriaLogs autodiscovery/history/tail/field detection | Opt-in historical logs adapter | P3 | DEFERRED | Pending: unit + scoped acceptance | Not yet delivered |
| Fleet | Opt-in cross-context summary, saved membership, Argo drill | Read-only bounded multi-cluster Eye | P3 | DEFERRED | Pending: unit + scoped acceptance | Not yet delivered |
| Plugins | Trust-gated (Approved/Disabled) subprocess, fixed argv, env allowlist (PATH/HOME/LANG only), bounded stdout/stderr, timeout, process-group cancellation | Owned subprocess tasks via `app::session::Sessions` (`Kind::Plugin`) | P0 | TESTED | M12.1/M12.2 unit + M12.8 live: `scripts/accept-m12.py` (real stdout/exit code, real no-credential-leak proof, real timeout, real mid-run cancellation, zero orphan on the OS process table) run twice clean; `scripts/soak-m12.py` 64-cycle churn, zero orphans | `GroupKillGuard` fix (M12.2) closes a real task-abort-vs-cooperative-cancel gap; JSON `protocolVersion:1` object projection to stdin |
| Plugins | Inline commands/placeholders/bulk output; live activity panel/stderr tail (streamed, not post-completion); catalog search/describe/install/update/rollback/offline/checksums/withdrawal/remove; managed port-forwards from a plugin; bundled sanitize; external Popeye/Trivy catalog integrations | Structured external command contract; versioned adapter protocol; explicit future registry design; extensions after safety/process controls | P3 | DEFERRED | Pending: unit + scoped acceptance | Not yet delivered — M12 delivered the smallest safe execution shape only (see row above), evidence-backed scope amendment in `docs/M12_ACCEPTANCE.md` |
| Distribution | Linux/macOS x86_64/aarch64, Cargo/Homebrew/Nix | Build and release automation | P2 | TESTED (subset) | M12.5/M12.8: Linux x86_64 built, packaged, and live-verified locally (`scripts/package.sh`, `scripts/accept-m12.py` seq7); other 3 platforms CI-defined in `.github/workflows/release.yml` (`workflow_dispatch`-only, never triggered this milestone) — see `docs/INSTALL.md` | No Cargo/Homebrew/Nix packaging; no macOS/aarch64 build has ever actually run |
| Performance | Published startup/filter/view/RSS methodology | Pipeline benchmark and session soak | P0 | TESTED (subset) | `benches/pipeline.rs`; M4.4 75-minute RSS/fd/thread observations in M4_ACCEPTANCE.md; M12.7 added cold-startup (median 1.58ms) and a 20k-object tier, both recorded with hardware/rustc/OS context in `docs/M12_ACCEPTANCE.md` | No comparative performance claim; large-cluster campaign beyond one machine's memory remains |
| SAURON | Eye problem-priority overview (mission addition) | Evidence-ranked current-context view | P1 | ACCEPTED (M11.2) | M11.8 unit + live RBAC-forbidden proof (never claims healthy/zero) | Reverse-Severity ordering, no numeric score; reuses adjacent::Target for navigation |
| SAURON | Blast radius, safety lens, change preview | Labeled inference + typed patch policy | P1 | ACCEPTED (M11.5) | M11.8 unit + live owner-chain grouping | Reuses the same bounded Xray graph, grouped by graph::Provenance; never causal language; no mutation path |
| SAURON | Context diff, navigation replay, explainable score | Read-only comparison/replay; scoring optional | P3 | ACCEPTED (M11.6, reduced scope) | M11.8 unit + live EQUIVALENT/UNKNOWN proof | Single-target kind/namespace/name comparison only; comparison key explicitly not identity; no scoring |

## Current gap review — M9 accepted, reconciled 2026-09-21

M8 and M8B add accepted guarded user-facing mutations and advanced cluster
operations on M7's gateway. M9.0–M9.5 and M9.7 add Flux/Argo inspection and
supported guarded actions plus native Helm inspection. M9.6 Helm mutations
remain explicitly deferred. Composite rows retain subset qualifications:
accepted milestone scope is not full Sofka parity.

Latest recorded suite: 305 unit + 74 fake HTTP, plus 9 integration live tests.
M9 combined acceptance/regression ran twice; bounded M9 soak was 240 seconds
per run (53–54 cycles), not 75 minutes. Local tag `m9-accepted`: `f2a18d0`;
subsequent `75a3c63` fixes only the soak counter sentinel. M10 has not started.
See [M9_ACCEPTANCE.md](M9_ACCEPTANCE.md) for limitations and the narrow Helm
Secret-read exception; no generic Secret reveal, NOTES, or Helm actions added.

M6 (M6.0-M6.6) fully ACCEPTED 2026-09-18: bounded relationship graph
(ownerReferences, explicit typed references, Service/Pod selectors,
EndpointSlice/Ingress status references, PVC/PV/StorageClass storage
references, bounded reverse Config/Secret/ServiceAccount/PVC scans), the
`:adjacent` grouped-relationship view, and the `:xray` bounded (depth 1-3)
cycle-safe traversal are all implemented, unit/fake/live-tested, and
interactively verified end to end via `scripts/accept-m6.py` plus a
75-minute soak. Per-slice evidence: [M6_ACCEPTANCE.md](M6_ACCEPTANCE.md).

M7 (M7.0-M7.6) fully ACCEPTED 2026-09-18: central deterministic policy
engine, incarnation-safe mutation identity, confirmation contract bound to
the exact intent, single execution gateway with TOCTOU revalidation,
redacted append-only journal, and read-only `:policy`/`:mutations`
surfaces — all unit/fake/live-tested (including one narrowly-scoped
internal proof mutation against the isolated `kind-sauron-test` fixture,
never production) and interactively verified via `scripts/accept-m7.py`
plus a 75-minute soak (zero reconnects). M7 deliberately ships **no
user-facing mutation workflow** at its own checkpoint; M8 subsequently added it. Per-slice
evidence: [M7_ACCEPTANCE.md](M7_ACCEPTANCE.md).

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
[EXEC.md](EXEC.md); M4 acceptance does not claim it is fully fixed. The bounded relationship graph, `:adjacent` and `:xray` are now delivered as
of M6 (see above); blast-radius prediction and broader configurable guardrails
remain future work, while the central mutation policy is accepted through M7–M9.
M5 explicitly stayed read-only and did not build toward the
graph beyond Explain's own bounded, verified-ownership two-hop Pod correlation.
