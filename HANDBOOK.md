# SAURON engineering handbook

Canonical project memory. Read this before each major phase, inspect the code, reconcile
claims with reality, and update this file after meaningful changes. README is for users.
Last reconciled: 2026-09-15. Project began in an empty directory with no Git repository.

## Production boundary — explicit user instruction

The pre-existing Kubernetes cluster is PRODUCTION. The user explicitly permits reads
and forbids anything that could alter it. This restriction persists across sessions.
Never create, apply, patch, edit, delete, scale, restart, evict, drain, exec, attach,
upload, run helpers/debug containers, Helm/GitOps actions, or mutating plugins there.
Do not run fixture or cleanup commands using implicit/default kubeconfig context.
Production activity so far: local kubeconfig context-name enumeration and one GET
`/version`; no resource creation, modification or deletion. No test fixtures in production.
All fixture writes so far went to a separate Docker kind cluster named `sauron-test`,
context `kind-sauron-test`, explicit `.test-cluster/config`. Do not confuse these clusters.
Operational instructions and the latest checkpoint: `docs/RUNBOOK.md`.

## Identity and mission

SAURON: one eye over the entire cluster. Help Kubernetes operators see what exists,
understand what needs attention, inspect evidence, and control only what policy permits.
Users: SREs, platform engineers, application on-call engineers, cluster administrators.
Primary journeys: investigate failed workloads; navigate ownership/config/storage;
follow logs and events; inspect arbitrary APIs; safely review and execute changes.

Principles: keyboard first; explicit scope at all times; deterministic health; evidence
before diagnosis; unknown is distinct from healthy/zero; native APIs; responsive input;
bounded work; graceful partial results; honest feature status. Color supplements text.
Narrow terminals must remain usable. Never infer performance from implementation language.
When live acceptance finds a real bug: find the root cause, add a regression test when
reasonable, run the full check suite, then repeat the exact live flow that found it against
the real cluster before moving on. A milestone is done when observed working live under
the flows most likely to break it — not when it compiles or unit tests pass.
Non-goals: hosted control plane, telemetry uploads, credential broker, automatic repair,
required AI, embedded external security scanners, copying Sofka source/artwork/branding.
Brand constants belong in `src/brand.rs`; manifests/package metadata necessarily repeat
the binary name. No Tolkien artwork, quotations, or affiliation. Website is unset.

## Research baseline

See `docs/RESEARCH.md` and `docs/SOFKA_PARITY.md`. Inspected Sofka website and main
checkout `2024e5921cf9cb06fd02f666630f97bae5e24eb3` (package 0.27.3) on 2026-09-15.
Reference checkout is outside this repository at `/tmp/sauron-research.FyCFg9/sofka`;
it is disposable and not required to build. No source incorporated. Official documentation
and Kubernetes contracts guide independent implementation. Documentation conflicts are
recorded rather than treated as requirements. Public roadmap search found a formerly
indexed `future.md`, absent at inspected HEAD; historical aspirations are not shipped parity.

## Architecture and invariants

Single Rust package, library plus thin CLI binary. Modules, not forty empty subsystems.

```text
terminal stream ──> input/keymap ──> command ──> application state
                                                │ effects
                                                v
                       task supervisor / epoch / cancellation
                          │        │        │
                      discovery  watches  evidence/log reads
                          └────────┴────────┘
                              bounded channel
                                    │
                                    v
                         reducer → object store → rows → Ratatui
                                    │
                           timeline / health rules

future mutation: intent → policy → RBAC → preview → confirmation
                → revalidate identity → API → outcome journal
```

One application owner updates state. Network workers communicate immutable messages.
Every connection/view has an epoch; old results are discarded even if cancellation races.
Watch restart uses a staging map; publish only at InitDone, preserving old rows with a
resync indication during relist. UID identifies incarnation, namespace/name the slot.
Delete events cannot remove a replacement with a different UID. Selected UID survives
sort/update; disappearance or replacement clears selection. No locks across await.
Network tasks are owned, cancelled, and joined/aborted on shutdown. Queue backpressure
must never silently drop individual watch updates. Rendering is capped and batches updates
with a bounded drain to prevent input starvation. No API requests from rendering.

Discovery retains GVR, kind, scope, shortnames and verbs. Core discovery is separate from
extension discovery. Failed groups are visible and do not erase successful groups.
Aliases resolve deterministically; explicit group qualification avoids ambiguity. Only
one selected resource watch initially; global fanout must have separate limits.
Objects enter a shared dynamic representation; curated projections are pure and qualified
by API group. Unknown kinds have name/namespace/age/status fallback. CRD/Table columns
are a subsequent slice. Secret bodies must be redacted before storage/display/export.

Config: XDG TOML, strict schema, exact cluster/context keys (avoid filesystem name
collisions), recursive layer merge, validated limits. Failed reload retains prior config.
Persistence initially absent except opt-in diagnostics; timeline/navigation session-local.
Commands have structured parsing and one registry; effective help follows key bindings.
Plugins are deferred: structured stdin, explicit executable/argv, bounded output and
deadline, process-group cancellation, no sandbox claims. Providers are opt-in and may
never receive kubeconfig credentials at arbitrary external URLs.

## Repository ownership map

| Path | Owns / interface | Must not own |
| --- | --- | --- |
| `src/main.rs` | CLI dispatch and process lifecycle | health rules, widgets |
| `src/brand.rs` | product metadata | runtime state |
| `src/app/` | state, reducer, task lifecycle, input routing | raw HTTP in widgets |
| `src/kube/` | config/client, discovery, watch, evidence/log I/O | terminal rendering |
| `src/resources/` | identity, projections, health, bounded store | network or mutations |
| `src/filters/` | bounded lexer/parser/AST, typed evaluation | implicit API scope widening |
| `src/command/` | grammar, action registry, effective keymap | networking |
| `src/ui/` | pure rendering, theme, document/table layout | Kubernetes clients |
| `src/config.rs` | validated TOML and XDG paths | kubeconfig edits |
| `src/explain.rs` | evidence-based findings | invented root causes |
| `src/safety.rs` | redaction and eventual mutation gateway | bypasses for plugins |
| `tests/` | fake API and controlled cluster/terminal acceptance | writes to default cluster |
| `benches/` | reproducible measured hot paths | fake product data |
| `docs/` | user references, contracts, parity and measurements | unsupported claims |

Rows above are intended boundaries, not a claim all paths exist yet. Update as built.

## Feature status

States: NOT STARTED, RESEARCHED, DESIGNED, IMPLEMENTING, FUNCTIONAL, TESTED,
ACCEPTED, DEFERRED. FUNCTIONAL requires real behavior; TESTED identifies actual tests;
ACCEPTED requires demonstrated acceptance, not compilation or fixture-only rendering.

