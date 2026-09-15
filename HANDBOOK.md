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
| Kubeconfig/discovery/live resource store/table | IMPLEMENTING | cargo check passes; real M1 acceptance pending |
| Context/namespace/generic discovery/commands | IMPLEMENTING | command navigation implemented; picker/history pending |
| Filters/sort/documents/events | IMPLEMENTING | regex cell-matching regression fixed; all unit tests pass; full acceptance pending |
| Pod logs | IMPLEMENTING | native follow/previous; explicit container command; validation pending |
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
