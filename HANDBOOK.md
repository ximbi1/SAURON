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
| Build, CLI, tracing | IMPLEMENTING | next slice |
| Kubeconfig/discovery/live resource store/table | ACCEPTED | observed live against kind-sauron-test; Refresh-after-refresh table-emptying bug found and fixed (see journal) |
| Context/namespace/generic discovery/commands | ACCEPTED | context picker w/ per-context namespace memory (M2 item 1) and namespace picker w/ recents (M2 item 2) both observed live; history/breadcrumbs still pending (M2 item 4) |
| Filters/sort/documents/events | ACCEPTED | regex fix, text filter, YAML, Explain, Events all observed live; sort cycling observed, not exhaustively |
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
As of 2026-09-15: `cargo fmt --check` clean, `cargo check --all-targets` clean,
`cargo clippy --all-targets -- -D warnings` clean (0 lints), `cargo test --all-targets`
18/18 passing (15 unit, 3 fake-HTTP integration in `tests/watch_transport.rs`; the live
`tests/cluster.rs` acceptance test is `#[ignore]`d pending an explicit isolated-cluster run).
Debug-profile bench sample on the development machine: 100 objects 11.5ms build /
0.5ms filter+sort / 4.4ms render; 1,000 objects 107ms / 5.4ms / 14.6ms; 5,000 objects
472ms / 24.8ms / 53.0ms. Debug build, single run, no release-profile or repeated-sample
data yet — not a performance claim, a recorded local measurement only.

## Known limitations and bugs

Initial implementation exists; the checks above are green. Broad parity is a long-term
backlog. Existing machine: rustc/cargo 1.95, 15 GiB RAM with other workloads; keep build
parallelism modest. Docker available. Existing production API `/version` read succeeded
(Kubernetes v1.33.4). No sensitive resource contents collected in research.
Visual/TUI acceptance against the live isolated cluster is still pending — green checks
are unit/fake-HTTP evidence, not a demonstrated interactive session. Log reader caps
allocation before clipping (16 KiB per line, 4 KiB read chunks); covered by the fake-HTTP
watch tests but not yet by a dedicated log-clipping regression test. Row rebuild is gated
by a `prepare()` memo keyed on store revision/filter/sort/descending, plus the current
second only when the filter has an `age` comparison (see journal entry below — an earlier
version included the clock unconditionally and resorted every tick regardless of scope). Other open concerns: document wrap
scroll semantics, scope selection during empty filters. Watch list synchronization is
labeled separately from an established watch; no header-level connection probe yet.
No in-cluster config fallback, context/namespace pickers, server Tables, CRD printer columns,
multi-container log fanout, metrics, graph, or mutations yet. Core Event reads cap at 200
and report truncation. Describe is SAURON's contextual native report, not kubectl parity.

## Current milestone / continuation instructions

M1 implementation is FUNCTIONAL and TESTED against fake HTTP + unit coverage, but not yet
ACCEPTED: no demonstrated interactive TUI session against the live isolated cluster has
been recorded in this handbook. M2/M3 inspection code is also present but not accepted.
The regex cell-matching bug and the 8 original Clippy lints are fixed; all checks listed
above are currently green. `tests/cluster.rs` (live kind-sauron-test acceptance) exists
but has not been executed and run to completion in this handbook's record.
Next: run `bash scripts/test-cluster.sh fixtures` then `bash scripts/test-cluster.sh test`
against `kind-sauron-test`; drive the real TUI interactively (watch/recovery, namespace and
context switching, terminal restore on exit/resize, Pod logs follow/previous) and record
what was actually observed. Only mark a slice ACCEPTED after that direct observation.
Re-read this handbook at phase boundaries. Never mark broader milestones done from
isolated unit tests alone. Keep buildable handoffs.

## Journal

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
config-file changes out of scope for this item. Next: M2 item 3, generic resources/CRDs/
aliases resolving deterministically, attacking `:po → :pods → ambiguous-alias` and
`pods → CRD → deployments → back → forward`.