| Slice | State | Evidence / outstanding |
| --- | --- | --- |
| Research and architecture | DESIGNED | sources inspected; docs created before code |
| Build, CLI, tracing | TESTED | working CLI/headless diagnostics; M1/M2 check suites |
| Kubeconfig/discovery/live resource store/table | ACCEPTED | observed live against kind-sauron-test; Refresh-after-refresh table-emptying bug found and fixed (see journal) |
| Context/namespace/generic discovery/commands | ACCEPTED | entire M2 including combined adversarial flows; annotated local m2-accepted at 675567d |
| Basic filters/sort/documents/events (M1 scope only) | ACCEPTED | basic flows observed live; NOT acceptance of the fuller M3 requirements |
| M3 filter completion | ACCEPTED | 41 unit + 4 fake HTTP checks; reproducible live snapshots/PTY `scripts/accept-m3.py filters`; stale input-error regression fixed and replayed |
| M3 sorting | ACCEPTED | 46 unit + 4 fake HTTP; live typed sort/update/selection/history/scope/replacement via accept-m3.py sorting |
| M3 documents | IMPLEMENTING | shared viewer/refresh/search interaction slice next |
| M3 events/Tables/CRD columns | DESIGNED | ordered slices, not accepted; see docs/M3_ACCEPTANCE.md |
| Pod logs | ACCEPTED | follow, previous, and explicit-container all observed live |
| Exec/port-forward | RESEARCHED | M4; no actions exposed |
| Health/Explain/timeline | IMPLEMENTING | pure rules + fresh-object/UID-related Event evidence; child correlation pending |
| Metrics | DESIGNED | missing metrics remain unknown; no samples fabricated |
| Graph/relationships/Xray | RESEARCHED | M6 |
| Policy/guardrails/journal/mutations | DESIGNED | read-only initial release |
| GitOps/Helm | RESEARCHED | native inspection before actions |
| Eye/Pulse/bundles/diff | RESEARCHED | bounded evidence collection |
| Plugins/providers/fleet/packaging | DEFERRED | stable core first |

Detailed capability status and acceptance plans live in `docs/SOFKA_PARITY.md`.

## Decisions

### 2026-09-15: dynamic single-package architecture
Alternative: one crate or renderer per kind. Choose cohesive modules and shared objects;
common resources get pure projections. Avoid build and API surface overhead until needed.
### 2026-09-15: bounded channels and staged relist
Alternative: Sofka's documented unbounded messages. Prefer bounded backpressure, capped
objects/bytes, bounded frame batches. Relist is atomic to avoid false emptiness/deletions.
### 2026-09-15: current stable dependencies
Rust 2024, kube 4.2, k8s-openapi 0.28 with v1_32 schema, ratatui 0.30.2,
crossterm 0.29, Tokio, futures-util, tokio-util CancellationToken, serde/json,
TOML, clap, tracing, anyhow, regex. Use Rustls standard verification. No custom TLS
exceptions. No gzip initially to avoid compressed watch stream compatibility issues.
Select the oldest schema supported by this k8s-openapi release for broad API compatibility;
this is not a tested server-version compatibility claim. Cargo.lock is committed.
### 2026-09-15: conservative initial safety
Inspection comes first. No write flag that pretends unsupported mutations exist. Secret
values hidden at ingress, no reveal yet; log content never written to application tracing.
Kubeconfig exec credential plugins are trusted executable configuration and may execute
as part of authentication; SAURON does not edit kubeconfig or export it.
### 2026-09-15: reordered vertical slices
Headless snapshots and pure health arrive with M1 for measurable acceptance. Full metrics,
log orchestration, and correlated Explain remain separate. No need to wait until M12 for
non-TTY diagnostics. Central safety precedes every future mutation.

## Security assumptions

Kubeconfig and its certificate/key files are user-owned trusted inputs. Never print raw
config, auth failures containing credential output, Secret payloads, TLS keys, or tokens.
Honor configured identity/RBAC; discovery verbs are capabilities, not authorization.
SelfSubjectAccessReview answers may age; final API responses remain authoritative.
No implicit shell execution beyond Kubernetes credential plugin behavior. Exec/attach,
uploads and volume helper creation are potentially mutating and must be policy gated.
Loopback only for future forwards; no automatic binds to public interfaces. Local exports
need restrictive permissions, explicit destination and overwrite protection. Journals must
record outcomes/uncertain cancellation, not assert success on request submission.
Cluster data/logs are untrusted terminal input; strip control sequences. Redaction is
defense in depth, cannot prove arbitrary application text contains no sensitive data.
The existing cluster is authorized only for read-only smoke checks; all fixture mutations
must explicitly select a dedicated SAURON test cluster with a separate kubeconfig.

## Testing and performance

Unit: store identity/relist semantics, health edge cases, AST precedence/unknown logic,
quantity/duration parsing, sort, config merge, command grammar, redaction/policy.
Integration: local HTTP fake API for pagination, 403, disconnect, 410, task cancellation,
epoch races. Controlled kind cluster for real discovery/watch/logs and CRDs. TUI:
Ratatui TestBackend at normal/narrow/zero dimensions and PTY input/restore checks.
Fixtures: healthy and failed Pods/workloads, pending storage, warning Events, restricted
identity, CRD. Never manufacture data inside production workflows.
Checks: `cargo fmt --check`, `cargo check --all-targets`,
`cargo clippy --all-targets -- -D warnings`, `cargo test --all-targets`.
Bench harness: 100/1,000/5,000 Pods, generic objects, filter/sort/update/render. Record
environment, build mode, counts, repetitions, timings and limitations. Startup and API
latency are separate from in-memory throughput.
M2 checkpoint records green fmt/check/clippy/tests and live acceptance. M3 baseline
re-run: `cargo test --locked --lib` 34/34 passed; three additional fake-HTTP tests
live in `tests/watch_transport.rs`. Live `tests/cluster.rs` is opt-in (`#[ignore]`),
previously passed against isolated kind; this attribute does not mean untested.
New M3 full-suite/live results will be recorded per slice, not inferred from M2.
Debug-profile bench sample on the development machine: 100 objects 11.5ms build /
0.5ms filter+sort / 4.4ms render; 1,000 objects 107ms / 5.4ms / 14.6ms; 5,000 objects
472ms / 24.8ms / 53.0ms. Debug build, single run, no release-profile or repeated-sample
data yet — not a performance claim, a recorded local measurement only.

## Known limitations and bugs

Initial implementation exists; the checks above are green. Broad parity is a long-term
backlog. Existing machine: rustc/cargo 1.95, 15 GiB RAM with other workloads; keep build
parallelism modest. Docker available. Existing production API `/version` read succeeded
(Kubernetes v1.33.4). No sensitive resource contents collected in research.
M1 and M2 visual/TUI acceptance is recorded below; M3 acceptance is pending. Log reader caps
allocation before clipping (16 KiB per line, 4 KiB read chunks); covered by the fake-HTTP
watch tests but not yet by a dedicated log-clipping regression test. Row rebuild is gated
by a `prepare()` memo keyed on store revision/filter/sort/descending, plus the current
second only when the filter has an `age` comparison (see journal entry below — an earlier
version included the clock unconditionally and resorted every tick regardless of scope). Other open concerns: document wrap
scroll semantics, scope selection during empty filters. Watch list synchronization is
labeled separately from an established watch; no header-level connection probe yet.
No in-cluster config fallback, server Tables, CRD printer columns,
multi-container log fanout, metrics, graph, or mutations yet. Core Event reads cap at 200
and report truncation. Describe is SAURON's contextual native report, not kubectl parity.

## Current milestone / continuation instructions

M1 and M2 are ACCEPTED. Verified local annotated `m2-accepted` points to
`675567d940d0cdb7ad8e5c7ae2e95d4d3de5e435`; initial M3 worktree was clean.
This is a reference/rollback checkpoint, not permission to discard user changes.
Items 1-4 (filters, sorting, documents, Events) are ACCEPTED. Current: M3 item 5
(server Tables/CRD columns), then item 6 (combined adversarial live acceptance).
Contract and case ledger: `docs/M3_ACCEPTANCE.md`. M3 remains entirely read-only.
No `m3-accepted` until every required slice and combined live flow is demonstrated.
Re-read this handbook at phase boundaries. Never mark broader milestones done from
isolated unit tests alone. Keep buildable handoffs.

## Journal

