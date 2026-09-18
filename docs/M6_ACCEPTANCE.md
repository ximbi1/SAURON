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
| M6.2 | Service selectors, reverse selectors, EndpointSlice/Endpoints, Ingress | NOT STARTED | Pending | None | Never infer Pods from IP | NOT ACCEPTED |
| M6.3 | Storage and bounded reverse config/identity/mount references | NOT STARTED | Pending | None | No arbitrary CRD reference inference | NOT ACCEPTED |
| M6.4 | Adjacent with UID-safe canonical navigation/history | NOT STARTED | Pending | None | Registry/help/32x9 required | NOT ACCEPTED |
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
