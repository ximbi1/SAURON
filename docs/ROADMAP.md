# Executable roadmap

Complete vertical slices and update HANDBOOK/parity after each. No acceptance by scaffolding.

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
| 12 | Plugins/providers/headless maturity/packaging/performance | Process limits/trust; four platform builds; checksums/license inventory; reproducible performance campaign |

Headless inspection, health projections and tests are pulled forward to M1. Readonly and
redaction are foundational. M4 interactive exec/attach/forward requires a conservative
central operation boundary (readonly denies); M7 later adds full mutation policy and
journaling. Development operational acceptance is isolated-kind-only, never production.
At every milestone compare current parity, input latency, broken journeys, crash cases,
permissions assumptions and keystroke cost. Continue the next implementable slice.
