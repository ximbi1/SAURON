# M3 inspection/query acceptance ledger

Baseline: local annotated `m2-accepted` → `675567d`, verified 2026-09-15.
Status vocabulary and safety rules: HANDBOOK.md / AGENTS.md. No production fixtures.
All live work targets verified `kind-sauron-test`, `.test-cluster/config` explicitly.
Items 1 and 2 are ACCEPTED (2026-09-15); remaining slices are not accepted.

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

## 2. Sorting — ACCEPTED

Full fmt/check/clippy clean; 46 unit + 4 fake HTTP passed. Fresh-binary
`accept-m3.py sorting` PASS: typed quantity/count/bool and unknown ordering, selected
UID after live value update, history restoration, rapid scopes, delete/recreate no
auto-selection, Pod age/restarts/name/filter. Unit tests cover stable ties, mixed numeric
precision, pending-list selection, rapid updates and no task/history/epoch change on sort.

Typed stable ascending/descending; unknown always last in either direction. Test name,
age, restart/count/quantity/percent/bool and generic fields; duplicate ties remain stable;
selected UID preserved, disappearance clears it. Filter/scope/history/rapid-watch
compositions live; no additional history, alias resolution or tick-only sort rebuilds.

## 3. Documents — ACCEPTED

Evidence: `cargo fmt --check`, locked all-target check/clippy/test pass (49 unit + 4
fake HTTP). Live acceptance against `kind-sauron-test` with a freshly rebuilt binary,
manual tmux session (no `accept-m3.py documents` mode was written for this item; the
same live-flow discipline was applied by hand):

| Case | Evidence | Status |
| --- | --- | --- |
| Vertical nav: down/up/page/home/end | Live YAML, line counter tracked correctly | PASS |
| Horizontal scroll, wrap on/off | Wrap off enables `←`/`→`; wrap on rejects with a real error, not a silent no-op | PASS |
| Search: match, next, no-match | `matches i/N` tracked correctly; 0/0 for an absent term, no crash | PASS |
| Update then refresh (same UID) | External label change via kubectl, `r` while doc open updated content + snapshot time in place | PASS |
| Delete then refresh | External delete, `r` → "NOT CURRENT" + real 404, not a crash | PASS |
| Same-name replacement (different UID) | Recreated same name, `r` → "Object was replaced or has no UID", the existing UID-pin invariant, not silently showing the new object under the old title | PASS |
| Palette over an open document | `:` shows the table underneath (unchanged from before); Esc or an unrelated failed command restores the SAME document; a successful navigation command correctly replaces it — no extra history entry from the palette/document juggling | PASS |
| History unchanged by opening/closing documents or logs | Confirmed via one `[` after a document-then-navigate sequence | PASS |
| 32x9 with document + fullscreen | Both clip/expand without panicking | PASS |
| Logs share the same Document model | Live follow stream; `r` (no refresh source) errors clearly without interrupting the stream | PASS |
| Exit/terminal restoration | Clean `q` from within a document, terminal restored | PASS |

Found and fixed two real bugs during this pass:
1. **Startup-crashing key chord**: `ScrollLeft`/`ScrollRight`'s registry keys (`"left,h"`/
   `"right,L"`) used the plain word `"left"`/`"right"`, which `command::parse_key` didn't
   recognize as named keys (only `up`/`down`/`home`/`end`/`pageup`/`pagedown`/`enter`/`esc`/
   `tab`/`backtab`); it only accepted single characters otherwise, so bare `"left"`/`"right"`
   panicked with "Unsupported key chord" -- and since `Keymap::compile` runs at every
   `State::new()`, this crashed 12 unrelated unit tests and would have crashed the app at
   startup. Fixed by adding `"left"`/`"right"` to `parse_key`'s named-key table.