### 2026-09-15 — M3 item 4 accepted (Events)
Added a Warning-only toggle (`W` / `:toggle_warnings`, scoped to the new
`document::Source.warning_only` field) so the existing UID-correlated Events view can
show just Warning-severity events; toggling re-fetches through the exact same
`refresh_document()`/UID-pin path as a manual Refresh, keeping `Document` itself
action-agnostic (it never caches raw event JSON, only rendered text). Added
`involvedObject.fieldPath` to the rendered line (e.g. `(spec.containers{worker})`) --
previously read nowhere despite pinpointing which part of the object an event is
about. Added three fake-HTTP tests: a 403 on the Events list (still returns Ok with
the message shown and the bearer token redacted, never panics), a `continue` token
(the existing PARTIAL notice), and mixed Normal/Warning events exercising both the
toggle and the timestamp fallback chain.

Investigated, but did NOT confirm, a suspected bug: whether a present-but-`null` JSON
field (e.g. `lastTimestamp`) could make `.or_else()` stop before reaching a real
timestamp later in the fallback chain (`eventTime`, then `metadata.creationTimestamp`).
Checked `k8s-openapi`'s hand-written `Event` `Serialize` impl directly: every optional
field is `serialize_field`'d only `if let Some(_)`, so a `None` field is always
*omitted* from the JSON, never emitted as `null` -- and a `null` on the wire
deserializes to `None` the same way, so it's omitted too on the way back out. Since
`events()` always round-trips through this typed struct, the "present but null" case
this theory worried about cannot actually occur via the real API path; the existing
`.or_else()` chain was already correct for every case that's actually reachable.
Hardened the chain anyway to skip `Value::Null` explicitly (`.find(|v| !v.is_null())`)
since it's free and strictly not worse, but this is recorded honestly as a
non-bug/defensive-hardening, not a fixed live bug -- the discipline of finding root
cause before claiming a fix cuts both ways: it also means not overclaiming one that
isn't there once actually traced to source.

Live acceptance against `kind-sauron-test`: mixed real Normal/Warning events on
`crashloop` with `fieldPath` showing correctly; the Warning toggle on/off, and a clear
error when tried on a non-Events document; a CRD instance (`observatory`) with
genuinely zero related Events; Refresh re-fetching in place; deleting the target then
Refresh showing "NOT CURRENT" with a real 404, not stale content; a context switch
(returns to the table, matching the existing document-doesn't-survive-a-context-switch
behavior) and Back/Forward afterward correctly restoring the table rather than leaking
Events/Warning-only state; recreating the same-named Pod with a new UID and reopening
Events showing that new UID's fresh (empty) events, never the deleted one's stale
data. Related-object navigation explicitly deferred (the ledger permits this): every
Event already correlates to the single selected object via the server-side UID filter,
so there is no *different* related object to jump to from this view without a
materially larger feature that doesn't fit the canonical-identity contract yet.
A fake-timeout test for `events()` was not added -- the mechanism is the same
per-call `tokio::time::timeout` wrapper already used and exercised elsewhere in this
file; a dedicated hang-simulation harness for this one call site was judged not worth
building this pass. Documented as a real, acknowledged gap, not silently skipped.
Full case-by-case evidence in `docs/M3_ACCEPTANCE.md` item 4.
Re-ran `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test
--all-targets` (49 unit + 7 fake-HTTP) after implementing. Fixture state reconciled.

### 2026-09-15 — M3 item 3 accepted (shared document viewer)
Picked up mid-implementation (`src/app/document.rs`, unicode-aware wrap/layout/search,
`Freshness`, palette-preserving-document, horizontal scroll) with `cargo fmt/check/
clippy` already clean but `cargo test --all-targets` failing 12/49 tests with
"Unsupported key chord" panics. Root cause: the new `ScrollLeft`/`ScrollRight` bindings
used `"left"`/`"right"` as key names, which `command::parse_key` never learned (only
named `up`/`down`/`home`/`end`/`pageup`/`pagedown`/`enter`/`esc`/`tab`/`backtab`, else a
single char); since `Keymap::compile` runs in every `State::new()`, this broke nearly
every test that constructs one and would have crashed the app at startup. Fixed by
adding the two names to `parse_key`; full suite back to 49/49 (+4 fake-HTTP).
Rebuilt and ran the full M3 item 3 live checklist by hand against `kind-sauron-test`
(vertical/page/home/end nav, horizontal scroll with wrap on/off, search incl. no-match,
update-then-refresh, delete-then-refresh, same-name-different-UID-then-refresh, palette
opened over a document, 32x9, fullscreen, logs sharing the same model, clean exit).
Found a second real bug live: a per-action validation error (e.g. the wrap-off-required
rejection) was written to `state.error`, the same field a genuine transport/watch error
uses, so it never cleared on a later successful unrelated key press -- it sat there
masking the document's own status line (line/col, search matches) after a search that
had actually succeeded. Routed key- and picker-dispatched action errors through the
existing `input_error` field instead (already used for filter-input rejections, already
correctly cleared on success, already prioritized correctly in the status line) --
`state.error` remains exclusively for backend/transport state that must survive
unrelated key presses until the watch actually recovers. No unit test (the mechanism is
in the async terminal-input loop); verified by rebuilding and replaying the exact
sequence that found it, which now shows the search result instead of the stale error.
Confirmed the UID-pin check already in `kube::evidence::document()` (previously only
exercised by the initial Yaml/Describe/Explain/Events open) now also protects the new
interactive Refresh action correctly: delete-then-refresh and same-name-replacement-
then-refresh both show a clear "NOT CURRENT" error, never stale or silently-wrong
content. Opening the palette from a document now restores it afterward instead of
discarding it (superseding the M2 finding that had recorded the old discard behavior as
intentional -- it was a limitation worth fixing once `palette_document` existed to make
restoring correct, not a permanent design choice). Full details and case-by-case
evidence in `docs/M3_ACCEPTANCE.md` item 3. Re-ran `cargo fmt --check`, `cargo clippy
--all-targets -- -D warnings`, `cargo test --all-targets` (49/49) after both fixes.
Fixture state reconciled (`scripts/test-cluster.sh fixtures`); the crashloop pod was
deleted and recreated live during testing and came back clean with no leftover label.

### 2026-09-15 — M3 item 2 accepted
fmt/check/clippy clean; 46 unit + 4 fake-HTTP tests passed. Fresh binary then
`python3 scripts/accept-m3.py sorting` PASS: numeric 2 vs 10, memory, bool, reverse,
unknown-last, live update moving selected A while UID stays selected, history round-trip
with selection, rapid namespace/context switches retaining sort, deletion then recreation
of selected A leaves selection empty, Pod restarts/age/name sorts combined with filter.
Terminal returned normally. No new live bug. Fixture ConfigMap A deleted and recreated
only inside verified kind; B/C unchanged. No production access.
All-target benchmark: 100/1000/5000 rows filter+sort median 0.273/2.302/13.750ms and
render 2.937/10.082/45.633ms (debug, five samples, local load not controlled).
Next shared document UX; server-table field metadata integration remains item 5.

### 2026-09-15 — M3 item 2 implementation
Re-read current handbook/code after filter commit `ce02631`. Added pure `resources/sort.rs`
using shared scalar fields, unknown-last in both directions, stable namespace/name ties,
cached per-row keys and exact mixed integer/float ordering. Sort command accepts explicit
typed JSON Pointer fields without re-resolving resources or creating history/tasks.
Code review found history UID cleared before initial list completion and automatic
selection after empty→replacement transitions; fixed using initial-list gating and a
one-time per-view autoselect flag. Added regressions plus guarded ConfigMap fixtures and
`accept-m3.py sorting` for live updates/delete/recreate. Full checks/live acceptance pending.
No new dependencies. Fixture mutations exclusively through identity-verifying helper.

