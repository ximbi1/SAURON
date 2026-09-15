# M3 inspection/query acceptance ledger

Baseline: local annotated `m2-accepted` → `675567d`, verified 2026-09-15.
Status vocabulary and safety rules: HANDBOOK.md / AGENTS.md. No production fixtures.
All live work targets verified `kind-sauron-test`, `.test-cluster/config` explicitly.
Item 1 is ACCEPTED (2026-09-15); remaining slices are not accepted.

## 1. Filters — ACCEPTED

Evidence: `cargo fmt --check`, locked all-target check/clippy/test pass (41 unit + 4
fake HTTP). `python3 scripts/accept-m3.py filters` passed live with freshly built binary;
cases below covered by this script and named filter/runtime/transport regressions. Found
stale syntax-error display on regex repair; fixed root cause, full checks and exact live
replay passed before resuming. Remaining milestone work is not implied by this acceptance.

| Cases | Required evidence | Status |
| --- | --- | --- |
| Plain/fuzzy; quoted substring; valid/invalid regex; inverse | AST unit + live table | PASS |
| AND/OR/NOT; nesting; precedence; malformed recovery | AST unit + live unchanged-view assertion | PASS |
| Integer/float/count; age; CPU/memory; percent; bool | Typed unit cases; live age/restarts/CRD JSON values | PASS |
| Absent/null/wrong type; NOT UNKNOWN; unknown OR true/AND false | Truth-table unit + visible unknown count + headless notice | PASS |
| Label existence/value; qualified and case-sensitive keys | Unit + live fixture labels | PASS |
| Server label selector only; field selector only; both + local AST | Fake HTTP verifies selectors; live combined scoped results | PASS (independent single selectors expanded in combined M3) |
| Resource/ns/context switches with filters; Refresh | Live rapid repeated switches, fake stale epoch injection | PASS |
| Back/forward preserves AST, selectors, scope, canonical identity | Regression + live back/forward/Refresh/context | PASS |
| Malformed input cannot widen scope or add history | Regression + live invalid→corrected filter | PASS |

Contract: strong Kleene three-valued logic. Only TRUE rows are included; UNKNOWN rows
are counted separately and reported, never described as false. Missing label existence
is FALSE for a valid metadata labels map or omitted labels; missing label comparison
is UNKNOWN. No automatic AST pushdown. Explicit `-l` and `-f` server selectors constrain
the list/watch independently; local predicates evaluate only within that result set.
Generic scalar fields use explicit JSON Pointer `field:/spec/path`, with optional typed
prefix `integer:`, `number:`, `count:`, `duration:`, `cpu:`, `memory:`, `percent:`, `bool:`
before that field name. No arbitrary code or JSONPath evaluation in filters.

## 2. Sorting — IMPLEMENTING

Typed stable ascending/descending; unknown always last in either direction. Test name,
age, restart/count/quantity/percent/bool and generic fields; duplicate ties remain stable;
selected UID preserved, disappearance clears it. Filter/scope/history/rapid-watch
compositions live; no additional history, alias resolution or tick-only sort rebuilds.

## 3. Documents — DESIGNED

Shared vertical/page/home/end/horizontal/wrap/fullscreen/search/next/previous/refresh
actions. Live long YAML, 32x9, resize/search, no matches, wrap, update then refresh,
delete then refresh, same-name replacement, exit/terminal restoration, unchanged history.
Fresh GET and pinned UID; cancellation/request/epoch gates. Secret redaction retained.

## 4. Events — DESIGNED

UID correlation, Warning toggle, explicit timestamp precedence, count/reason/type/message/
reference, bounded partial notice, no-events retention caveat. Fake 403/timeout/partial;
live Normal/Warning/empty/refresh/deleted target/context/history/replaced UID. Related-object
navigation only if it fits canonical identities cleanly; otherwise explicitly defer it.

## 5. Tables / CRD printer columns — DESIGNED

Research Kubernetes negotiation contract before transport changes. Curated → server Table
→ safe CRD printer subset → generic fallback (final choice recorded after research).
GVR+UID+RV identity gates and scoped async results. No Table-watch requirement: document
bounded read fallback and separate live object source. Live namespaced and cluster CRDs,
numeric/bool/date/missing/priority columns, update/recreate/CRD removal/ambiguity/narrow/
horizontal columns. Fake unsupported/partial/stale responses and metadata failures.

## 6. Combined adversarial live acceptance — NOT STARTED

- Complex filter → CRD → namespace → back → forward.
- Invalid regex → repair → context switch; semantics unchanged.
- Server+local selectors → Refresh → all namespaces → concrete namespace.
- Typed sort → live update → UID selection retained.
- Document → update → refresh → delete → explicit failure.
- Document search → resize → return; history unchanged.
- Warning Events → context switch → back → replacement UID.
- CRD columns → update → same-name replacement; no old cells.
- Table → rapid resource switches → return; no old columns/rows.
- 32x9 with long breadcrumb/filter/table/document transitions.
- Filter + sort + history + Refresh interleaved.
- Rapid context/namespace/resource changes with Table/Event/document requests in flight.

Each real live bug interrupts progress: minimal repro, root cause, regression, full suite,
fresh binary, exact live replay. Record commands/results in RUNBOOK and handbook journal.
Each slice runs fmt/check/clippy/tests with `--locked --all-targets` (fmt separately).
Only after all acceptance, reconcile docs and create local annotated `m3-accepted` with
features, fixes, limits, live scope and deferrals. Never publish/push.