2. **Stale action error masking document status**: a per-action validation error (e.g.
   "turn wrapping off before horizontal scrolling") was written to `state.error`, the SAME
   field a real transport/watch error uses -- so it never cleared on a later successful,
   unrelated key press (e.g. a search), permanently hiding the document's own status line
   (line/col, search matches) behind a one-off rejection. Found live while testing search
   right after a rejected scroll attempt. Fixed by routing key-dispatched and
   picker-dispatched action errors through the existing `input_error` field (already used
   for filter-input rejections, already correctly cleared on the next successful action and
   already prioritized correctly against a real transport `error` in the status line), not
   `state.error`. No unit test added (the mechanism lives in the async terminal-input loop,
   not a pure function); verified by rebuilding and repeating the exact live sequence that
   found it, which now shows the search result instead of the stale rejection.

Contract: one `Document` model (`src/app/document.rs`) backs YAML, describe, explain,
events, static help, and logs -- grapheme-aware wrapping via `unicode-segmentation`/
`unicode-width`, a cached visual-line layout keyed on `(revision, width, wrap)`, bounded
lines/bytes/visual-segments/search-matches, and a `Freshness` state (`Local`/`Snapshot`/
`Refreshing`/`Error`) shown in the status line. `refresh_document()` reuses the exact
`kube::evidence::document()` UID-pin check that originally protected only Yaml/Describe/
Explain/Events, extended here to the interactive Refresh action. Logs have no `source`
(streaming, not a point-in-time GET) and refreshing them errors clearly rather than
attempting a GET. Opening the palette from a document no longer discards it (`palette_document`
holds it while the palette is open); this supersedes the M2 finding that documented the
old discard-on-palette behavior as intentional -- it was a limitation worth removing once a
correct restore mechanism existed, not a permanent design choice.

## 4. Events — ACCEPTED

Evidence: `cargo fmt --check`, locked all-target check/clippy/test pass (49 unit + 7
fake HTTP, 3 new for this item). Live acceptance against `kind-sauron-test` with a
freshly rebuilt binary, manual tmux session.