### 2026-09-15 — M3 item 1 accepted (query/filter slice)
Full fmt/check/clippy clean; 41 unit + 4 fake-HTTP tests pass; fake servers require
loopback permission outside sandbox (initial denied-bind run recorded, rerun passed).
Rebuilt binary and repeated exact regex-repair live failure successfully, then completed
`python3 scripts/accept-m3.py filters`: snapshots for fuzzy/substring/regex/Boolean,
case-sensitive labels, native CRD scalars and quantity casts; unavailable metrics stay
UNKNOWN; live Pod age/restarts; server label+field selectors and local filter together;
history/back/forward/Refresh selectors; three rapid namespace rounds and three context
rounds; all-namespaces; ambiguous alias rejection; terminal return. All PASS against
isolated kind only. No new dependencies, no production requests.
Added `filters/value.rs` shared scalar access: exact integer/count and finite f64 quantities,
native JSON Pointer scalar types, typed validation; docs/FILTERS.md records limits.
Explicit equality is now case-sensitive; fuzzy/substring/regex remain insensitive.
History carries resolved Resource metadata and selectors, no same-catalog re-resolution.
Watch runtime query text becomes canonical version/plural; no alias retained after resolve.
Stricter current AGENTS ambiguity invariant overrides historical M2 built-in-shadow policy:
`po` colliding with a CRD now errors in every case; `v1/pods` is explicit. Existing M2
historical entries remain historical, not the current alias contract.
Benchmark harness ran as part of all-target tests: 100/1000/5000 filter-sort medians
1.038/9.855/31.534ms, render 8.730/31.584/66.793ms (debug, five samples, concurrent
build contention; not suitable for cross-version performance comparison).

### 2026-09-15 — M3 filter live bug: stale syntax error after repair
Reproduced with live Eye table → `/` → Ctrl-U → `/[/` → Enter → Ctrl-U →
`/^observ/` → Enter. Rows matched but old "invalid or oversized regex" still displayed.
Root cause: filter errors reused transport `State.error`; success never cleared it.
Added separate input-error state and atomic `State::set_filter`, preserving transport
errors on correction. Regression asserts previous AST survives invalid input and corrected
input clears only its error. Full checks and exact fresh-binary live replay pending.
Other M3 tasks paused until replay succeeds, per AGENTS.md.

### 2026-09-15 — M3 baseline reconciliation
Read AGENTS, handbook, runbook, parity and code; verified clean HEAD and annotated M2
checkpoint. Fixed stale current sections contradicting recorded M1/M2 acceptance;
historical entries retain their original time-specific claims. Isolated cluster identity
check passed (Docker access required sandbox approval). Baseline 34 unit tests passed.
Inspection found selector fields absent from history, fallback-to-All on history parse
failure, lowercased label keys, missing label-existence AST, and display-string numeric
coercion. These are M3 item 1 work, not accepted functionality. No production requests.

### 2026-09-15 — investigation and design
Inspected website, README, features, architecture, keys/keybindings, configuration,
views, filtering, safety, debugging, providers, plugins, benchmarks, comparison, current
dependency manifests and selected discovery/Helm source. Searched roadmap and found
stale indexed page absent from HEAD. Created memory, source notes, architecture, roadmap
and parity plan before implementation. Environment checks and existing API version read
succeeded. No Kubernetes mutations performed.

### 2026-09-15 — first implementation and toolchain setup
Added Cargo project/lock, brand, config layering, safety redaction, object projections and
health, staged bounded store/timeline, filter lexer/AST, key/command registry, transport
discovery/watch/evidence/logs, app state/tasks/input, Ratatui views and non-TTY CLI.
`cargo check --all-targets` passed. First full test build running; no tests claimed passed
yet. Added serde_yaml_ng (MIT) for YAML output and chrono (MIT OR Apache-2.0) for timestamps;
Tokio websocket feature reserved for subsequent native exec/forward work.
Created dedicated kind `sauron-test` (v1.33.1), separate kubeconfig in ignored
`.test-cluster/config`; its node is Ready. Existing kubeconfig context unchanged.
User explicitly authorized installing development tools. sudo requires a password, so
installed distribution-matching rustfmt/Clippy 1.95 packages in
`/home/ximbi/.local/opt/sauron-rust-tools`, linked through `~/.local/bin` already on PATH.
`cargo fmt --version`, `cargo clippy --version`, and `cargo fmt` succeeded.
No privileged installation or user shell configuration changes required.

### 2026-09-15 — user production restriction and verification checkpoint
User clarified existing cluster is production: reads allowed, all altering operations
forbidden. Recorded this restriction prominently and added an operational runbook plus
test target validation. Fixture writes used only explicit kind-sauron-test kubeconfig:
namespace, healthy Deployment/Service, crashloop/unschedulable/missing-image Pods,
redaction-sentinel Secret, Eye CRD and custom resource. Test node Ready; healthy Pod
Running; intentionally failing Pods have expected failure/pending states.
First full unit/TUI test run: 14 passed / 1 failed (anchored regex cell semantics).
Clippy reported 8 actionable lints. These are not accepted milestones. Rustfmt installed
and applied; current edits require another formatting pass. Added opt-in live test
`tests/cluster.rs`, not yet executed. No performance measurements yet.

### 2026-09-15 — regex fix, lint cleanup, transport regression tests, first commit
Fixed the anchored-regex cell-matching bug (filter now matches name/namespace/each cell
independently instead of a concatenated row string) and all 8 Clippy lints. Added
`tests/watch_transport.rs`: a real local HTTP server exercising paged list+watch replacement
with UID-based relist, bounded-channel cancellation, 403 Forbidden error surfacing without
leaking bearer tokens, and partial discovery when one API group is forbidden. Added a
`prepare()` memo in `State` so dirty-frame redraws only re-filter/sort when store revision,
filter text, sort key or direction actually changed. Added a synthetic-data bench
(`benches/pipeline.rs`) over 100/1,000/5,000 objects; first debug-profile numbers recorded
above. Verified on this machine: `cargo fmt --check`, `cargo check --all-targets`,
`cargo clippy --all-targets -- -D warnings` all clean; `cargo test --all-targets` 18/18
passing (the live `tests/cluster.rs` test remains `#[ignore]`d, not yet run against
`kind-sauron-test`). No visual/TUI acceptance against the live cluster recorded yet — that
remains the next step. Made the first Git commit of the project after confirming no
kubeconfig, certificate, key, Secret fixture body, or other credential material was staged.

### 2026-09-15 — prepare() memo bug: clock alone forced a resort every tick
`prepare()`'s cache key included `Utc::now().timestamp()` unconditionally, so it changed
every second regardless of data/filter/sort, defeating the point of memoizing row rebuilds
on the dirty-frame tick. Root cause was real but overbroad: only an `age`-based filter
comparison (e.g. `age>1h`) can change row membership from time alone, since an object can
cross the threshold with no store update. AGE column values and AGE-based sort order do
NOT need this: `ui::render` already recomputes `Object::age`/`field` fresh every frame
straight from `state.rows`, and elapsed time advances every object's age by the same delta,
so relative sort order is time-invariant. Fix: added `Expr::has_time_predicate()` (true
only when the filter tree contains an `age` comparison) and made `prepare()` include the
current second in its key only when that is true, decoupling display freshness and
sort-order invalidation from filter-membership invalidation. Added regression tests in
`src/app/state.rs` proving repeated `prepare()` calls across a real ~1.1s clock tick do not
resort when nothing relevant changed, that an `age`-filtered `prepare()` does reconsider
membership across a tick, and that `Object::age` keeps advancing independent of the memo.
Re-ran `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, and
`cargo test --all-targets` (22/22 passing) after the fix; re-ran the 100/1,000/5,000-object
bench, no regression (debug-profile, single machine, single run).

### 2026-09-15 — M1 interactive acceptance against kind-sauron-test, and a second
### prepare() bug the first fix uncovered
Ran `bash scripts/test-cluster.sh fixtures` and `bash scripts/test-cluster.sh test`
(`tests/cluster.rs` live test now passes against the real cluster, not just `#[ignore]`d).
Drove the real TUI interactively over tmux against `kind-sauron-test`: live watch table
for `pods -n sauron-fixtures` showed correct STATUS/RESTARTS/AGE for all four fixtures
(CrashLoopBackOff, ImagePullBackOff, Unschedulable, Running); `:ns kube-system` relisted
real cluster Pods; the `ey` CRD shortname resolved and rendered `observatory` with the
generic-kind `Unknown` status fallback; `/coredns` filter correctly narrowed `[2/8]`; `y`
showed redacted YAML (`last-applied-configuration: <redacted>`); `X` showed Explain
findings with real evidence and next-inspection guidance; `E` showed UID-correlated
Events; `l` streamed real timestamped log lines with working follow; resizing to 40x10 and
back caused no panic and re-rendered correctly; `q` restored the terminal cleanly every
time (no leftover raw mode/alternate screen).

