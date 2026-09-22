# Executable roadmap

Complete vertical slices and update HANDBOOK/parity after each. No acceptance by scaffolding.

Current checkpoint (2026-09-22): M1–M8 and the added M8B advanced-operations
milestone accepted; M9.0–M9.5/M9.7 accepted, M9.6 Helm mutations explicitly
deferred; M10.0–M10.9 accepted (tag `m10-accepted`) — bulk multi-select
mutations, workspaces, bookmarks, configurable keymaps, themes, and config
persistence, all reusing the unmodified M7/M8/M8B mutation gateway;
M11.0–M11.8 accepted (tag `m11-accepted`) — Eye, Pulse, evidence bundle,
blast radius, and context diff (reduced scope), all composing M1-M10's own
evidence primitives rather than building new ones. M12.0–M12.8 accepted
(tag `m12-accepted`) — trust-gated plugin subprocess execution
(process-group limits, env allowlist, timeout/cancellation, zero orphan
proven under sustained churn); headless `schemaVersion` hardening;
Linux x86_64 packaging built and live-verified locally, the other three
platforms CI-defined but never executed; dual MIT/Apache-2.0 licensing
with a full dependency license inventory; a performance campaign closing
the cold-startup and large-object-count gaps. Providers remained
evidence-backed DEFERRED (see `docs/SOFKA_PARITY.md`); this was the final
milestone of the current roadmap. The table below states milestone goals,
not claims of full feature parity; acceptance ledgers record shipped subsets.

| Milestone | Deliverable | Acceptance |
| --- | --- | --- |
| 0 | Research, memory, build, CLI, CI | fmt/check/clippy/tests; offline info needs no TTY or credentials |
| 1 | kubeconfig → discovery → watch → Pod store/table | Real API initial list plus watch update/delete; UID replacement; responsive quit; terminal restored |
| 2 | Context/namespace/generic kinds, command palette | Switch rapidly without stale rows; restricted namespace listing; CRD and alias navigation |
| 3 | AST filters, typed sort, YAML/describe/events | Precedence/errors/unknown tests; selectors hit API; document scrolling; related events by UID |
| 4 | Logs, container choice, exec/attach, forwards | Live log follow cancellable; bounded buffer; terminal restored after exec failure; loopback port conflict/stop |
| 5 | Metrics, health, Explain, timeline | Missing metrics stays unknown; real failure fixtures; evidence paths; fresh UID check; relist doesn't invent transitions |
| 6 | Graph/adjacent/Xray | Verified owner UID chains; service/storage references; partial RBAC and bounded traversal |
| 7 | Central mutation policy, guardrails, journal | Every mutation denied under readonly; stale target rejected; outcomes and cancellation uncertainty recorded |
| 8 | Delete/scale/restart/image/node workflows | Isolated-cluster mutations; patch preview; UID/RV conflict; drain PDB/emptyDir/unmanaged safety |
| 9 | Flux/Argo/Helm | Actual CRD fixtures and Helm storage decode; no assumed suspend semantics; rollback explicitly scoped |
| 10 | Bulk/workspaces/bookmarks/themes/keymaps | Invalid reload retains policy/keymap; per-context scope; effective help; mark identity |
| 11 | Eye/Pulse/bundle/context diff/blast radius | Partial overview explicit; sensitive fixture redaction; local export permission/overwrite tests |
| 12 | Plugins/providers/headless maturity/packaging/performance | Process limits/trust (delivered, live-proven); four platform builds (Linux x86_64 delivered and live-proven, the other three CI-defined only); checksums/license inventory (delivered); reproducible performance campaign (delivered); providers evidence-backed DEFERRED |

Headless inspection, health projections and tests are pulled forward to M1. Readonly and
redaction are foundational. M4 interactive exec/attach/forward requires a conservative
central operation boundary (readonly denies); M7 later adds full mutation policy and
journaling. Development operational acceptance is isolated-kind-only, never production.
At every milestone compare current parity, input latency, broken journeys, crash cases,
permissions assumptions and keystroke cost. Continue the next implementable slice.