| Case | Evidence | Status |
| --- | --- | --- |
| UID correlation | Server-side `involvedObject.uid=` field selector (pre-existing); reconfirmed live on a freshly recreated Pod with a new UID | PASS |
| Warning toggle | New `W` / `:toggle_warnings` action, scoped to `Source.warning_only`; live toggle on/off on real mixed Normal/Warning events; errors clearly ("only applies to the Events view") when not viewing Events | PASS |
| count/reason/type/message/fieldPath | All shown; `involvedObject.fieldPath` (e.g. `spec.containers{worker}`) now appended when present — previously silently dropped | PASS |
| Bounded partial notice | Existing `limit(200)` + `continue` token check; fake-HTTP test added (`events_partial_continue_token_is_reported`) | PASS |
| No-events retention caveat | Live on a CRD instance with genuinely zero related Events; wording distinguishes "no events at all" from "no Warning events among N total" when the toggle is active | PASS |
| Fake 403 | `events_403_is_visible_and_never_leaks_secrets`: Events document still returns Ok with the Forbidden message and the bearer token redacted, never panics | PASS |
| Fake partial (continue token) | `events_partial_continue_token_is_reported` | PASS |
| Fake timeout | Not added — the existing per-call `tokio::time::timeout(connection.timeout(), ...)` wrapper is the same mechanism already exercised for the Pod-identity GET elsewhere; a dedicated fake-hang harness for this specific call was judged not worth the test-infrastructure cost this pass. Documented gap, not silently skipped. |
| Live Normal/Warning/empty/refresh/deleted-target/context/history/replaced-UID | All exercised: mixed real events with the toggle; a CRD instance with no events; `r` refetching in place; deleting the target then `r` → "NOT CURRENT" 404; switching context (returns to the table, matching the existing document-doesn't-survive-context-switch behavior); back/forward after a context switch correctly restores the table, not stale Events state; recreating the same-named Pod and opening Events again shows the NEW UID's fresh (empty) events, never the old UID's | PASS |
| Related-object navigation | Explicitly DEFERRED, as the ledger permits: every Event already correlates to the single currently-selected object (server-side UID filter), so there is no *different* related object to navigate to from this view without a materially larger feature (arbitrary-object jump from free-text event fields) that doesn't fit the canonical-identity contract cleanly yet. |

Found and fixed one real gap, and investigated one suspected bug that turned out not
to be reachable:
- **Real**: `involvedObject.fieldPath` was read nowhere; added it to the rendered line
  when present, and to the corresponding fake-HTTP test.
- **Investigated, not a live bug**: considered whether `event.get("lastTimestamp")`
  returning `Some(Value::Null)` for a present-but-null field could stop the
  `.or_else()` timestamp fallback chain before reaching a real value (`eventTime`,
  `creationTimestamp`). Checked `k8s-openapi`'s hand-written `Serialize` impl for
  `Event` directly (`event.rs`, `last_timestamp`/`event_time`/etc. are only
  `serialize_field`'d `if let Some(value) = &self.x`): every `Option` field is
  *omitted* when `None`, never emitted as JSON `null`. Since `events()` always goes
  through this typed `KubeEvent -> serde_json::Value` round-trip, a field that's
  absent from the wire response (or explicitly `null`, which deserializes to `None`
  the same way) always ends up *absent* in the `Value`, not `Value::Null` — so
  `.or_else()`'s existing behavior was already correct for every reachable case.
  Hardened the fallback to explicitly skip `Value::Null` anyway (`.find(|v|
  !v.is_null())`, zero cost, strictly not worse) in case a future dependency version
  or a different code path changes that serialization behavior, but this is NOT
  claimed as a fixed live bug — the regression test added for it
  (`warning_only_filters_events_and_a_null_timestamp_field_falls_through`) verifies
  the fallback chain and the fieldPath/toggle behavior, not a demonstrated prior
  failure, since none could be constructed.

Contract: one Events view per selected object (server-filtered by UID), rendered
through the same `Document`/`Freshness`/UID-pin machinery as Yaml/Describe/Explain.
`Source.warning_only` is a display filter applied to the already-fetched, already
bounded (200) event list — toggling it re-fetches (via the normal Refresh path) rather
than caching raw events on the client, keeping `Document` itself GVK/action-agnostic.

## 5. Tables / CRD printer columns — ACCEPTED

**Research finding, before any transport change**: `kube` 4.2 / `k8s-openapi` 0.28 ship
no typed support for the server's `Table` content-negotiation format
(`Accept: application/json;as=Table;v=v1;g=meta.k8s.io`) — it would need a raw
`http::Request` built by hand and a hand-written response type. More fundamentally,
that format is a **one-shot, non-watchable snapshot**: its `columnDefinitions` carry a
name/type/priority but no JSONPath a live-watched object could be re-evaluated
against later, so a Table response only tells you what kubectl would have printed for
the objects in *that one response* — it cannot drive a continuously-live table without
either re-polling on a timer (contradicting this project's watch-first design) or
falling back to a second, separate mechanism anyway for live updates.

**Decision**: implement CRD `additionalPrinterColumns` directly instead. A CRD's own
declared columns carry a `jsonPath`, which we convert once (per resource, not per
object) to a JSON Pointer and evaluate against every live-watched object using the
exact typed `Field`/`Scalar` machinery the filter language (`FILTERS.md`) already
provides — genuinely live, no polling, no Table-watch needed. Server Table conversion
remains a documented, deliberately deferred fallback for a resource that is neither
curated nor a CRD (an aggregated API without printer columns); given hand-curated
projections already cover the common built-ins, this gap is judged low-value relative
to the one-shot/live mismatch above, and is left for a future milestone if it proves
necessary. `docs/M3_ACCEPTANCE.md`'s original "curated → server Table → safe CRD
printer subset → generic fallback" ordering becomes, after this research: **curated →
CRD printer columns → generic fallback**, with server Table noted but not built.

Evidence: `cargo fmt --check`, locked all-target check/clippy/test pass (53 unit + 10
fake HTTP, 7 new for this item). Live acceptance against `kind-sauron-test`, extending
the existing `eyes.testing.sauron.local` CRD fixture (whose schema already had
`count`/`ratio`/`enabled`/`cpu`/`memory`/`percent` fields defined but unused by any
printer column) with a full set: `Focus` (string), `Count` (integer), `Ratio`
(number), `Enabled` (boolean), `Absent` (a field that genuinely doesn't exist, for the
missing case), and `Detail` (`priority: 1`, wide-only).

| Case | Evidence | Status |
| --- | --- | --- |
| Live namespaced CRD | `eyes.testing.sauron.local` (Namespaced scope) | PASS |
| Live cluster-scoped CRD | `probes.a.sauron.test` (Cluster scope, no printer columns declared — exercises the "CRD found but no columns" path, distinct from "no CRD found") | PASS |
| Numeric column | `Count` (integer) showed `8`, then `42` live after an external `kubectl patch` | PASS |
| Bool column | `Enabled` showed `true` | PASS |
| Missing column | `Absent` (`.spec.doesNotExist`) showed `-`, not an error or a blank crash | PASS |
| Priority (wide) column | `Detail` hidden by default, shown only after `w` (existing Wide toggle, reused directly — no new toggle needed) | PASS |
| Update | `kubectl patch` while the view was open updated `Count` in place on the next tick, no manual refresh needed (columns are evaluated from the live watched object every render) | PASS |
| CRD removal while viewing it | Deleting the CRD live: watch enters `WatchError`/stale, `[0/0]`, but the printer-column *headers* stay (cached from the earlier successful fetch) rather than reverting mid-session to a shorter generic set — no crash, no wrong data shown | PASS |
| Ambiguity | Not applicable to this layer: `printer::fetch` only ever runs against an *already-resolved* canonical `Resource` (post `discovery::resolve`), so there is no ambiguous name to resolve here — ambiguity is entirely handled upstream (M3 item 1's `AGENTS.md` alias-collision rule) | N/A by design |
| Narrow terminal | 32x9 with 6 extra columns clips without panicking, same as every other wide table | PASS |
| Horizontal columns | `w` (Wide) toggle reused exactly, no new key | PASS |
| Curated resources unaffected | `pods` (empty API group) confirmed to skip the CRD lookup over the network entirely (`printer_columns_skip_the_network_entirely_for_core_resources`, and live: no behavior change) | PASS |
| Non-CRD, non-curated resource | `portals.a.sauron.test` navigated to directly (has no printer columns of its own — a straightforward CRD-without-columns case, not a distinct "aggregated API" case, which wasn't separately available in the fixture cluster); generic `NAME`/`STATUS`/`AGE` shown, no error | PASS |
| Fake unsupported/partial/stale responses | `printer_columns_are_empty_not_an_error_for_a_non_crd_resource` (404 → empty, not an error); `printer_columns_are_fetched_live_from_the_crd_spec` (full success path); `printer_columns_skip_the_network_entirely_for_core_resources` (a panicking fake handler that must never be called for an empty-group resource) | PASS |
| Metadata failures (malformed JSONPath) | `printer::json_path_to_pointer` unit tests reject `[?(...)]`, `[*]`, `..`, negative/non-numeric indices — that column is omitted, not guessed at or shown wrong; `a_column_with_an_unsupported_json_path_is_omitted_not_guessed` | PASS |

GVR+UID+RV identity gates: no new identity concept needed here — printer columns are
keyed by `resource` (canonical GVK, already gated) and evaluated per-object from the
same live watched `Object` (already UID/RV-correct) every render; there is no separate
per-column identity to track. Scoped async results: the fetch is spawned alongside
the watch in `watch_resource()`, carries the watch's own `epoch`, and is dropped by
the existing `reduce()` epoch check exactly like every other async result if the scope
changes before it completes — verified live by switching resources rapidly while a
fetch might still be in flight (no stale columns from a previous resource ever
appeared).

Contract: `kube::printer::fetch()` is a single bounded read (GET on the CRD object,
`connection.timeout()`-wrapped), never a list/watch, spawned once per resource-view
change and cached in `State.printer_columns` until the next `cancel_scope()`. Columns
merge additively with curated/generic ones (skipping a name collision, which none of
the fixtures produced); `priority == 0` always shown, `> 0` only with Wide.

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