While exercising `r` (Refresh/restart current watch) repeatedly, found that the *previous*
fix exposed a second, more serious latent bug: `prepare()`'s key used `store.revision`,
but `cancel_scope()` (called by every reconnect/refresh/context/namespace/resource switch)
replaces `store` with a fresh `Store::new()` whose revision restarts at 0 and unconditionally
clears `rows`. Two independent watches routinely finish their initial relist at the same
revision number (e.g. both land on 1 after one `finish()`), so on the second such switch
`prepare()` saw an unchanged key, skipped `rebuild()`, and the table stayed permanently
empty (`[0 / N]`, still labeled "list synchronized") until something else changed the
filter or sort. The original unconditional per-second timestamp had been accidentally
masking this by forcing a rebuild at least once per second regardless of the rest of the
key; removing it for the common case surfaced the real aliasing bug. Fixed by adding
`epoch` to the key alongside `revision` — `epoch` is bumped on every `cancel_scope()` and
never resets, so the pair is unique across watch generations even when the revision number
alone repeats. Added a regression test reproducing the exact scenario (second watch lands
on the same revision as the first; asserts rows are rebuilt from the new store, not left
empty). Re-ran `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`,
`cargo test --all-targets` (23/23), and re-verified live: five consecutive `r` presses
against `kind-sauron-test` all correctly redisplayed all four fixture Pods afterward.
M1 is now ACCEPTED for: live discovery/watch/table, namespace switching, generic/CRD
resource resolution, filters, YAML redaction, Explain, Events, Pod log follow, resize
resilience, and terminal restore on quit — all directly observed against the isolated
cluster, not inferred from unit tests.

### 2026-09-15 — remaining M1 slices observed: previous logs, explicit container,
### context switching, all-namespaces, sustained session; a third bug found and fixed
Closed out the previously-untested M1 acceptance items, all against `kind-sauron-test`:
`p` (previous logs) on `crashloop` correctly showed the terminated container's last line
and ended cleanly; `:logs worker` (explicit container) streamed the named container's
live output; `0` (all-namespaces) correctly relisted 13 real Pods across `kube-system`,
`sauron-fixtures` and `local-path-storage` with a NAMESPACE column and `<all>` header;
`:ctx` switching was exercised by temporarily adding a second context
(`kind-sauron-test-alias`, same cluster/user, different default namespace) to the
isolated `.test-cluster/config` with `kubectl config set-context` — never touching the
default kubeconfig — and switching to it reconnected correctly and relisted the same
fixtures; the added context was removed again immediately after. A ~150s continuous
soak of the live watch showed AGE advancing correctly, RSS ~25MB, ~2% CPU, no crash, and
a clean terminal restore on quit (not a multi-hour endurance run, but sustained beyond a
single interaction).

While testing explicit container logs, found that `open_logs` never resets
`state.status`, so opening a new log stream kept showing whatever status text was left
over from the previous action (observed as "Log stream ended" displayed while a brand new
follow session was actively streaming live lines) — misleading, since it implies the
stream already ended when it has not. Fixed by setting `status` to "Streaming logs · Esc
returns" at the start of `open_logs`, mirroring the pattern `open_document` already uses.
Re-ran `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, and
`cargo test --all-targets` (23/23) after the fix; verified live that opening a new log
view after a previous one had ended no longer shows stale "ended" text.

M1 is now ACCEPTED for all slices listed above, including previous/explicit-container
logs, context switching, and all-namespaces mode. Remaining gap: this was a single
~150s soak, not a multi-hour/overnight endurance run, and only one namespace/context
combination was exercised per feature — broader combinatorial coverage is still open.

## M2 plan and discipline

Agreed order, chosen to minimize risk: (1) real context picker with correct per-context
namespace memory and no state leak; (2) full namespace navigation (selector, `<all>`,
favorites/recents if in scope, rapid repeated switches to hunt races); (3) generic
resources + CRDs + aliases/shortnames resolving deterministically, including ambiguous
names; (4) navigation history/breadcrumbs preserving identity and scope across
enter/exit; (5) command palette centralized on the action registry, not scattered
special-cased commands; (6) M2 interactive acceptance against `kind-sauron-test` trying
to break it, not just demonstrating the happy path.

Deliberate ugly cases to attack for each M2 slice (this is where the next bugs are):
`namespace A → all → namespace B → all`; `context A → context B → context A`;
`pods → CRD → deployments → back → forward`; `:po → :pods → ambiguous-alias`; changing
scope while the previous watch is still settling; rapid repeated context switches;
select an object → change scope → return.

Two rules added to `AGENTS.md`, both earned by M1: (1) when live acceptance finds a real
bug, find the root cause, add a regression test when reasonable, run the full suite, and
repeat the exact discovering flow before moving on; (2) when changing context, namespace,
resource scope, or another identity boundary, actively test for stale asynchronous
results from the previous scope — a result may only update visible state when its
epoch/request identity still matches the current view, and this must be tested under
rapid repeated switching, not only a single clean transition.

M2 is not done because it compiles. It is done when contexts, namespaces, CRDs/aliases,
navigation and the command palette have been observed working for real and late-arriving
results from an abandoned scope never contaminate the current view.

### 2026-09-15 — M2 item 1: real context picker with namespace-per-context memory
Replaced `:ctx` (no args)'s static text dump with a real interactive picker: a new
`Mode::Picker(Picker)` (`title`, `items`, `active` index, `cursor`) rendered as a
selectable list with the current context marked (`●`) and the cursor row reverse-video;
Up/Down moves, Enter switches, Esc cancels. `:ctx NAME` still works directly for muscle
memory/scripting; both paths go through a new `Runtime::switch_context`.

Added namespace-per-context memory: `Runtime.namespace_by_context: HashMap<String,
Option<String>>`. Before switching, the outgoing context's current namespace (including
`None` for all-namespaces) is remembered; the incoming context's namespace is restored
from memory if this session has visited it before, otherwise it falls through to the
existing `Some("")` sentinel that `Payload::Connected` resolves to the connection's
kubeconfig default. The lookup is a pure function, `namespace_for_context`, unit tested
for both the unvisited case and the visited-including-all-namespaces case (the two are
easy to conflate: `HashMap::get` returning `None` means "never visited", `Some(None)`
means "visited, was in all-namespaces" — collapsing that distinction was the obvious way
to get this wrong).

Live acceptance against `kind-sauron-test`, attacking the case list above rather than
just the happy path: added a second context (`kind-sauron-test-b`, same cluster/user,
different default namespace) via `kubectl config set-context` on the isolated
kubeconfig — now a permanent, idempotent step in `scripts/test-cluster.sh fixtures`, not
an ad hoc one-off. Verified: opening the picker lists both contexts with the active one
marked; switching context A → B → A restores A's `sauron-fixtures` namespace correctly;
setting B to all-namespaces, going to A, then back to B via the picker restores B's
all-namespaces view (not B's kubeconfig-default `kube-system`) — the core thing this
feature exists to get right. Stress-tested rapid repeated switching per the new
stale-result rule: three rounds of 8 back-to-back `:ctx` commands with no settling time
between them, plus ten rounds of reopen-picker-and-switch with no delay, all converged
to a fully consistent final state (correct context, correct namespace, correct row data,
no partial/mixed rows, no stuck Loading/Picker mode) — no stale-epoch leak found. RSS
~27MB after the stress run, no crash, clean terminal restore on quit.
Re-ran `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, and
`cargo test --all-targets` (25/25, two new) after implementing; the temporary second
context was removed and the kubeconfig restored before committing, then recreated
through the updated `scripts/test-cluster.sh fixtures` to confirm the reproducible path
works too.

