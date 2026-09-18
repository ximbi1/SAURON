# M6 relationship / Adjacent / Xray acceptance ledger

Baseline: `m5-accepted` at `1a45dbd`; README subsequently added at `d877ee0`.
M1–M5 acceptance is preserved, not repeated as a new baseline audit.
Production is read-only; all fixture writes require `scripts/test-cluster.sh`,
`.test-cluster/config`, and verified `kind-sauron-test` Docker/API identity.

Relationship does not imply cause. Owner UID, explicit reference, selector match
and status reference are distinct provenance classes. No name heuristics.

## Slice ledger

| Slice | Contract | Implementation | Unit / fake API | Live evidence | Bugs / gaps | Verdict |
| --- | --- | --- | --- | --- | --- | --- |
| M6.0 | Canonical scoped identities, provenance, bounded deterministic graph and traversal | TESTED foundation | 5 new unit tests; 110 unit + 19 fake HTTP suite | None | Unresolved-target/source-error presentation arrives with resolver; no UI yet | NOT ACCEPTED live |
| M6.1 | Generic owner UID validation; Pod/workload explicit references | IMPLEMENTING transport integration | 4 extractor + 2 resolver unit tests; 3 graph fake HTTP; full suite 116 unit + 22 fake HTTP green | Guarded m6-test PASS: Deployment→RS→Pod UID chain, template config/Secret/SA resolution | No aggregate report/UI; no operation-wide request budget yet | NOT ACCEPTED |
| M6.2 | Service selectors, reverse selectors, EndpointSlice/Endpoints, Ingress | TESTED | 3 pure network tests + 2 catalog-ambiguity tests + 1 reverse-selector fake HTTP test; 26 fake HTTP + 121 unit green | Guarded m6-test PASS: Service→Pod selector, Ingress→Service/TLS-Secret, real controller EndpointSlice targetRef without apiVersion resolves through unambiguous catalog | Never infer Pods from IP; EndpointSlice apiVersion omission root-caused and fixed (see journal) | ACCEPTED |
| M6.3 | Storage and bounded reverse config/identity/mount references | TESTED | 3 storage-extraction unit tests + 1 reverse-reference fake HTTP test; 27 fake HTTP + 124 unit green | Guarded m6-test PASS: PVC→PV, PVC→StorageClass, PV→PVC via claimRef with exact UID match, PV→StorageClass, reverse Pod→PVC, against a real dynamically-provisioned local-path PV | claimRef kind/apiVersion hardcoded as schema-fixed (not a guess, unlike EndpointSlice targetRef); reverse scan bounded to an explicit built-in workload candidate list (Pods/Deployments/StatefulSets/DaemonSets/Jobs/CronJobs), no arbitrary CRD reference inference | ACCEPTED |
| M6.4 | Adjacent with UID-safe canonical navigation/history | TESTED | 2 render unit tests + 2 app-level navigation tests; 127 unit + 27 fake HTTP green | Guarded m6-test PASS: real Deployment adjacent report renders OWNS/REFERENCES groups with UID-real navigable targets | `:adjacent`/`a` opens grouped view (OWNED BY/OWNS/SELECTED BY/REFERENCES/REFERENCED BY); `enter` (`Follow`) jumps via exact GVK+namespace+UID reusing the existing history stack, never by name; no interactive terminal smoke test run in this automated session, only headless unit+live coverage | ACCEPTED |
| M6.5 | Bounded cycle-safe Xray, existing health, no causal claims | NOT STARTED | Pending | None | No global graph database | NOT ACCEPTED |
| M6.6 | Combined adversarial acceptance, regressions, soak | NOT STARTED | Pending | None | Tag prohibited until complete | NOT ACCEPTED |

## Required evidence

M6.0: context/GVR/UID separation, replacement, deterministic dedup/order,
node/edge/depth bounds, cycles, explicit partial coverage. Unit evidence permits a
foundation commit, not claims of live topology acceptance.

M6.1: generic owner resolution and mismatch, duplicate owners, reverse children;
Pod regular/init/ephemeral env/envFrom, projected/ordinary config/Secret/PVC
volumes, imagePullSecrets, Node and ServiceAccount; accurate workload template paths.

M6.2: equality selectors, empty selectors select nothing, namespace isolation,
reverse Services; explicit endpoint targetRef only; Ingress backend/TLS paths.

M6.3: claimRef UID validation, StorageClass references, reverse mounts and
bounded supported workload scans; denied scans preserve successful evidence.