### 2026-09-15 — M2 item 2: full namespace navigation, and a fourth bug (stale
### key dispatch during Mode::Loading)
Replaced `n` (`Action::Namespaces`)'s old behavior — navigating away to a `namespaces`
resource table you had to remember names from and retype `:ns NAME` against — with the
same real-picker treatment as the context picker: a one-shot bounded list call
(`kube::discovery::list_names`, limit 500, same pattern as related-Events reads) fetches
the cluster's actual namespace names without disturbing whatever resource was on screen,
then opens `Mode::Picker(PickerKind::Namespace)`. `<all>` is always first; namespaces
this session has recently switched to (a `Runtime.recent_namespaces` MRU, capped at 5,
cleared on context switch since namespaces are context-specific) come next, then the
rest alphabetically. `:ns` bare now opens the same picker (`Command::Namespace` became
`Option<String>`, mirroring `Command::Context`); `:ns NAME`/`:ns *`/`0` (all-namespaces)
still work directly through a new shared `Runtime::switch_namespace`.

Live acceptance against `kind-sauron-test`, attacking the case list from the M2 plan:
`namespace A → all → namespace B → all` with settle time between each step — correct
every time. Rapid repeated switching with zero settle time: three rounds of five
back-to-back `:ns` commands (concrete → all → concrete → concrete → all) — every round
converged to the correct final namespace and row count, no stale data. Reopening the
picker after several switches confirmed MRU ordering (`<all>`, most-recently-used
namespaces in order, then the alphabetical rest) and correct `●` active-marking.

Found and fixed a real bug this way, exactly the kind the new AGENTS.md stale-result
rule exists to catch: `Mode::Loading` (used while connecting, watching, or — now —
fetching the namespace list) had no dedicated key handling, so it fell into the generic
table-action fallback. A key pressed while a fetch was still in flight (e.g. `y` right
after `n`, before the namespace list arrived) got dispatched as a real table action
(`Action::Yaml`) against whatever row was selected *before* the fetch started, silently
cancelling the in-flight fetch's shared cancellation token as a side effect and landing
the user in an unrelated YAML view instead of the picker they were waiting for. No data
corruption — the epoch/request system correctly invalidated the superseded fetch — but
a real, user-visible violation of "a result may only affect the view it was requested
for," extended here to "an *input* meant for a pending view must not act on the old one
either." Fixed by giving `Mode::Loading` its own arm: while loading, only Esc acts
(cancelling the fetch and returning to `Table`, matching the "Esc cancels" text the UI
already showed); every other key is a no-op. Confirmed with `eprintln!` instrumentation
that this was a stale key-vs-async-completion race, not a logic error in the picker
itself, before writing the fix; removed the instrumentation before committing.
Re-verified live in a **fresh** process (the first re-test after the fix still failed
because the old tmux pane was running the pre-fix binary — a reminder to always restart
the process under test, not just rebuild it) — with a fresh binary, `n` immediately
followed by `y` correctly stays in the picker, and eight rounds of rapid
reopen-picker-and-select converged to a valid, consistent table every time.
Re-ran `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, and
`cargo test --all-targets` (25/25) after the fix.

M2 item 2 is ACCEPTED: namespace selector, `<all>`, recents, and rapid-switch resilience
all observed live. Favorites (persisted, cross-session) were considered and deliberately
left out — recents already cover the natural, low-risk case, and persistence would need
config-file changes out of scope for this item. ### 2026-09-15 — M2 item 3: canonical resource identity, deterministic alias/CRD
### resolution, and a false-alarm crash investigation
Structural fix first, requested explicitly: resource resolution must produce a
canonical GVK identity before any watch starts, and nothing downstream may re-derive
that identity from a human alias against a catalog that might have changed. Concretely,
`Runtime::watch()` used to re-call `catalog.resolve(&state.query.resource, ...)` on
every Refresh, namespace switch, and post-reconnect rewatch — meaning a name
disambiguated once was never actually "locked in." Split into three methods:
`watch_resource(resource: Resource)` (the only place that starts a watch, always given
an already-resolved identity), `watch()` (reuses `state.resource`, for Refresh/
namespace/all-namespaces — same catalog, must not re-resolve), and `rewatch()`
(re-resolves `state.query.resource`'s text against the *current* connection's catalog,
used only right after a context switch with no pending explicit navigation, since
crossing to a different cluster is exactly when re-resolving the same name is correct).
`navigate()` now resolves once and calls `watch_resource` directly. Also fixed two
UI spots that displayed the raw alias/text instead of the resolved canonical name: the
top header ("Resource: po" → "Resource: pods") and the table border title, both now
read `state.resource.map(Resource::qualified)`.

Rewrote `Catalog::resolve` for deterministic, non-silent ambiguity: it used to prefer
a core (empty-group) match silently over other candidates, and matched shortnames
case-sensitively with no ambiguity check at all. Now: explicit qualification (id form
or `plural.group`) always wins outright, but only when the input is *actually*
qualified (contains `/` or `.`) — a bare word like "widgets" must not accidentally
match a core resource's `qualified()` just because a core resource's qualified form
happens to have no group suffix (this was a second silent-preference bug introduced
while fixing the first one, caught by the very unit test written to catch the first
one). Plural/kind matches and shortname matches are each deduplicated by GVK id and
bail with the full list of `plural.group` options whenever more than one distinct
resource matches — never a silent pick, even when one candidate is core. The 12
built-in aliases (`po`, `dp`/`deploy`, `svc`, ...) are now a shared `BUILTIN_ALIASES`
table consulted case-insensitively before live discovery, matching kubectl precedent
that they always win over a colliding CRD shortname; `discover()` now emits a
`catalog.warnings` entry when a live CRD's shortname is shadowed this way, visible via
`:info`, so the shadowing is never silent. A not-found error now says "discovery was
partial, this is not proof it doesn't exist" instead of a flat not-found when
`catalog.warnings` is non-empty. 6 new unit tests, including one that specifically
encodes case-insensitivity for the built-in table (see the bug below).

Added `tests/fixtures/ambiguous.yaml` + `ambiguous-instances.yaml`, applied by
`scripts/test-cluster.sh fixtures`: `portals.a.sauron.test` (shortname `po`, colliding
with the built-in), two CRDs both named `widgets`/kind `Widget` in different groups (a
genuine cross-group collision), and a cluster-scoped `probes.a.sauron.test` CRD.

Live acceptance against `kind-sauron-test`, attacking the case list given for this item:
`:po`/`:pods` resolve to the identical GVK; `:widgets` (ambiguous) is rejected with both
`plural.group` options listed, never silently picked, and `:widgets.a.sauron.test`
disambiguates correctly to only that group's object; `:probes` shows
"Namespace: n/a (cluster-scoped)" with no NAMESPACE column, confirming the
cluster-scoped-vs-namespaced UI fix; the `po` shortname collision warning is visible via
`:info`; deleting a CRD while actively watching it produces a visible WatchError, not a
crash, and rows go to 0; deleting a CRD and then navigating to it for the first time
(discovery cache still lists it, live API doesn't) produces a clean "404 Not Found"
`WatchError`, not a crash; rapid repeated resource switching between `pods`/`eyes`/
`probes`/`widgets.a.sauron.test` with zero settle time (three rounds) always converged
to the correct final resource with no stale rows.

Found and fixed a real bug this way: `BUILTIN_ALIASES` was matched case-sensitively
(`*alias == query`), so `po` deterministically won (matching kubectl), but `PO` missed
the built-in table entirely and fell through to genuine, *correct* discovery-based
ambiguity detection against the new `portals` fixture (which also declares shortname
`po`) — reporting an ambiguity error for `PO` that `po` never hit. Inconsistent:
case-insensitivity has to be uniform or the built-in table's "always wins" guarantee
silently stops applying depending on how you capitalize. Fixed by matching
`BUILTIN_ALIASES` with `eq_ignore_ascii_case`. Regression test added with all four
casings against a catalog that includes the colliding CRD.

Spent real effort chasing what looked like a crash: after a successful `:PO`, pressing
Escape "defensively" between commands, then typing a new command, would sometimes land
in a shell prompt instead of the app, with the typed text executed as a shell command.
No panic ever appeared in stderr across several `eprintln!`-instrumented repro attempts.
Root cause, once isolated to the minimal sequence: `Action::Back` is bound to `esc,q`,
and its fallback branch (`Mode::Table`, no active filter) sets `state.quit = true` —
intentional "Esc at the root quits" behavior, the same pattern k9s and similar TUIs use.
It was never a crash; it was the app correctly quitting because Escape was pressed while
already at the top level with nothing left to back out of, and the test script's own
"press Escape between commands to be safe" habit is exactly the input pattern that
triggers it. No code change; noted here because it cost real time and is worth knowing
before the next round of adversarial live testing: don't press Escape reflexively at
Table with an empty filter unless you mean to quit.

Re-ran `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, and
`cargo test --all-targets` (29/29, six new) after all the above.

M2 item 3 is ACCEPTED: canonical GVK identity end-to-end, deterministic non-silent
alias/shortname/CRD resolution (case-insensitive, ambiguity-checked, cross-group-safe),
cluster-scoped-vs-namespaced UI correctness, and graceful handling of a CRD deleted
mid-session — all observed live, not inferred from unit tests. `pods → CRD →
deployments → back → forward` restoring resource+namespace+context+selection is
explicitly NOT covered here: there is no resource-level navigation history yet
(`Action::Back` only leaves document mode, clears a filter, or quits) — that is M2 item
4, next.

### 2026-09-15 — M2 item 4: navigation history/breadcrumbs
Followed the user's rule exactly: history stores semantic navigation *intent*, never a
state snapshot. `state::HistoryEntry` is six short fields — `context`, `namespace`,
`resource` (the canonical `Resource::qualified()` string, never a raw alias, so a
restore re-resolves deterministically even across catalog changes), `filter_text`,
`sort`, `descending`, and `selected` (a UID, not a name — same-name-different-UID must
not reselect, matching the existing `rebuild()` invariant). No store/rows/watch data
ever touches it. `Runtime` holds two `VecDeque<HistoryEntry>` (`history`/`forward`,
classic browser stacks) fixed at `HISTORY_LIMIT = 100` from the start via a pure,
unit-tested `push_capped` helper — never grows unbounded across a long session.

Push points are deliberately narrow: `Command::Resource` (typing a new resource),
`switch_namespace`, and `switch_context` — each pushes the view being *left* before
applying the change, then clears `forward` (taking a new path invalidates old redo,
standard back/forward semantics). Refresh, sort/filter/wide tweaks, and opening/
closing a document or log view do NOT push — they don't call any of those three, so
they're excluded by construction, not by a special case. New keys `[`/`]`
(`Action::HistoryBack`/`HistoryForward`, "navigation" scope) pop a stack entry, push
the current view onto the other stack, and call `apply_history`: reconnect only if the
entry's context differs from the current one (`pending_history`, mirroring the
existing `pending`/context-switch-replay mechanism), otherwise re-resolve `resource`
against whatever catalog is current and start a fresh watch directly. Never revives
old rows — the normal watch/rebuild pipeline repopulates real data every time.

Header/scope line replaced with the requested compact breadcrumb: `ctx:X › ns:Y ›
resource`, or `ctx:X › resource` with no `ns:` segment for a cluster-scoped resource.
Always the resolved canonical name, never the typed alias (`:ey` shows
`eyes.testing.sauron.local`).

Live acceptance against `kind-sauron-test`, attacking the full case list given for
this item: `pods → eyes(CRD) → deployments → back → back → forward → forward` walked
the stack exactly as expected; back after a namespace change and after a context
change both restored correctly; `<all>` vs a concrete namespace round-tripped
correctly through back/forward; opening a YAML view and returning via Esc did not add
or disturb any history entry; two/three Refreshes in a row followed by one Back
returned directly to the prior view, confirming Refresh never pushes; going back to a
CRD deleted mid-session (or before ever watching it again) produced a clean
`WatchError`/404, not a crash, consistent with the M2-item-3 finding; 15 rapid `[`
presses past the actual stack depth, and 15 rapid `]` presses back, both stopped
cleanly at the real boundary with no crash (`pop_back` on an exhausted stack is a
no-op by construction); a breadcrumb at 35x10 clipped instead of panicking or
corrupting the layout.

Found and fixed a real bug this way: restoring a history entry set `state.resource`
(the canonical `Resource`, used for display and for the watch itself) correctly, but
never updated `state.query.resource` (the *text* field `rewatch()` re-resolves against
a *new* catalog after a context switch). Sequence that exposed it: navigate to `pods`,
then `eyes`, Back to `pods` (now `state.resource` = pods, but `query.resource` was
still last set by the `eyes` navigation), then switch context — `rewatch()` used the
stale `query.resource` text and landed on `eyes` on the new context instead of `pods`.
Fixed by also setting `state.query.resource = entry.resource.clone()` in
`finish_history`. Also chased what looked like a same-name-different-UID selection
bug (deleted and recreated a CRD instance with a new UID, went Back, and initially saw
the row rendered as selected) — turned out to be leftover confusion from an extremely
long single test session with many overlapping back/forward/context/namespace
operations, not a real bug: a clean, isolated re-run with `eprintln!` instrumentation
confirmed `rebuild()` correctly cleared `selected` to `None` when the remembered UID
wasn't among the fresh rows (`selected_before=Some(old-uid)`, `row_uids=[new-uid]`,
`selected_after=None`), exactly per the existing invariant. Instrumentation removed
before committing either way.