M6.4–5: epoch/request races, target replacement before navigation, canonical
history, cancellation, deterministic cycle/depth handling, 32x9, help consistency.

## Combined live flows (pending)

1. Deployment → ReplicaSet → Pod → back/forward, UID-correct.
2. Pod → ConfigMap → reverse references, exact field evidence.
3. Pod → Secret, no content fetched/displayed.
4. Pod → ServiceAccount → referencing Pod.
5. Pod → PVC → PV → StorageClass → reverse mount.
6. Service ↔ Pod selectors visibly distinct from ownership.
7. Service → EndpointSlice → explicit targetRef, no IP-only edge.
8. Ingress → Service and TLS Secret.
9. Same-name/new-UID replacement during Adjacent/Xray.
10. Context switch during collection rejects late result.
11. Restricted RBAC retains usable graph with PARTIAL source failure.
12. Cyclic fixture or deterministic unit graph terminates within bounds.
13. Fanout reaches configured bound and visibly reports PARTIAL.
14. 32x9 Adjacent and Xray.
15. M5 broken-workload Explain regression.
16. M4 forward remains functional during graph navigation.
17. Metrics collector remains scoped and functional.
18. Quit while collecting restores terminal and joins owned tasks.

Run combined flows twice where practical with a freshly built binary. Full locked
fmt/check/clippy/test plus live M1–M5 regressions precede acceptance. Record actual
coverage, not merely script exit status. Bugs require root cause, regression, full
checks, rebuild and exact live replay before progressing.

## Performance / soak (pending)

Record requests/operation, concurrency, candidate scans, nodes/edges, bound hits,
cancellation; idle rendering must issue zero requests. Target 75-minute isolated
kind soak rotating scopes/Adjacent/Xray/history/Explain with metrics and an M4
forward active: cycles, graph operations, RSS/fds/threads/tasks, errors and partials.
These are observations, not proof of leak freedom.

## Journal

- M6.4 accepted: added `src/adjacent.rs`, rendering a `report::Report` as text
  grouped by intrinsic direction/provenance (OWNED BY / OWNS / SELECTED BY /
  REFERENCES / REFERENCED BY — SelectorMatch is its own group regardless of
  direction; StatusReference edges fold into REFERENCES/REFERENCED BY with an
  explicit "(status reference)" annotation, never silently equated with a
  user-authored ExplicitReference). Each rendered row carries its exact line
  index plus canonical GVK/namespace/UID as an `adjacent::Target`. Wired
  `Action::Adjacent` (key `a`, table mode) through the existing
  `open_document`/`refresh_document` path via a new `start_adjacent` branch
  (Adjacent has its own resolver, `relationships::report::adjacent`, not the
  generic per-action `kube::evidence::document`), and `Action::Follow` (key
  `enter`, document mode): maps the document's current scroll position back to
  the nearest target at/after the top of the viewport, then reuses the exact
  same `push_history`/`apply_history`/`finish_history` stack as `[`/`]` and
  `HistoryBack`/`HistoryForward` — so navigating from Adjacent is a normal,
  reversible history entry, not a special case, and the destination resource is
  the caller's already-resolved `Resource` (never a re-resolved name string).
  Added 2 render unit tests and 2 app-level tests
  (`adjacent_follow_navigates_by_canonical_identity_and_history_returns`,
  `follow_without_an_adjacent_target_errors_instead_of_navigating`). Extended
  the live m6-test to call the real renderer against the live Deployment
  report and assert every produced target carries a real UID. Full locked
  fmt/check/clippy/test green: 127 unit + 27 fake HTTP. Guarded
  `scripts/test-cluster.sh m6-test` replay passed. No interactive terminal
  smoke test was run in this automated session — only headless unit and live
  API-level coverage; flag this explicitly if a manual TUI pass is wanted
  before M6.6's combined acceptance. M6.4 ACCEPTED. Proceeding to M6.5 (Xray).