Re-ran `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, and
`cargo test --all-targets` (31/31, two new for `push_capped`) after the fix.

M2 item 4 is ACCEPTED: bounded, semantic-intent-only navigation history with working
back/forward across resource/namespace/context changes, a compact canonical
breadcrumb, and no crashes or stale-data leaks under adversarial rapid use — all
observed live.

### 2026-09-15 — M2 item 5: command palette unified on the action registry
Found and removed real duplication, exactly the class of bug the user's rules for this
item were written to catch. Before this item there were THREE separate, drifting
name->behavior tables: `registry()` (the real one, driving keybindings and effective
help), a hardcoded `match` inside `command::parse` mapping a handful of command names
to `Action`s, and a separate `COMMANDS` constant used only for autocomplete
suggestions. The hardcoded `parse` match only covered 7 of the registry's ~30 actions
(most navigation/table/document actions like `sort`, `reverse`, `wide`, `wrap`,
`fullscreen`, `search_next/previous`, `history_back/forward`, `down/up/first/last`
were silently unreachable by name even though they had real key bindings), and
`COMMANDS` was a third, independently-maintained partial list.

Removed both. `parse` now resolves a typed name by looking it up directly in
`registry()` — the SAME list `Keymap` compiles keybindings from and `Keymap::help()`
renders — so a name is reachable by typing it if and only if it is a registered
action; there is no second table to fall out of sync. `command::command_names()`
replaces `COMMANDS`, built from `registry()` plus the handful of argument-taking
commands (`ctx`, `ns`, `info`, `reload`, `sort` with an argument, `logs`/
`previous_logs` with a container) that have no zero-arg `Action` equivalent and so
are necessarily handled before the generic registry lookup.

That registry-wide lookup immediately surfaced a real, previously invisible name
collision: `sort` is both a registered *action* (key `S`, cycles to the next column,
no argument) and a *command* with its own argument syntax (`:sort column[:asc|:desc]`)
that predates this item — same name, different behavior depending on whether you
pressed the key or typed the word. `:logs`/`:previous_logs` had the same shape
(action vs. optional-container command). Fixed uniformly: bare `:sort`/`:logs`/
`:previous_logs` now resolve to the exact same `Action` as their key binding; only an
explicit argument takes the argument-taking path. A new test,
`every_registered_action_name_resolves_via_parse_to_the_same_action`, asserts this
holds for literally every entry in `registry()` — it would have caught the `sort`
collision immediately, and now guards against any future one. A second test,
`command_names_never_silently_drops_a_registered_action`, asserts the reverse: nothing
in the registry is missing from what the palette suggests.

Also found and fixed a duplicated hardcoded string that isn't an `Action` name but is
the same class of bug: the idle-mode hint bar (` : commands  / filter  X explain  y
YAML  l logs  ? help`) was a literal string in `src/ui/mod.rs`, completely
disconnected from the actual effective keymap — a user remapping `yaml` in config
would see a hint bar still claiming `y` opens it. Replaced with `hint_bar()`, built
from `state.keymap.primary_key(Action::X)` for each hinted action (a new `Keymap`
method) — verified live with a real config override remapping `yaml` to `z`: the hint
bar correctly showed "z YAML" instead of the old hardcoded "y YAML".

Also added a mode gate the palette didn't have before: a key binding is naturally
mode-scoped (`Keymap::action` only matches bindings for the current mode), but a typed
`Command::Action` bypassed that entirely and would silently do nothing for a
mode-inappropriate action (e.g. typing `:search_next` while not viewing a document) —
found live while working through the user's "comandos deshabilitados según modo" case.
Fixed by checking the action's own registered `mode` against the current UI mode at
dispatch time in `command()` and returning a real error ("`search_next` is not
available in table mode") instead of a silent no-op.

Live acceptance against `kind-sauron-test`, attacking the full case list for this
item: rapid open/close of the palette (10 cycles at zero delay) never got stuck or
quit — an earlier attempt with 0.15s inter-key sleeps DID appear to fail intermittently,
traced to tmux/test-harness subprocess timing, not the app (0.3s sleeps and a
step-by-step re-run were reliably clean; noted as a harness sensitivity, not a bug);
fuzzy search (`expln` suggesting `explain`) and Tab-completion both work; a full
action name executes identically whether typed or pressed (`X` vs `:explain` on the
same pod produced the same Explain report); an action requiring a selection with none
present shows "Select a row first", not a crash, exactly as before; executing a
scope-changing command and immediately reopening the palette works cleanly; a 35x10
(then 30x8 with the palette open) terminal clips without panicking; the suggestion
popup is capped at 8 fuzzy matches by design, never literally scrolls, so "long enough
list to scroll" is satisfied by the existing bound rather than a new scroll mechanism.

One thing chased and *not* fixed, documented instead as an existing, coherent design
rather than a bug: opening the palette from within a document view (`Action::Palette`)
unconditionally returns to the table underneath, discarding the document — meaning a
document-scoped action like `search_next` can never actually be typed by name while
"in" a document, since opening the palette to type it has already left document mode
by the time the command runs. This matches `Action::Back`'s existing behavior (Esc from
a document also always returns to the table, never to some other prior document) —
one overlay level, always returns to the table, a simple and already-established
mental model, not something introduced by or in scope for this item.

Re-ran `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, and
`cargo test --all-targets` (34/34, three new) after all of the above.

M2 item 5 is ACCEPTED: one action registry drives keybindings, effective help, the
hint bar, and the command palette, with no second hardcoded table anywhere and a
regression test enforcing it going forward.

### 2026-09-15 — M2 item 6: whole-of-M2 interactive acceptance, combined sequences
Not individual features in isolation — items 1-5 exercised together, in the
combinations most likely to expose interaction bugs between them, against
`kind-sauron-test` + `kind-sauron-test-b` (same physical cluster, two contexts) with
the full ambiguous/cluster-scoped CRD fixture set. Three global criteria judged every
sequence: never mix state from different scopes, never silently resolve an ambiguous
identity, never show help/state that contradicts what a real action would do.

Ran the full combined-sequence list: context A → namespace X → CRD → filter → back →
context B → forward (each step showed the correct real data for its exact
context+namespace+resource, filter correctly cleared/restored across the boundary,
forward correctly no-op'd after a new navigation cleared it — expected stack
semantics, not a bug); palette open → rapid scope change typed inside it → immediate
action execution; key remap → help → hint bar → key execution → name execution, all
four agreeing (`z` for Explain everywhere, verified live); ambiguous `:widgets` →
qualified `:widgets.a.sauron.test` → context switch (same physical cluster, so the
canonical resource re-resolved cleanly; correctly showed zero rows for the new
context's different default namespace, not stale rows from the old one) → back
restored the original namespace's real data; `<all>` → concrete namespace → picker
(showing recents) → context switch → back/forward; deleting a CRD that was sitting in
the back stack, then navigating back into it — clean 404, not a crash; switching
context from the picker while the previously-viewed resource had just started a fresh
successful watch — no interference, correct data for the new context; three rounds of
combined `:ns` + back + `:ctx` + back + forward with zero settle time (first attempt
used a cluster-scoped resource, which made the test visually inconclusive since
namespace has no effect on cluster-scoped data or its breadcrumb — redone with `pods`,
a namespaced resource, converging to correct, real, coherent data every time); a
32x9 terminal with the namespace picker, then the palette with a long resource name
being typed, then after navigating with a long breadcrumb — all clipped without
panicking; Refresh interleaved with context/namespace navigation, then confirmed via
Back that the refreshes never added spurious history entries.

No new bugs found in this pass — a meaningful result on its own: it means the fixes
from items 1 through 5 compose correctly under combined, adversarial use, not just
individually. Re-ran `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`,
and `cargo test --all-targets` (34/34) after reconciling fixtures back to a clean
state.

**M2 is ACCEPTED** against all three global criteria, observed under combined
sequences, not just each item's own isolated acceptance:
- Never mixed state from different scopes — every combination of context/namespace/
  resource/filter change showed exactly the real data for its own exact combination,
  including the "looks unchanged" cluster-scoped case, which was verified to be
  correct-and-boring rather than broken.
- Never silently resolved an ambiguous identity — `:widgets` errored with both
  `plural.group` options every time it was tried, including mid-sequence, never once
  silently picking one.
- Never showed help/state contradicting the real available action — hint bar, help
  screen, key execution and name execution all agreed with each other and with a live
  config remap in every combination tried.

Tagged locally as `m2-accepted` (see RUNBOOK), the same lightweight checkpoint pattern
as `m1-accepted`. Next milestone: M3, not yet planned in detail.