- M6.3 accepted: added `src/graph/references/storage.rs` (PVC.spec.volumeName->PV,
  PV.spec.claimRef->PVC with UID carried verbatim from the field Kubernetes
  populates on bind, PVC/PV.spec.storageClassName->cluster-scoped StorageClass).
  claimRef's kind/apiVersion are hardcoded, not guessed: the field is schema-fixed
  to name exactly one PersistentVolumeClaim, unlike EndpointSlice's targetRef which
  can point at any kind. Added `reverse_references()` to
  `kube/relationships/report.rs`: for ConfigMap/Secret/ServiceAccount/PVC roots,
  scans an explicit bounded candidate list (Pods, Deployments, StatefulSets,
  DaemonSets, Jobs, CronJobs) and links back any candidate whose own `extract()`
  targets match by kind/namespace/name; a denied kind records an issue and does
  not drop edges already found in another kind. Added 3 storage-extraction unit
  tests and 1 reverse-reference fake-HTTP test
  (`graph_report_reverse_configmap_reference_from_pod_and_deployment`). Extended
  the m6-fixtures Deployment with a PVC mount (`m6-data`, default `standard`
  StorageClass, WaitForFirstConsumer) so the live cluster genuinely provisions
  and binds a PV. Full locked fmt/check/clippy/test green: 124 unit + 27 fake
  HTTP. Guarded `scripts/test-cluster.sh m6-test` replay against `kind-sauron-test`
  passed, including asserting the PV's real claimRef UID matches the exact live
  PVC object. M6.3 ACCEPTED. Proceeding to M6.4 (Adjacent view).

- M6.2 accepted: independent review confirmed the EndpointSlice apiVersion fix is
  correctly scoped (only `Provenance::StatusReference` tolerates an empty
  `api_version`; owner/explicit references still require exact GVK) and wired
  (`mod network;` compiles the split-out extractor). Added the one missing
  required-evidence item, a fake-HTTP test proving the reverse Pod→Service
  selector edge (`graph_report_reverse_service_selector_from_pod_root`), since
  prior coverage only drove the graph from the Service side. Full locked
  fmt/check/clippy/test green: 121 unit + 26 fake HTTP. Guarded
  `scripts/test-cluster.sh m6-test` re-run against `kind-sauron-test` passed,
  including the real controller-produced EndpointSlice missing apiVersion.
  Proceeding to M6.3.

- M6.2 live bug: `m6-test` failed at the real EndpointSlice targetRef assertion.
  Direct read of the controller-produced slice confirmed kind/name/namespace/UID
  present but apiVersion omitted; extractor wrongly required apiVersion and dropped
  the reference. Root cause is application schema assumption, not fixture/harness.
  Added omission regression plus catalog-kind collision test. Status references
  without version now resolve only when discovery offers one canonical resource;
  ambiguous kinds stay Unsupported, ownerReferences still require exact GVK/UID.
  Full locked suite and exact guarded live replay running; progression paused until
  both pass. No IP/name heuristics introduced.

- `377402e` commits M6.1 extraction/transport, not whole-slice acceptance. Added
  aggregate selected-object report with additive issues, exact root revalidation
  and explicit reverse-scan coverage. Fake tests for Forbidden+successful edge
  and root replacement during collection passed. Added byte-bounded native request
  transport (2 MiB GET / 8 MiB metadata list), error bodies ignored, 30-second
  overall deadline. Full suite 116 unit + 25 fake HTTP passed; live aggregate
  Deployment report passed through m6-test. Follow-up fmt check caught formatting
  on the final issue-cap marker; corrected with rustfmt, final suite passed.
  No UI/epoch delivery yet, no M6.1 acceptance claim.

- M6.1: shared extractor covers every requested PodSpec path including six workload
  prefixes. Exact GVK lookup, namespace validation, expected UID rejection,
  metadata-only Secret reads (406 stays unsupported), cancellation and per-request
  timeout implemented. Reverse owner scans inspect one explicit GVR/page, 200
  candidates/50 children, exact owner UID/GVK/name; metadata only. Cluster-owner
  to namespaced-child scan currently explicitly Unsupported (no implicit all-ns
  widening). Fake HTTP proves metadata Accept header/no fallback and replacement.
  Full locked checks green: 116 unit + 22 fake HTTP. Added isolated `sauron-m6`
  fixtures and `relationships_live` opt-in test through guarded m6-test script;
  first live run PASS: real owner chain and three template reference targets;
  Secret result has neither data nor stringData. No production access.

- M6.0 foundation: canonical GVR/GVK/scope/UID validation, separate provenance,
  ordered evidence dedup, atomic node/edge bounds, cycle/depth handling, shared
  Observation timestamp. Five added unit tests; locked fmt/check/clippy clean,
  110 unit + 19 fake HTTP passed (one opt-in live test ignored). No live graph
  claim. Next M6.1 builds extraction/transport atop this committed foundation.

- 2026-09-18: ledger created before implementation. M6.0 starting; no cluster
  access, new acceptance, or tag claimed. Next: minimal graph model and unit tests.
