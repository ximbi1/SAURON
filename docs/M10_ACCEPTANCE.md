# M10 — Bulk/Workspaces/Bookmarks/Themes/Keymaps: acceptance ledger

Status: **PLANNED — NOT STARTED**. This document is M10.0: the scope-freeze
contract written before any M10 implementation code, exactly like
`docs/M8B_ACCEPTANCE.md` was written before M8B and `docs/M9_ACCEPTANCE.md`
before M9. It records what reconnaissance found in the existing codebase,
which primitives every later slice must reuse rather than fork, and the
proposed slice ledger. Nothing in this file authorizes touching code,
cluster, or CI — only the slices marked ACCEPTED in the Journal, once this
contract exists, authorize implementation work.

`docs/M9B_A_ACCEPTANCE.md` and `docs/M9B_B_ACCEPTANCE.md` are separate,
PLANNED — NOT STARTED ledgers for deferred Helm mutation work; M10 does not
touch, schedule, or depend on them.

## Purpose

M1-M9 built a Kubernetes/operator console: observation (watch/list) ->
evidence (M5 health, M6 relationships) -> guarded single-target action
(M7/M8/M8B mutation model, extended to Flux/Argo/Helm in M9) -> journal ->
verification. Every milestone through M9 operates on exactly one object at
a time. M10 is the first milestone whose deliverable is explicitly
*plural*: select many identifiable objects, act on them as individually
tracked operations, and give the user durable ways to organize/return to
cluster views (workspaces), specific objects (bookmarks), and their own
terminal ergonomics (keymaps, themes). Per `docs/ROADMAP.md`'s own M10
line, the deliverable contract is: "Invalid reload retains policy/keymap;
per-context scope; effective help; mark identity."

M10 is explicitly **not** "a new bulk-mutation engine" and **not** "a
second config system." Every slice composes what M1-M9 already built. If a
candidate M10 feature cannot be expressed as composition of the existing
selection/mutation-gateway/config/command-registry/theme primitives, this
document narrows or defers that feature rather than forking the
architecture to fit it (see Explicit design rule, below — the same
discipline M9's own kickoff prompt established).

## Relationship to existing milestones

M1-M9 are ACCEPTED (tag `m9-accepted`, `f2a18d0`) and M10 must not regress
any of them: existing single-object selection, mutation workflows,
command grammar, config resolution, and rendering must keep working
identically when M10 features are not in use. M9B-A/M9B-B remain PLANNED
— NOT STARTED and out of scope for M10 entirely — not read, not extended,
not referenced by M10 code.

Concretely, M10 reuses:

- **Selection/identity** — `app::state::State.selected: Option<String>` is
  already a **UID**, not a row index or a name (`src/app/state.rs:63`,
  with the module comment at line 39 explicitly documenting "same-name-
  different-UID must not reselect"). `State::rebuild()`/relist logic
  already clears `selected` when the UID it names is no longer present
  (`src/app/state.rs` around line 270-283) rather than silently rebinding
  to a same-name replacement. `HistoryEntry.selected` (line 51) already
  carries this same UID convention across back/forward navigation, and is
  explicitly documented as "semantic navigation intent... never a state
  snapshot." M10.1's multi-select is a **superset** of this exact
  invariant (a `BTreeSet`/ordered collection of the same identity kind
  the single-selection model already uses), not a parallel selection
  concept.
- **Full target identity for mutation** — `app::session::Scope` (context,
  cluster, resource id, namespace, name, uid, epoch, request —
  `src/app/session.rs:27`) is the identity unit every mutation, log
  session, exec session, and port-forward already keys off. Multi-select
  needs this full `Scope`-shaped identity (not just a UID string) because
  a bulk operation must survive being reasoned about outside the single
  current table view's implicit context/namespace/resource. M10.1 defines
  its selected-target type in terms of (or literally reusing) `Scope`.
- **Mutation gateway** — `mutation::{MutationIntent, MutationTarget,
  MutationEffect, MutationRisk, PolicyDecision, PolicyReason,
  PolicyEvaluation, ConfirmationRequirement, Confirmation,
  MutationOutcome, Verification}` (`src/mutation.rs`), the single policy
  engine `mutation::policy::evaluate` (`src/mutation/policy.rs`), the
  shared `mutation::workflow::{Built, Workflow}` shell every action
  builder returns (`src/mutation/workflow.rs`), and the single execution
  gateway `kube::mutation::{preflight, commit, verify}`
  (`src/kube/mutation.rs:256,400,655`). M10.3 (bulk guarded mutations)
  must build *N* `Workflow`s — one fully independent
  intent/evaluation/preview/confirm/commit/verify per target — and drive
  them through this exact gateway in a loop/bounded-concurrency
  orchestrator. It must not invent a second policy engine, a second
  `Confirmation`/TOCTOU mechanism, or a second executor. `Confirmation`
  already binds to `(request_id, scope, effect, payload_sha256)`
  (`src/mutation.rs:129-144`) — a bulk confirmation is naturally modeled
  as one `Confirmation` per target sharing the same bulk `request_id`
  prefix/correlation, not one confirmation authorizing many scopes.
- **Journal** — `mutation::journal::{Journal, Phase, Record}`
  (`src/mutation/journal.rs`), append-only, redacted, schema-versioned.
  M10.3 adds no new file/format; a bulk run is a sequence of ordinary
  per-target `Record`s correlated by `request_id`, exactly like Drain's
  own `DrainStarted`/`DrainStep`/`DrainFinished` phases already bracket a
  sequence of ordinary per-Pod eviction records without replacing them
  (`src/mutation/journal.rs:33-38`) — the exact precedent M10.3's bulk
  bracket phases should follow.
- **Command registry** — `command::{Command, Action, Keymap, parse,
  command_names}` (`src/command/mod.rs`) is the single command
  grammar/dispatch table. `Action` is a static registry of `(action,
  name, mode, description, default keys)` tuples (`registry()`, consumed
  by `Keymap::compile` at `src/command/mod.rs:357`); `Keymap::compile`
  already merges user TOML overrides over these defaults, validates every
  key with `parse_key`, and already performs **deterministic per-mode
  conflict detection** (`src/command/mod.rs:371-383`, tested at
  `effective_bindings_detect_conflicts`, line ~1008). `Keymap::primary_key`
  (line 402) already exists specifically so UI help text reflects the
  *effective* (possibly user-overridden) binding, not a hardcoded
  default. M10.6 (configurable keymaps) is therefore almost entirely
  **already built** — reconnaissance found no separate keymap engine to
  design; the remaining gap is a `:keys`/help-overlay UI slice showing
  the resolved table, more thorough conflict/malformed-config
  diagnostics surfaced to the user (currently `compile()` returns an
  `anyhow::Error` on conflict, which must fail configuration load safely
  rather than crash startup), and confirming per-context/per-cluster
  scoping composes with `Config::resolve` (see below).
- **Config persistence** — `src/config.rs`'s `Config`/`Settings` is
  already the one coherent, versioned-by-convention config model:
  `Settings` (`#[serde(deny_unknown_fields)]`, so unknown keys are a load
  error, not silently ignored) holds `keys: BTreeMap<String,
  BTreeMap<String, Vec<String>>>` (keymap overrides), `theme: String`,
  `favorite_namespaces`, `aliases`, and more; `Config` layers `base` under
  per-`clusters`/per-`contexts` TOML tables merged by `Config::resolve`
  (`src/config.rs` — cluster layer then context layer, matching
  ROADMAP's own M10 line "per-context scope"). `Config::load` already
  treats "file absent" as default-safe and any malformed TOML/unknown
  field as an explicit load error with the raw source deliberately
  omitted from the message (credentials-safety precedent). M10.4
  (workspaces), M10.5 (bookmarks), M10.7 (themes), and M10.8 (integration/
  migration) must all extend `Settings`/`Config` — new fields, not a
  second file, second format, or second load path. This is the single
  biggest reuse finding of this reconnaissance: **there is already one
  config file, one resolution layering (base -> cluster -> context), one
  fail-safe-on-malformed policy, and one credentials-safety convention**,
  and M10 must extend it, never duplicate it.
- **UI/theme rendering** — `ui::Theme` (`src/ui/mod.rs:17`) is a struct of
  named `Color` fields (`foreground/background/accent/good/warning/
  critical/muted`) with three built-in constructors and a `severity(self,
  Severity) -> Color` mapping method (`src/ui/mod.rs:58-64`) that is
  **already** the closest thing to a semantic-role system in the
  codebase: callers ask for "the color for this `Severity`," not an
  arbitrary RGB value, everywhere health/status color is rendered
  (`src/ui/mod.rs:158`). There is no `ThemeRole` enum yet and no
  monochrome/no-color fallback — severities are currently distinguished
  by color alone in table rows (`Row::new(cells).style(Style::default()
  .fg(theme.severity(...)))`, line 158) with no accompanying glyph/text
  marker. M10.7 must add that non-color channel (this is a concrete,
  evidence-based non-goal-turned-requirement — see below) and should
  generalize `severity()`'s existing pattern into a small closed
  `ThemeRole` enum (`Normal/Muted/Selected/Warning/Critical/Unknown`,
  matching this task's own suggested shape) rather than adding new ad hoc
  `Style::default().fg(...)` call sites throughout the renderer.
- **Test harnesses** — `scripts/accept-m*.py` (regression scripts, one per
  milestone, run unmodified for M10.9's full regression), `scripts/soak-
  m*.py` (soak harness precedent for M10.9), `scripts/test-cluster.sh`
  (Docker/API loopback identity guard backing `mutation_test_cluster_
  verified`, `kind-sauron-test`) and `scripts/test-cluster-m9.sh` /
  `bootstrap-test-cluster-m9.sh` (the `sauron-m9` dedicated GitOps
  cluster). M10 live/mutation testing reuses these guard scripts
  unmodified — `kind-sauron-test` for M10.1-M10.3/M10.9 general bulk-
  mutation proof, `sauron-m9` only if a bulk test specifically needs
  Flux/Argo/Helm targets (unlikely; M10.3's matrix is expressed primarily
  in terms of ordinary namespaced resources already proven safe in
  `kind-sauron-test`).

## Explicit design rule (mirroring M9's own kickoff framing)

For every M10 feature, ask: does this reuse the existing selection model /
mutation gateway / config model / command registry / theme abstraction, or
does it fork a parallel one? If a feature only seems to require a fork,
this document must say so explicitly (see Open architectural questions)
rather than silently building the fork. M10 must not become "a TUI with a
bolted-on second settings system," "a bulk button that skips per-target
policy," or "a keymap engine that bypasses `Keymap::compile`."

## Invariants (SAUR-ON's own established vocabulary)

- **UNKNOWN != ZERO != HEALTHY** — a bulk aggregate with unverified/
  unknown-outcome members must never render as if it succeeded, and must
  never render as "0 affected" when the truth is "verification could not
  be attempted." `MutationOutcome::OutcomeUnknown` and
  `Verification::Unknown` already encode this per-target; bulk aggregation
  must preserve, never collapse, that distinction.
- **UID != NAME** — bulk selection identity is UID-based (composed with
  namespace/context/GVK), exactly like single selection already is
  (`src/app/state.rs:39`); no bulk feature may resolve or re-target by
  name alone.
- **RELATIONSHIP != CAUSE** — bookmarks/workspaces observing that two
  objects are related (owner refs, label selectors) never implies one
  caused the other's state; M10 does not add a new relationship-inference
  engine (that's M6's, reused as-is if ever touched).
- **COMMIT != OBSERVED EFFECT** — a bulk target's `MutationOutcome::
  Committed` is never itself proof of the desired end state; each target
  still gets its own independent `Verification`, exactly like single-
  target mutations already require.
- **REQUEST ACCEPTED != DESIRED EFFECT OBSERVED** — applies per-target in
  bulk exactly as it does today for a single mutation; a bulk operation's
  aggregate "N committed" is a request-accepted count, never conflated
  with "N verified converged."
- **LOOKS CORRECT != PROVEN CORRECT** — bulk preview rendering (a table of
  N targets and their proposed changes) is a rendering of intent, not
  proof of anything; only per-target `commit`/`verify` results are proof,
  and only for the targets actually executed.
- No second policy engine, mutation gateway, confirmation system, journal,
  or verification dispatcher — restated as a hard constraint on M10.3
  specifically, since bulk mutation is the one slice most likely to be
  tempted to build a parallel fast path.
- No hidden automatic mutation retries — a bulk run that hits a transient
  failure on target 7 of 20 does not retry target 7 automatically; it
  records the failure and continues (or stops, per the cancellation model
  M10.3 defines) to target 8. Retrying is always a distinct, explicit,
  user-initiated new intent.
- No name-prefix identity heuristics — bulk "select all matching prefix
  X" is explicitly out of scope (see non-goals); selection is always by
  exact identity (filter/search narrows the *visible* candidate set, but
  the resulting selection is still a bounded, explicit set of UIDs, never
  a live pattern re-evaluated at execution time).
- No context-name mutation authorization — `PolicyContext.
  cluster_verified_for_mutation` (`src/mutation/policy.rs:20`) remains the
  only source of mutation authorization; M10 workspaces/bookmarks must
  never persist or imply this flag (see M10.4/M10.5 contracts and the
  Safety boundary below).
- No runtime shell-out to kubectl/helm/flux/argocd — unchanged from every
  prior milestone; M10 adds no CLI subprocess anywhere, including
  workspace/keymap/theme file I/O (pure Rust file read/write only).
- Production remains READ-ONLY, always. Never push/publish without
  explicit authorization.

## Safety boundary

Production is read-only, always, for every M10 slice — this does not
change relative to M7/M8/M8B/M9. All mutation/live-write testing (M10.1's
selection-stability proof under a real relist, M10.3's bulk guarded
mutations, M10.9's combined acceptance) uses only the already-established,
externally-verified disposable kind test clusters: `kind-sauron-test`
(general mutation proof, via `scripts/test-cluster.sh`) and `sauron-m9`
(only if a bulk test genuinely needs a Flux/Argo/Helm target, via
`scripts/test-cluster-m9.sh`). Mutation safety is never inferred from
context name, `readonly` flag alone, namespace, "looks like localhost," or
a familiar cluster name — only `mutation_test_cluster_verified`, set only
by the CLI flag backed by the external Docker/API loopback proof already
implemented in `scripts/test-cluster.sh`, ever authorizes a live mutation
in tests. M10 must not weaken, bypass, or duplicate these guard scripts.

Workspaces/bookmarks (M10.4/M10.5) introduce a new, M10-specific safety
requirement beyond "don't mutate production": **persisted state must never
be able to accidentally restore mutation authorization**. Concretely nothing
persisted by M10 may include `mutation_test_cluster_verified`, an active
`Confirmation`, an in-flight `Workflow`, a stale UID treated as a live
target, or any credential/token/kubeconfig material. See M10.4's own
contract and the corresponding entry in Architectural questions.

## Explicit non-goals for M10

Based on what reconnaissance actually found (not a generic list):

- **No unbounded/global selection.** "Select all" always means "select
  all *currently visible* rows in the bounded, already-filtered table,"
  never a fresh unbounded cluster-wide scan. `State.store`/`Store::new`
  already bounds `max_objects`/`max_bytes` (`src/config.rs` defaults:
  20,000 objects / 128MiB) — M10.2's bulk selection reuses that existing
  bound, it does not introduce a second one, and never silently exceeds
  it by paginating beyond what the table already loaded.
- **No cross-resource-kind bulk mutation in one operation.** Given
  `MutationTarget.resource: Resource` is per-target and action builders
  (`scale`/`restart`/`delete`/...) are already kind-gated (`SCALE_KINDS`/
  `RESTART_KINDS`/`DELETE_KINDS` in `src/mutation/workflow.rs:16-27`), a
  single bulk action operates over a selection whose members are the
  *same* action-eligible kind; a selection mixing eligible/ineligible
  kinds surfaces those as explicit per-target `Unsupported` entries
  (mirroring `unsupported()` at `src/mutation/workflow.rs:76`), never a
  silent skip and never a forced narrowing the user didn't ask for.
- **No new relationship/health/timeline engine.** Bookmarks/workspaces
  reference objects; they do not compute or cache relationship/health
  facts — those are re-derived live from M5/M6 exactly as they are today
  when a bookmark is opened.
- **No "GitOps-aware bulk" special-casing.** M9's Flux/Argo/Helm guarded
  actions are explicitly excluded from M10.3's first bulk-eligible set
  (see M10.3 contract) — bulk starts with the plain-Kubernetes actions
  M8/M8B already proved (scale/restart/delete/label/annotate/cordon/
  uncordon/evict), and only extends to M9 actions in a later, explicitly
  separate step if genuinely needed, per this document's own "small
  slices, real evidence" discipline.
- **No plugin/headless/scripted bulk execution** — belongs to M12,
  exactly as M9's own non-goals already stated for GitOps.
- **No arbitrary shell/CLI keymap actions** — keymaps bind to the
  existing closed `Action` enum only; M10.6 does not add a way to bind a
  key to an arbitrary shell command or external program.
- **No theme marketplace/import-from-URL** — M10.7 ships a small,
  hardcoded set of built-in themes (mirroring the existing `ember`/`mono`/
  `midnight`-shaped constructors already in `ui::Theme`) plus a
  documented, versioned, validated TOML shape for a user theme; no
  network fetch of theme definitions.
- **No workspace/bookmark sync across machines** — local file only,
  exactly like `config.toml` today; no cloud sync, no multi-device merge.
- **No bulk drain, no bulk force-delete.** Both are explicitly excluded
  from M10.3's scope — a deliberate, user-confirmed safety/scope decision
  (see Architectural question 7, RESOLVED), not an omission. Per-target
  safety is not sufficient for either: bulk Drain in particular has
  emergent, set-level cluster effects a generic per-target bulk contract
  does not model (aggregate remaining scheduling capacity across the
  targeted Nodes, aggregate PDB/disruption pressure differing from
  evaluating each Node independently, execution-order sensitivity, the
  safe decision for target N potentially changing after targets 1..N-1
  complete, and materially different cancellation/partial-completion
  consequences than a simple per-target bulk report). Both are recorded
  as a dedicated future backlog item (see "Future backlog: dedicated
  high-risk bulk operations" below), never silently included in M10.3's
  bulk-safe set and never implemented as part of M10.

## Core design principle for bulk mutations: BULK != BYPASS

A bulk mutation is an explicit, bounded collection of individually
identifiable, individually policy-evaluated, individually journaled,
individually verified operations. Every target keeps its own exact
GVK/namespace/name/UID/policy result/confirmation semantics/commit
outcome/verification outcome/journal traceability. Partial success must
remain visible — e.g. "17 selected, 14 committed+verified, 1 policy
denied, 1 target replaced, 1 outcome unknown" — never collapsed to
"SUCCESS." No target may be authorized merely because another target in
the same bulk operation was authorized: each target gets its own
`PolicyEvaluation` from the unmodified `policy::evaluate`, its own
`Confirmation` (or refusal), its own `preflight`/`commit`/`verify` calls.
A "confirm bulk operation" UI gesture is sugar for "confirm N individually
evaluated operations whose evaluations the user has seen," never a single
authorization token that fans out.

## Proposed slice ledger

| Slice | Scope | Status |
| --- | --- | --- |
| M10.0 | M10 acceptance contract / architecture freeze (this document) | ACCEPTED |
| M10.1 | Selection model / multi-select foundation | ACCEPTED |
| M10.2 | Bulk read-only operations / selection UX | ACCEPTED |
| M10.3 | Bulk guarded mutations | ACCEPTED |
| M10.4 | Workspaces | ACCEPTED |
| M10.5 | Bookmarks / saved navigation targets | ACCEPTED |
| M10.6 | Configurable keymaps | ACCEPTED |
| M10.7 | Themes / appearance configuration | ACCEPTED |
| M10.8 | Cross-feature UX integration / persistence / migration | ACCEPTED |
| M10.9 | Combined acceptance / full regression / soak | PLANNED — NOT STARTED |

This mirrors the task's own suggested shape unchanged: reconnaissance did
not find a reason to split or reorder it further. M10.1 must precede
M10.2/M10.3 (selection is their shared foundation); M10.6/M10.7 are
independent of M10.1-M10.5 and could in principle be reordered earlier,
but are kept last-but-one because M10.8 (the config-integration slice)
benefits from all of M10.4/M10.5/M10.6/M10.7's config shapes existing
first, so the migration/versioning slice is designed once against the
real final shape rather than speculatively.

## Per-slice contracts

### M10.1 — Selection model / multi-select foundation

**Purpose.** Extend `State.selected: Option<String>` to a bounded ordered
multi-select collection without breaking single-selection semantics.

**Scope.** A new selection type — e.g. `Selection` wrapping an ordered
`IndexSet<TargetId>` (or `BTreeSet` if insertion order is not needed) —
where `TargetId` carries at minimum `{context, namespace, resource
(GVK-equivalent id, matching `Scope.resource`'s existing `"v1/pods"`-style
string), name, uid}`, i.e. the same identity fields `Scope` already
carries minus session-local `epoch`/`request`. Cursor focus (today's
`table.selected()` ratatui cursor row) stays a **distinct** concept from
the selected set — a user can move the cursor without changing selection,
and toggle-select the row under the cursor explicitly (mirroring how `vim`
visual-block / file-manager multi-select UIs keep cursor and mark-set
separate).

**Architecture/invariants.**
- Selection membership test is by full identity tuple, not UID alone,
  because two different namespaces/contexts could theoretically share a
  UID string only in adversarial/test data — namespace+context+resource
  scoping the same way `Scope`/`Confirmation::authorizes` already do
  removes any ambiguity.
- A relist/reorder/filter change must never silently retarget selection:
  if a selected UID is no longer present in `rows`, it becomes explicitly
  "stale/missing" in the selection (rendered distinctly), never silently
  dropped without indication and never rebound to a same-name new-UID
  replacement — this generalizes the existing single-`selected` clearing
  logic in `State::rebuild()` (`src/app/state.rs` ~line 270-283) to a set,
  but changes its user-facing behavior from silent-clear to
  explicit-stale-marker, since silently shrinking a 20-item bulk selection
  with no visible trace is a worse UX/safety trade for bulk than for
  single-select.
- Kind/namespace/context change (e.g. user runs `:namespace other` or
  `:context other` while a selection is active) has explicit,
  documented semantics — the default is "selection is scoped to the
  view it was made in; changing that view's kind/namespace/context clears
  or marks-stale the whole selection," not a persisted cross-view
  selection (this needs an explicit decision — see Architectural
  questions).
- Bounded selection count (a concrete cap, e.g. matching or below
  `max_objects`, with a much smaller practical UI-sane bound — proposed
  500, refined during implementation) — exceeding it is an explicit
  refusal with a stated count, never silent truncation.
- UI distinguishes cursor focus (row highlight) from selected set (a
  separate marker glyph/column), and both remain visible simultaneously.

**Tests.** Unit: identity-based membership; stale-on-relist marking (not
silent clear, not silent rebind); bounded-count refusal; toggle/select-
visible/clear are pure functions over `Selection` + `rows`; selection
across a kind/namespace/context change matches the documented policy
exactly. App/input tests: keybinding to toggle-select current row,
select-visible, clear, does not regress ordinary single-row navigation.

### M10.2 — Bulk read-only operations / selection UX

**Purpose.** Make the M10.1 selection set actually usable before any
mutation exists: select/deselect current, select all visible, clear,
bounded invert, inspect selected targets (a read-only detail list), and a
bounded summary line ("14 selected, 3 stale, resource: Pod, namespace:
sauron-m7").

**Scope/non-goals.** No global unbounded cluster scan (reuses M10.1's
bound). Every aggregated view states target/completed/partial-unknown
counts and bounded/truncated state explicitly — this generalizes the same
honesty M5/M6 already apply to partial RBAC/bounded traversal results.
Invert is only offered if the result is still boundedly clear (i.e. invert
within the currently visible/filtered set, never "everything in the
cluster minus what's selected").

**Tests.** Unit: summary aggregation counts are exact for a synthetic
`rows`+`Selection`; invert is scoped to visible rows only. App/input:
keybindings for each new action; command-palette entries via the existing
`Action`/`command_names()` registry, not a parallel input path.

### M10.3 — Bulk guarded mutations

**Purpose.** The highest-risk slice: run the existing single-target
mutation gateway (`policy::evaluate` -> `Workflow` -> `preflight` ->
confirm -> `commit` -> `verify` -> journal) once per selected target,
under one bulk-scoped UI flow, with a bulk-shaped preview and a bulk-shaped
result report that never collapses per-target outcomes.

**Scope.** Classify every existing mutation action explicitly:
- **bulk-safe:** label, annotate (idempotent, low blast radius per
  target).
- **bulk-safe-with-stronger-confirmation:** scale, restart, set-image,
  cordon/uncordon, delete, evict, trigger (each already carries its own
  per-action risk in the existing model; bulk does not lower any
  individual target's `MutationRisk`/`ConfirmationRequirement` — it adds
  an *additional* bulk-level acknowledgement, e.g. "you are about to
  attempt this on N targets," on top of, never instead of, each target's
  own existing confirmation strength).
- **excluded from M10.3 entirely (user-confirmed decision, not deferred-
  by-default):** force delete, drain — both are already documented
  composite/highest-risk exceptions (`src/mutation/drain.rs`,
  `PolicyReason::ForceSemantics`); running many of either concurrently
  multiplies worst-case blast radius in a way the generic per-target
  bulk contract does not model. This is a resolved architectural
  decision (Architectural question 7), not an open one — see the non-
  goals entry above and the "Future backlog: dedicated high-risk bulk
  operations" section for what a later, separate contract for these
  would need to cover.
- **deferred:** any M9 Flux/Argo/Helm guarded action (see non-goals).

**Execution model (must be resolved and documented here as slices land,
not left implicit):** sequential vs. bounded concurrency (recommendation:
sequential by default, matching every prior milestone's "no surprising
parallel writes" posture, with bounded concurrency only if a later slice
proves it's needed and safe); cancellation semantics stop **future**
unstarted targets only, never imply or attempt reversal of already-
committed targets (mirrors `Verification`'s own "commit is commit, never
downgraded" rule); ordering is deterministic (selection order); failure
handling continues to the next target by default (no fail-fast that
silently abandons the rest without a report) unless the user explicitly
cancels; a structured `BulkResult` type reuses `MutationOutcome`/
`Verification` per target rather than inventing new success/failure
vocabulary.

**Preview.** A bounded, deterministic table of every target and its
proposed change — explicit truncation past a stated bound (e.g. render
first 50, state "and 12 more not shown, all still included in execution"),
never hiding risk behind "...and N more" with no count or acknowledgement
that those targets are still part of the operation.

**Tests.** See the dedicated Bulk mutation test matrix section below —
individual target records must be inspected in every test, not just the
final aggregate count.

### M10.4 — Workspaces

**Purpose.** Persist/restore a navigation view: cluster/context identity,
namespace/scope, resource kind, filters/search, view mode, optional
bookmarks/UI prefs.

**Scope/invariants.** Built from the existing `HistoryEntry`-shaped
navigation-intent model (`src/app/state.rs:33-52`) — a workspace is
essentially a named, persisted `HistoryEntry` (minus `selected`, which is
UID-scoped to a specific already-loaded incarnation and must not be
treated as durable across a fresh load — reopening a workspace does not
restore selection, only the view; M10.1 selection is session-local for
now unless a later slice explicitly proves durable bulk-selection restore
is safe). Workspaces persist through the M10.8 config model (extending
`Config`/`Settings`, not a new file). **Must not** persist or restore:
`mutation_test_cluster_verified`, any `Confirmation`, any active
`Workflow`, any stale UID treated as authoritative, any credential/secret/
kubeconfig material. Versioned (a schema version field, consistent with
`journal::SCHEMA_VERSION`'s existing precedent). Malformed/old workspace
state fails gracefully — the app starts with defaults and a visible
warning, never a crash, mirroring `Config::load`'s own existing "absent
file is default-safe, malformed file is an explicit error" split.

### M10.5 — Bookmarks / saved navigation targets

**Purpose.** Persisted references to specific objects, as navigation aids.

**Scope/invariants.** Persist GVK + namespace + name + last-observed UID.
Reopening: same UID = exact resource; different UID = explicit
replacement/stale, rendered as such, never silently treated as "the same
object"; missing = explicit missing state. **Never** authorizes mutation
by itself — opening a bookmark is exactly equivalent to navigating there
manually and does not pre-fill or imply any `Confirmation`.

### M10.6 — Configurable keymaps

**Purpose.** User-facing surface over the keymap engine that (per
reconnaissance) **already exists** in `command::Keymap`/`Action`/
`registry()`/`Keymap::compile`.

**Scope.** `:keys`/help-overlay UI showing the effective (resolved,
possibly-overridden) table via `Keymap::help()`/`primary_key()` — both
already implemented. Confirm/extend deterministic conflict detection
(`Keymap::compile`'s existing per-mode conflict check) to surface a
user-actionable diagnostic (which two actions, which mode, which key) on
load failure rather than the current bare `anyhow::Error` string, and to
fail safely (fall back to defaults with a warning, never crash startup)
rather than aborting the process — this is a real gap found in
reconnaissance, not speculative. Mode-awareness and one coherent
resolution layer are already satisfied by `available()`/`Keymap::action()`
— M10.6 does not add a second key-dispatch path.

### M10.7 — Themes / appearance configuration

**Purpose.** Centralize presentation, never semantics.

**Scope.** Generalize `ui::Theme::severity()`'s existing pattern into a
small closed `ThemeRole` enum (`Normal/Muted/Selected/Warning/Critical/
Unknown`, matching `Severity`'s own existing variants plus `Selected`) so
renderer call sites ask for a role, not an arbitrary `Style`. Add a
non-color distinguishing channel (glyph/text prefix, e.g. a one-character
status marker already partially implied by how the table renders health)
so a monochrome/no-color terminal still distinguishes healthy/warning/
error/unknown, selected/focused, mutation risk, and denied/pending/
partial/stale — reconnaissance found today's table styling
(`src/ui/mod.rs:158`) is color-only for severity, which is the concrete
gap this slice closes. Current/default theme (`ember`) stays visually
unchanged. Invalid theme config falls back to a built-in default with a
visible warning, never a crash or a blank/invisible UI.

### M10.8 — Cross-feature UX integration / persistence / migration

**Purpose.** One coherent versioned config model, not four independent
persistence mechanisms bolted onto `src/config.rs` separately.

**Scope.** Extend `Config`/`Settings` (file location: unchanged,
`config::directory()` + `config.toml`, already XDG-aware; format:
unchanged TOML) with the M10.4/M10.5/M10.6/M10.7 fields, each under
`deny_unknown_fields` exactly like `Settings` already is. Define: schema/
versioning (a top-level version field distinguishing "no version present
= today's pre-M10 shape" from "M10-versioned shape," so an M9-era
`config.toml` continues to load with only the new fields defaulted, never
a hard break); defaults (every new field has a safe default matching "no
M10 feature in use" behavior); migration policy (forward-only, best-
effort field-level defaulting — no destructive rewrite of a user's
existing file without their action); invalid-config behavior (unchanged
from today: malformed TOML or a genuinely unknown field is a load error
with source omitted, not a silent partial load); atomic write strategy
(write-to-temp-then-rename in the config directory, since M10 introduces
the first *writes* to this file — today's `Config::load` is read-only;
workspaces/bookmarks/keymap-save/theme-save are new write paths and must
not corrupt the file on a crash mid-write); permission handling (config
directory/file permissions checked/set conservatively, consistent with
credentials-adjacent file handling elsewhere in the app). **Never** write
kube credentials, raw Secret data, mutation confirmation material, decoded
Helm values, or journal content into config — restated here as the single
place all of M10.4/M10.5's "never persist X" rules are enforced by one
shared serialization boundary, not four separate ad hoc checks.

### M10.9 — Combined acceptance / full regression / soak

**Purpose.** Close out M10 exactly like M8B.7/M9.7 closed out their
milestones.

**Scope.** Full M1-M9 regression via existing `accept-m*.py` scripts
unmodified. Combined M10 acceptance run twice clean. Production zero-write
guarantee reconfirmed across every new M10 action. Bulk TOCTOU/partial-
failure/cancellation proven (per the test matrix below). Terminal
restoration proven under a bulk-operation-in-progress interrupt. Config/
workspace migration/invalid-config behavior proven with real malformed
fixtures. Soak with RSS/fd/thread/reconnect/transient-error/state-growth
observations (a bulk-selection-heavy session is a new soak dimension not
exercised by M1-M9's soaks — bounded selection growth/shrink over a long
session), "observed stability only" honesty, never a leak-freedom claim.
Docs reconciled: `HANDBOOK.md`, `docs/RUNBOOK.md`, `README.md`, this file,
`docs/ROADMAP.md` checkpoint line. M9B-A/M9B-B confirmed still PLANNED —
NOT STARTED and untouched. Worktree clean. Local annotated tag
`m10-accepted`, never pushed without explicit authorization.

## Bulk mutation test matrix

A bulk test that only checks the final aggregate count (e.g. "14 of 17
succeeded") is **insufficient** — every test below must inspect individual
target records (per-target `PolicyEvaluation`, `MutationOutcome`,
`Verification`, and journal `Record`s), not just the count:

1. All-succeed — every target's individual `Committed`+`Verified` proven,
   not inferred from an aggregate.
2. One policy-denied among N allowed — the denied target's exact
   `PolicyReason`(s) recorded; the other N-1 unaffected and independently
   evaluated.
3. One target UID-replaced between preview and commit — TOCTOU rejection
   (`PolicyReason::TargetReplaced`/`MutationOutcome::TargetReplaced`) for
   that target only; others proceed normally.
4. One target `NotFound` at commit time.
5. One target `OutcomeUnknown` (simulated transport ambiguity) — never
   silently upgraded to success or downgraded to failure in the bulk
   aggregate.
6. Cancellation midway — targets already committed stay committed and
   verified as normal; targets not yet started are explicitly "not
   attempted," never silently omitted from the report.
7. Journal write failure *before* a given target's commit — that target's
   mutation must fail closed (not committed), matching the existing
   single-target pre-commit journal-failure contract.
8. Journal write failure *after* a target's commit (if the existing model
   permits this state) — must render as `CommittedButJournalIncomplete`
   for that target specifically, never silently dropped from the bulk
   report.
9. No hidden retry anywhere in the sequence — assert retry count is
   exactly zero for every outcome type, including transient-looking
   failures.
10. Selection changed after preview (user deselects/adds a target between
    preview and confirm) — executed set must exactly match the
    last-confirmed set, never the original preview set if it changed.
11. Namespace/context changed after preview — the entire bulk operation
    is invalidated (mirrors `Confirmation::authorizes`' existing scope-
    sensitivity), not silently re-scoped.
12. Mixed resource identities the action semantics reject — **reality
    check found during M10.3 implementation**: a selection literally
    spanning two different *kinds* (e.g. Pod and Deployment
    simultaneously) is structurally impossible in this app's UI model.
    `Selection` lives inside `State`, which shows exactly one resource
    kind at a time, and M10.1's own resolved Architectural question 8
    means any kind/resource/context/namespace change clears the whole
    selection via `cancel_scope()` — so by the time a bulk action reads
    `self.state.selection`, every member is guaranteed to share the
    current view's own single kind. The realistic version of this test
    case, proven instead: the SAME builder produces different per-
    target results because the targets' own live *content* differs, not
    their kind (e.g. `:bulk_set_image` where one selected Deployment has
    containers and another does not) — each ineligible target reports
    `Unsupported` individually; eligible targets still proceed. See
    `bulk_set_image_partial_unsupported_within_one_homogeneous_kind_
    selection` in `src/app/mod.rs`.
13. Empty selection — the bulk action is refused up front with a clear
    message, never silently a no-op success.
14. Maximum bounded selection (the M10.1 cap) — exercised at exactly the
    boundary and one over it (refused before it starts).

## Architectural questions that MUST be resolved before implementation

These are genuine ambiguities found during reconnaissance, not invented
busywork. **None of them block writing this document; several block
starting M10.1/M10.3 implementation** and are flagged as such.

1. **Does bulk confirmation change M7's trust model?** M7/M8's
   `Confirmation` binds to exactly one `(request_id, scope, effect,
   payload_sha256)`. A bulk UI reasonably wants "confirm once for the
   visible batch," but per this document's own BULK != BYPASS principle,
   the actual authorization must still be N individual `Confirmation`s.
   **Open question:** is "one user keypress produces N `Confirmation`s,
   each independently bound to its own target" an acceptable UX, or does
   the product want a distinct `BulkConfirmation` wrapper type (still
   authorizing N independent commits, just a different struct shape for
   UI convenience)? Blocks M10.3. Resolves by: a short design spike
   showing both shapes against the existing `Workflow`/`Confirmation`
   code before committing to one.
2. **Can workspace persistence accidentally restore mutation
   authorization?** Reconnaissance confirms `mutation_test_cluster_
   verified` is process-lifetime CLI-flag-only today (`src/main.rs:33`,
   never touches `src/config.rs`), so there is no existing code path by
   which a workspace/bookmark could restore it — but M10.4/M10.5 are new
   *write* paths into config, the first ones this codebase has had. Not
   currently blocking (the invariant is enforceable by simply never
   adding that field to the persisted shape), but M10.8's serialization
   boundary must have an explicit test asserting no mutation-authority-
   shaped field is ever reachable from `Config`'s `Serialize` impl once
   one is added (`Settings`/`Config` are currently `Deserialize`-only;
   M10.8 is the point at which they also become `Serialize`, which is a
   new surface worth its own dedicated test). Blocks M10.8's completion,
   not its start.
3. **Does bulk execution require retry semantics that would violate no-
   hidden-retry?** No — reconnaissance found no existing precedent for
   automatic retry anywhere in the mutation gateway (M8B.4 explicitly
   *disabled* kube-rs's transport-level auto-retry, per its own commit
   message). M10.3's bulk orchestrator inherits that zero-retry posture
   directly; this is not blocking, just restated as a hard constraint in
   this document (see Invariants) so it isn't accidentally reintroduced
   under "bulk convenience" pressure.
4. **Does keymap architecture require bypassing the command registry?**
   No — `Keymap::compile`/`Action`/`registry()` already provide
   deterministic conflict detection and a single resolution layer; M10.6
   is UI-and-diagnostics work on top of an already-adequate engine, not a
   new architecture. Not blocking.
5. **Would theme work make any existing semantic status color-only?**
   Reconnaissance found this is **already true today** (table row styling
   at `src/ui/mod.rs:158` uses `theme.severity()` color with no
   accompanying non-color marker) — not a risk M10.7 introduces, but a
   pre-existing gap M10.7 must fix as part of its own scope (see M10.7
   contract). Not blocking M10 start, but is a concrete acceptance
   criterion for M10.7/M10.9.
6. **Can the existing config model safely migrate persisted state?**
   Partially open: `Config::load` today is read-only and has no versioning
   field at all (an M9-era `config.toml` and a hypothetical M1-era one are
   structurally identical from the loader's point of view — new fields
   just default via `#[serde(default)]`). M10.8 is the first slice that
   needs real forward migration (old file + new fields). **Open
   question:** does M10.8 need an explicit version field from day one, or
   is `#[serde(default)]` field-level defaulting sufficient forever (i.e.
   "versioning" is really just "every field has a safe default and
   `deny_unknown_fields` catches genuine incompatibility")? Blocks M10.8
   design specifically (not M10.1-M10.7). Resolves by: implementing
   M10.4-M10.7's config field additions first, then checking whether any
   of them actually need a discriminated schema version (e.g. a field
   whose *meaning* changes, not just new fields) before deciding.
7. **Bulk Drain / bulk force-delete safety — RESOLVED (2026-09-21, user
   decision).** Running Drain (a composite cordon+sequential-eviction-
   sweep workflow with its own PDB/emptyDir/unmanaged-Pod safety logic,
   `src/mutation/drain.rs`) against multiple Nodes concurrently, or
   force-deleting many Pods at once, multiplies worst-case blast radius
   in a way M10.3's general per-target bulk-safe classification does not
   adequately model. Per-target safety alone is not sufficient for
   either operation. Bulk Drain specifically has emergent, set-level
   cluster effects: draining several individually-valid Nodes may leave
   insufficient remaining scheduling capacity; aggregate PDB/disruption
   pressure differs from evaluating each Node independently; execution
   order matters; the safe decision for target N may change after
   targets 1..N-1 already completed; and cancellation/partial completion
   has materially different consequences than a simple per-target bulk
   report. **Decision: both are explicitly excluded from M10.3's scope
   entirely** — not included with stronger confirmation, not silently
   folded into the generic classification table. This is recorded as a
   deliberate safety/scope decision, not an omission, and as a dedicated
   future backlog item (see "Future backlog: dedicated high-risk bulk
   operations" below) requiring its own safety contract if ever picked
   up. Nothing in M10 implements any part of bulk Drain/bulk
   force-delete. No longer blocking — M10.3 proceeds with the explicit
   bulk-safe action set: label, annotate, scale, restart, set-image,
   cordon, uncordon, delete, evict, trigger, plus any other action the
   acceptance contract positively classifies as bulk-safe after
   inspection (never assuming every existing single-target mutation
   belongs in bulk merely because its individual implementation already
   exists).
8. **Selection scope across kind/namespace/context changes (M10.1) —
   RESOLVED during M10.1 implementation.** Decision: `App::cancel_scope()`
   is already the single place `state.selected` (single-select) is
   cleared, and is already unconditionally called on every context
   switch (`connect()`), resource-kind switch, and namespace switch
   (both funnel through `watch_resource()` -> `cancel_scope()`). M10.1
   extends that exact call site to also clear `state.selection`
   (`src/app/mod.rs`, `cancel_scope()`) rather than giving `Selection`
   its own copy of context/resource/namespace to compare against — the
   selection is scoped to the view it was made in by construction (it
   lives inside `State`, and the one function that tears down a view's
   scope tears down the selection with it), with zero new state to keep
   in sync. Proven by `context_or_resource_switch_clears_the_whole_
   selection` (`src/app/mod.rs` test). No longer blocking.

None of items 1-8 is a reason to hold back this planning document; items 1,
2 (completion), 6, and 8 are real decisions/spikes that gate specific later
slices as noted. Item 7 was returned to the user for an explicit scope
decision before any bulk-Drain/bulk-force-delete code could be written and
is now RESOLVED (excluded from M10 entirely) — see item 7 above and the
backlog entry immediately below.

## Future backlog: dedicated high-risk bulk operations (NOT part of M10)

Per Architectural question 7's resolution: bulk Drain and bulk force-delete
are explicitly out of M10's scope. If ever picked up as a future milestone
or slice, that work needs its own dedicated safety contract — mirroring how
Drain itself earned its own M8B.5 write-up rather than being folded into a
generic classification table — and must at minimum address:

- Set-level remaining cluster capacity (not just per-target legality) before
  proceeding with each additional Node.
- Execution ordering, and whether it must always be sequential (no bounded
  concurrency) given that draining Node N's safety can depend on the
  post-drain state of Nodes 1..N-1.
- Recalculation of safety before each Node, not just once at preview time —
  the set of pods needing rescheduling changes after every completed drain.
- Aggregate PDB/disruption-budget effects across the whole targeted set, not
  merely each Node's own individual PDB evaluation in isolation.
- Partial completion and cancellation semantics specific to a multi-Node
  drain sequence (what state remains, what is safe to resume vs. must be
  re-previewed from scratch).
- Whether the engine must support "stop on risk change" — i.e. abort
  remaining targets if a later target's safety picture has materially
  worsened because of earlier targets' effects, not just abort on error.
- A blast-radius preview that reflects the *aggregate* effect of the whole
  targeted set, not N independent single-Node previews concatenated.

Nothing in M10 implements any part of this. This section exists so the
design considerations already identified during M10.0's own reconnaissance
are not lost before a future milestone picks this up.

## Bugs / limitations (placeholder)

None yet — implementation has not started. This section is updated per
slice, exactly like M8B/M9's own "Bugs / limitations" sections were.

## Journal

- 2026-09-21: M10.0 (acceptance contract / architecture freeze) written.
  Reconnaissance covered `docs/ROADMAP.md`, `HANDBOOK.md`,
  `docs/RUNBOOK.md`, `README.md`, `docs/M9_ACCEPTANCE.md`/
  `docs/M8B_ACCEPTANCE.md` (structure/tone mirrored), `src/command/mod.rs`,
  `src/app/mod.rs`, `src/app/state.rs`, `src/app/session.rs`,
  `src/app/document.rs`, `src/config.rs`, all of `src/mutation.rs` +
  `src/mutation/{policy,workflow,journal,drain,view}.rs`,
  `src/kube/mutation.rs`, `src/ui/mod.rs`, and the test harness scripts.
  Key findings, all restated above with file/line evidence: (1) selection
  is already UID-based with explicit stale/relist handling
  (`app::state::State.selected`) — M10.1 extends this to a bounded ordered
  set, it does not invent a new identity concept; (2) the full mutation
  gateway (`mutation::*` + `kube::mutation::{preflight,commit,verify}` +
  `mutation::journal`) is exactly reusable per-target for M10.3, with
  Drain's own `DrainStarted/DrainStep/DrainFinished` journal-phase-
  bracket pattern being the direct precedent for bulk's own bracket
  phases; (3) `src/config.rs` is already one coherent, layered
  (base/cluster/context), fail-safe-on-malformed config model that M10.4/
  M10.5/M10.6/M10.7/M10.8 must extend, not fork — and it is currently
  read-only (Deserialize only), so M10 is the first milestone to add
  config *writes*, which is a real new surface, not a trivial extension;
  (4) the keymap engine (`command::Keymap`/`Action`/`registry()`/
  `Keymap::compile`) already implements deterministic per-mode conflict
  detection and effective-binding introspection (`primary_key`, `help`) —
  M10.6 is much smaller than the task's own suggested shape implied,
  mostly a UI/diagnostics slice over an existing engine; (5) `ui::Theme`
  already has a `severity()` role-mapping method but table rendering is
  genuinely color-only today for severity (`src/ui/mod.rs:158`), a real,
  evidence-backed gap M10.7 must close, not a hypothetical risk.
  Slice ledger kept identical to this task's own suggested M10.1-M10.9
  shape — reconnaissance found no architecturally-forced reason to split
  or reorder it, only to note (per-slice, above) how much of M10.6 in
  particular is already-built versus net-new. One genuine architectural
  fork was found (bulk Drain / bulk force-delete safety, Open question 7)
  and is deliberately **not** resolved unilaterally in this document —
  recommendation given, decision left to the user. No other item in Open
  architectural questions was judged significant enough to block writing
  this document; each states what evidence/spike resolves it and which
  slice it gates.
- 2026-09-21: Architectural question 7 (bulk Drain / bulk force-delete
  safety) RESOLVED by explicit user decision: both are excluded from
  M10.3's scope entirely, not included with stronger confirmation, not
  folded into the generic bulk-safe classification table. Rationale
  (user-provided, recorded verbatim in Architectural question 7's own
  entry above): per-target safety is not sufficient for either
  operation — bulk Drain in particular has emergent set-level effects
  (aggregate remaining scheduling capacity, aggregate PDB/disruption
  pressure, execution-order sensitivity, per-target safety changing as
  earlier targets complete, and materially different cancellation/
  partial-completion semantics) that a generic per-target bulk contract
  does not model. M10.3's initial bulk-safe action set is now final:
  label, annotate, scale, restart, set-image, cordon, uncordon, delete,
  evict, trigger. A "Future backlog: dedicated high-risk bulk
  operations" section was added recording the specific design
  considerations (set-level capacity, ordering, per-Node recalculation,
  aggregate PDB effects, partial/cancellation semantics, stop-on-risk-
  change behavior, aggregate blast-radius preview) a future dedicated
  contract for bulk Drain/force-delete would need to cover — nothing in
  M10 implements any part of it. With this resolved, M10.0 is complete
  and no other item blocks starting implementation. **Next: M10.1
  (selection model / multi-select foundation)**, proceeding directly in
  this session per the user's own explicit preference (no further
  background/subagent delegation for the remainder of M10, to keep
  total token cost down even at the cost of more wall-clock time).
- 2026-09-21: M10.1 (selection model / multi-select foundation)
  implemented. New `app::selection` module: `Selection` (bounded, ordered
  `Vec<SelectedTarget>`, cap `MAX_SELECTION = 500`), `SelectedTarget`
  (`uid`/`namespace`/`name`), `SelectionError::BoundExceeded`,
  `toggle`/`add`/`clear`/`contains`/`iter`/`status` (splits into
  present-vs-stale against a fresh `rows` snapshot without ever mutating
  the selection itself). Added `State.selection: Selection`, deliberately
  separate from the existing `State.selected: Option<String>` cursor
  field. Three new `Action`s (`ToggleSelect` "space", `SelectVisible`
  "V", `ClearSelection` "C", all mode `table`, zero key conflicts —
  `Keymap::compile`'s existing conflict-detection pass caught nothing).
  UI: table rows now render a `✓ `/`  ` marker prefix on the first cell,
  visually distinct from the cursor's own `› ` `highlight_symbol` --
  cursor focus and selection membership are both visible simultaneously,
  never collapsed into one glyph (`src/ui/mod.rs`). Architectural
  question 8 (selection scope across context/resource/namespace changes)
  resolved by extending the existing `cancel_scope()` call site (already
  the single place `state.selected` is cleared, already called on every
  context/resource-kind/namespace switch) to also clear `state.selection`
  -- zero new scope-tracking state, the selection is scoped to its view
  simply by living inside `State` and being torn down alongside it.
  Evidence: 7 pure unit tests in `app::selection` (toggle add/remove by
  identity, add is idempotent and never deselects, bound is refused
  explicitly with the mutation left unchanged -- not partially applied,
  status marks a deleted-and-replaced same-name target stale by its
  ORIGINAL uid rather than silently rebinding to the new one, status
  leaves the selection itself unmutated, clear empties regardless of
  present/stale, iteration preserves insertion order); 10 app-level
  tests (toggle marks/unmarks without moving the cursor; toggle without
  a row selected errors rather than silently no-op; select_visible adds
  every visible row and is idempotent on re-run -- does not re-toggle an
  already-deselected-by-hand target off again; select_visible past the
  bound keeps exactly what fit and reports the refusal in `state.status`,
  never silent; clear empties it; a relist that drops a selected row's
  UID leaves it as an explicit stale entry, never silently forgotten;
  a context/resource switch via `cancel_scope()` clears the whole
  selection; ordinary cursor Down/Up navigation is completely unaffected
  by an active selection and vice versa). Full locked suite green (320
  unit, up from 305 pre-M10), `cargo fmt --check` and
  `cargo clippy --all-targets -- -D warnings` clean. No bugs found; no
  deviation from this document's own M10.1 contract. **Next: M10.2**
  (bulk read-only operations / selection UX), continuing directly in
  this session.
- 2026-09-21: M10.2 (bulk read-only operations / selection UX)
  implemented. Two new `Action`s: `InvertSelection` ("i") and
  `InspectSelection` ("s") -- select/deselect current, select-visible,
  and clear were already delivered as part of M10.1's own foundation
  work (the M10.2 contract's test list named them, but the primitives
  landed a slice early since they're inseparable from `Selection`
  itself). `invert_selection()` (`src/app/mod.rs`) reuses
  `Selection::toggle` over `state.rows` only -- never a global scan,
  satisfying the non-goal "invert within the currently visible/filtered
  set, never everything in the cluster minus what's selected" exactly
  by construction (it has no path to anything outside `rows`). New pure
  `selection::report()` renders a bounded (`REPORT_RENDER_BOUND = 100`)
  summary: exact present/stale/total counts up front, one line per
  target with an explicit `[stale]` marker, explicit truncation past the
  bound ("... N more not shown, all still selected") rather than a bare
  "...and more". `open_selection_view()` (`Action::InspectSelection`)
  renders it as a zero-network read-only document, including an explicit
  "(nothing selected)" state rather than erroring on an empty selection
  -- matching every other read-only view's own "never silent" precedent.
  Confirmed (no new code needed): `:select_toggle`/`:select_visible`/
  `:select_clear`/`:select_invert`/`:select_inspect` are already
  palette-suggestible and parse to their `Action`s purely through the
  existing `registry()` fallback in `command::parse` and
  `command_names()`'s own "every registry entry is automatically
  suggestible" design -- proven by the pre-existing, unmodified
  `every_registered_action_name_resolves_via_parse_to_the_same_action`
  test, which iterates the whole registry and already covered these five
  new entries with zero changes required.
  Evidence: 3 new pure unit tests in `app::selection` (`report()`'s
  present/stale counts are exact against a synthetic rows+Selection;
  explicit "(nothing selected)" state; explicit truncation message past
  the render bound, with the header count still reflecting the true
  total); 4 new app-level tests (invert flips only currently-visible
  rows; invert never touches a selected target that is outside the
  current `rows` -- e.g. a stale entry from a prior broader view -- since
  it only iterates `rows`, never the selection's own full membership;
  inspecting the selection opens a document with zero new network tasks;
  inspecting an empty selection renders the explicit empty state rather
  than erroring). Full locked suite green (327 unit, up from 320),
  `cargo fmt --check` and `cargo clippy --all-targets -- -D warnings`
  clean. No bugs found; no deviation from this document's own M10.2
  contract. **Next: M10.3** (bulk guarded mutations -- the highest-risk
  slice, per this document's own Architectural question 7 resolution:
  the bulk-safe action set is label/annotate/scale/restart/set-image/
  cordon/uncordon/delete/evict/trigger, force-delete and drain
  explicitly excluded), continuing directly in this session.
- 2026-09-21: M10.3 (bulk guarded mutations) implemented. **Key finding
  confirmed during implementation**: `kube::mutation::commit()` already
  internally re-evaluates policy, checks cancellation at multiple
  points, TOCTOU-revalidates the live object fresh, and journals every
  phase -- per-target, with zero changes needed. This meant the bulk
  engine did not need to reimplement any of that: it is a genuinely
  thin sequential loop over the exact same single-target primitives
  this codebase already exhaustively tests.
  New `mutation::bulk` module (pure): `BulkItem` (one target's own
  `Result<Workflow, String>` -- `Err` is a build-time `Unsupported`,
  recorded individually, never dropped), `BulkWorkflow` (`requirement()`
  is the STRONGEST confirmation tier among eligible targets, never a
  lowered one; `eligible()`/`excluded()` partition Deny/Unsupported
  targets out of execution explicitly), `BulkOutcome`, `BulkSummary`/
  `summarize()` (exact counts: selected/excluded/attempted/committed/
  verified/cancelled/other -- never a single collapsed bit). New
  `kube::mutation::bulk_commit()` (the only new I/O code this slice
  needed): sequential, never bounded-concurrent; on cancellation, every
  remaining target gets an explicit `MutationOutcome::Cancelled` entry
  so `results.len()` always equals `items.len()` -- never silently
  shrunk. New `mutation::view::bulk_report()` (bounded preview + per-
  target result rendering, truncation past 50 always explicit with a
  real count). `Document.bulk: Option<BulkWorkflow>` (mutually exclusive
  with `workflow`/`drain`, its own `"bulk"` keymap mode reusing the
  exact same `MutationConfirm`/`MutationDryRun` keys as `"mutation"`/
  `"drain"`, mirroring M8B.5's own precedent for this). `bulk_confirm()`
  mirrors `drain_confirm()`'s exact arm/commit contract, dispatched from
  `mutation_confirm()` alongside the Drain check already there. Ten new
  palette-only commands (`:bulk_label`/`:bulk_annotate`/`:bulk_scale`/
  `:bulk_restart`/`:bulk_delete`/`:bulk_evict`/`:bulk_cordon`/
  `:bulk_uncordon`/`:bulk_set_image`/`:bulk_trigger`), each composing
  `open_bulk_workflow()` with the exact same single-target builder
  function `:label`/`:scale`/etc. already uses -- no bulk-specific
  builder logic anywhere, confirming BULK != BYPASS by construction
  rather than by convention alone. Force delete and Drain have
  deliberately no bulk equivalent (Architectural question 7's own
  resolution).
  **Reality-check finding (test matrix item 12)**: a bulk selection
  literally spanning two different resource *kinds* is structurally
  impossible in this app -- `Selection` lives inside `State`, which
  shows one kind at a time, and any kind/namespace/context change
  clears the whole selection via `cancel_scope()` (M10.1's own resolved
  Architectural question 8). The test matrix entry was updated in place
  to record this and substitute the realistic equivalent: the same
  builder producing different per-target results because target
  *content* differs (proven via `:bulk_set_image` against Deployments
  with/without containers), not target kind.
  Evidence: 4 pure unit tests in `mutation::bulk` (eligible/excluded
  partitioning never silently drops Deny/Unsupported; requirement is
  the strongest among eligible, never lowered by a weaker one;
  requirement is None when every eligible item is plain Allow;
  `summarize()`'s every count is exact against a synthetic workflow+
  results); grammar tests for all 10 new commands (exact argument
  parsing, zero-argument commands reject arguments); 7 app-level tests
  (empty selection refused up front; readonly denies every individual
  target with its own `PolicyEvaluation`, zero requests sent; delete's
  Strong confirmation requires a second press with zero requests sent
  on the first; a context/namespace switch after preview invalidates
  the whole operation via the same `cancel_scope()`-driven epoch check
  M10.1 already established; the realistic mixed-eligibility case
  above; preview renders exact selected/eligible/excluded counts;
  dry-run is explicitly not implemented for bulk, matching Drain's own
  precedent); 2 fake-HTTP tests in `tests/watch_transport.rs` against a
  real HTTP endpoint (two targets committed and verified fully
  independently -- one target's outcome never leaks into another's;
  a pre-cancelled bulk operation reports every target explicitly
  `Cancelled` with zero requests ever sent to the server, and
  `results.len()` still equals the full target count). Full locked
  suite green (342 unit, up from 331; 76 fake-HTTP, up from 74),
  `cargo fmt --check` and `cargo clippy --all-targets -- -D warnings`
  clean, full M1-M8B interactive regression (`accept-m8b.py`)
  reconfirmed green on `kind-sauron-test` -- zero drift from touching
  the shared `src/app/mod.rs`/`src/command/mod.rs`/`src/mutation.rs`
  modules. No application bugs found. **Next: M10.4** (Workspaces),
  continuing directly in this session.
- 2026-09-21: M10.4 (Workspaces) implemented. New `app::workspace`
  module (pure data model + the bounded `save()` primitive):
  `Workspace` (`schema_version`, name, context, namespace, resource --
  stored as its qualified id STRING, never a live `Resource`, labels,
  fields, filter_text, sort, descending -- deliberately no field that
  could ever hold `selected`/a UID/`mutation_test_cluster_verified`/any
  `Confirmation`, so "never restores trust" is a structural guarantee,
  not a convention to remember), `WORKSPACE_SCHEMA_VERSION` (mirrors
  `mutation::journal::SCHEMA_VERSION`'s own precedent), `MAX_WORKSPACES
  = 50` (a same-name save/overwrite never counts against the bound --
  only a genuinely new name can exceed it, explicit refusal never
  silent eviction of an older entry).
  Per the M10.4 contract's own explicit dependency note ("Workspaces
  persist through the M10.8 config model... not a new file"), this
  slice does NOT touch disk -- `workspaces: BTreeMap<String, Workspace>`
  lives on `Runtime`, session-local, exactly like `history`/`forward`
  already do; M10.8 is where it gets a real round trip through
  `Config`/`Settings`.
  Four new palette commands: `:workspace_save NAME`, `:workspace_open
  NAME`, `:workspace_delete NAME`, `:workspace_list` (bounded, read-
  only). `save_workspace()`/`open_workspace()`/`finish_workspace()`
  deliberately mirror `current_history_entry()`/`apply_history()`/
  `finish_history()`'s exact shape -- `open_workspace` reuses the same
  "reconnect only if context differs" branch `apply_history` already
  has (via a new `pending_workspace` field cleared at every point
  `pending_history` already is, and resolved in the same
  `Payload::Connected` branch), but `finish_workspace` ALWAYS re-
  resolves the resource fresh from its stored string id via
  `connection.catalog.resolve(...)` -- never conditionally, unlike
  `finish_history`'s `crossed_catalog` branch -- since a workspace is
  meant to outlive the incarnation it was captured in (the catalog may
  have changed even within the same context). Opening a workspace never
  sets `state.selected`: `watch_resource()`'s own `cancel_scope()`
  already unconditionally clears both `selected` and `selection` (the
  exact same hook M10.1 already extended), so there is no separate step
  that could forget to do this.
  Evidence: 4 pure unit tests in `app::workspace` (same-name save
  overwrites without counting against the bound; the bound is refused
  explicitly with the map left unmutated, never silently evicting an
  older entry; overwriting an existing name still succeeds exactly at
  the bound; an overly long name is refused explicitly); 1 grammar test
  (all four commands' exact argument arity); 5 app-level tests (save
  then list renders the saved entry's real fields; opening a same-
  context workspace restores namespace/filter/sort/descending exactly
  while leaving both `selected` and the M10.1 `selection` empty even
  though a row was selected immediately before the open; opening an
  unknown name errors explicitly; delete removes it and a subsequent
  open then fails; saving with no resource selected errors). Full
  locked suite green (352 unit, up from 342; 76 fake-HTTP unchanged --
  this slice touched no network code), `cargo fmt --check` and `cargo
  clippy --all-targets -- -D warnings` clean, full M1-M8B interactive
  regression (`accept-m8b.py`) reconfirmed green on `kind-sauron-test`.
  No application bugs found. **Next: M10.5** (Bookmarks), continuing
  directly in this session.
- 2026-09-21: M10.5 (Bookmarks) implemented. New `app::bookmark` module:
  `Bookmark` (schema_version, name, context, resource -- qualified id
  string, same convention as `workspace::Workspace::resource`,
  namespace, object_name, and the last-observed `uid` as a comparison
  starting point, never treated as still-authoritative by itself),
  `BOOKMARK_SCHEMA_VERSION`, `MAX_BOOKMARKS = 200` (deliberately larger
  than `workspace::MAX_WORKSPACES` -- bookmarking one object is lighter-
  weight and more frequent than saving a whole view), `save()` (same
  same-name-overwrite-never-counts-against-the-bound shape as
  `workspace::save`). The module's own reason to exist: `Status`
  (`Exact`/`Replaced`/`Missing`/`NotCurrentlyViewed`) and the pure
  `status()` function that computes it from a fresh `rows` snapshot --
  a same-namespace/name row with a DIFFERENT uid is `Replaced`, never
  silently rendered as `Exact`; `NotCurrentlyViewed` is its own honest
  state (scope doesn't match the current view, or the list hasn't
  synced) rather than guessing Missing/Exact from stale information.
  Like workspaces, disk persistence is deliberately deferred to M10.8 --
  `bookmarks: BTreeMap<String, Bookmark>` lives on `Runtime`, session-
  local.
  `open_bookmark()`/`finish_bookmark()` mirror `open_workspace()`/
  `finish_workspace()`'s exact reconnect-only-if-context-differs shape
  (a new `pending_bookmark` field cleared at the same four points
  `pending_workspace` already is, resolved in the same
  `Payload::Connected` branch) but restore only `state.selected =
  Some(bookmark.uid)` after `watch_resource()` -- never a claim that the
  object is confirmed present; the EXISTING `rebuild()` UID-validation
  path (the same one M10.1 already relies on for stale-selection
  handling) is what actually confirms or clears it once the fresh list
  arrives. Opening a bookmark is therefore exactly equivalent to
  navigating there and selecting the row by hand -- no new trust is
  granted.
  Four new palette commands: `:bookmark_save NAME` (bookmarks the
  currently selected object), `:bookmark_open NAME`, `:bookmark_delete
  NAME`, `:bookmark_list`/`:bookmarks` (bounded, read-only, with a
  live-computed status column reusing `bookmark::status()` verbatim --
  no second copy of the present/replaced/missing logic anywhere).
  **Bug found and fixed during implementation** (not a regression, a
  first-draft mistake caught by the test suite before commit): the
  status view initially compared against `state.context` (a display
  mirror updated only on `Payload::Connected`) instead of the actual
  connection's own `context` field, causing every bookmark to render
  `NotCurrentlyViewed` even when genuinely still being viewed. Fixed to
  read `connection.context` directly, matching what
  `save_bookmark`/`current_history_entry` already use as "the real
  current context" everywhere else in this file. Caught by
  `bookmark_save_then_list_reports_exact_status_while_still_present`
  failing before any code was committed -- never shipped.
  Evidence: 7 pure unit tests in `app::bookmark` (same-name save
  overwrites without counting against the bound; the bound is refused
  explicitly with the map left unmutated; status is Exact only when the
  UID still matches; status is Replaced -- never Exact -- for a same-
  name different-UID row; status is Missing when absent from synced
  rows; status is NotCurrentlyViewed for a context/resource/namespace
  mismatch or an unsynced list, never guessed at; an all-namespaces
  view still correctly resolves a specific bookmarked namespace's own
  object); 1 grammar test (all four commands' exact arity, `:bookmarks`
  confirmed as a true alias for `:bookmark_list`); 7 app-level tests
  (save without a selected row errors; save-then-list renders `exact`
  while the object is still present; status flips to `REPLACED` the
  moment a same-name row's UID changes, never silently staying `exact`;
  status becomes `MISSING` once the row disappears; opening an unknown
  name errors; delete removes it and a subsequent open then fails;
  opening a same-context bookmark sets `state.selected` to the
  bookmarked UID for the normal `rebuild()` path to validate, proven by
  reading it back after the open completes synchronously). Full locked
  suite green (367 unit, up from 352; 76 fake-HTTP unchanged), `cargo
  fmt --check` and `cargo clippy --all-targets -- -D warnings` clean,
  full M1-M8B interactive regression (`accept-m8b.py`) reconfirmed
  green on `kind-sauron-test`. **Next: M10.6** (Configurable keymaps --
  per reconnaissance, mostly already-built UI/diagnostics work over the
  existing `command::Keymap` engine, not a new architecture), continuing
  directly in this session.
- 2026-09-21: M10.6 (Configurable keymaps) implemented. Confirmed by
  reconnaissance to be mostly already-built: `Action::Help` (bound to
  `?`, already palette-suggestible as `:help`) already renders the
  EFFECTIVE (resolved, possibly-overridden) binding table via
  `Keymap::help()`; mode-awareness and one coherent key-resolution layer
  were already fully satisfied by `available()`/`Keymap::action()` --
  M10.6 adds no second key-dispatch path, exactly as the contract
  required. No new `:keys` command was added since `:help`/`?` already
  is that surface; adding a second name for the same thing would be
  scope creep, not a real gap.
  Two real, reconnaissance-identified gaps were closed:
  1. **Conflict diagnostic was not user-actionable.** `Keymap::compile`'s
     per-mode conflict check previously reported only `"Conflicting key
     binding {key} in {mode}"` -- naming the key and mode but not which
     two actions collided, forcing a user to re-derive that from the
     registry themselves. Now reports `"Key conflict in {mode} mode:
     \"{key}\" is bound to both \"{other}\" and \"{new}\""` -- both
     action names, the exact mode, the exact key, in one line.
  2. **A malformed user keymap crashed the entire application at
     startup.** `State::new` previously propagated `Keymap::compile`'s
     error via `?`, and `Runtime::new`'s own `?` propagated it further
     up to `main()`, which printed the error and called
     `std::process::exit(1)` before the TUI ever rendered a single
     frame -- a config typo in `~/.config/sauron/config.toml`'s `[keys]`
     table made the whole app unusable, not just that one binding.
     `State::new` is now infallible: on a compile failure it falls back
     to `Keymap::compile(&BTreeMap::new())` (proven elsewhere to never
     fail -- the built-in registry has no self-conflicts) and surfaces
     the failure as a visible, dismissible `state.error`, exactly like
     every other "denied, not hidden" surface in this app, never a
     silent swallow and never fatal. Every caller that previously used
     `State::new(...)?`/`.expect("state")` was updated to the new
     infallible signature (`src/app/mod.rs`, `src/ui/mod.rs`'s own
     render tests, `benches/pipeline.rs`).
  Evidence: 2 new unit tests in `app::state` (a malformed keymap falls
  back to defaults -- proven by checking `j` still resolves to `Down`,
  never the user's broken override -- and reports the failure visibly
  in `state.error`; a valid keymap config produces zero startup error);
  1 existing test (`effective_bindings_detect_conflicts`) strengthened
  to assert the new diagnostic actually names both colliding actions
  (`yaml`/`down`) and the mode, not just that compilation failed.
  Full locked suite green (369 unit, up from 367; 76 fake-HTTP
  unchanged), `cargo fmt --check` and `cargo clippy --all-targets -- -D
  warnings` clean (including `--all-targets`, covering the benches
  crate this slice also touched), full M1-M8B interactive regression
  (`accept-m8b.py`) reconfirmed green on `kind-sauron-test` after one
  transient `seq9` forward-liveness flake (curl returned `000` against
  the still-running port-forward; classified environment/timing, not a
  regression -- an immediate retry passed clean end to end, matching
  this document's own established flake-classification precedent from
  M9's own regression runs). **Next: M10.7** (Themes), continuing
  directly in this session.
- 2026-09-21: M10.7 (Themes / appearance configuration) implemented.
  New `ui::ThemeRole` (`Normal`/`Muted`/`Selected`/`Warning`/`Critical`/
  `Unknown`) and `Theme::role()` generalize `Theme::severity()`'s own
  pre-existing private match into the one place any renderer should ask
  for presentational color -- `Theme::severity()` now delegates to
  `role(ThemeRole::from(severity))`, producing byte-identical colors to
  before (`good`/`muted`/`warning`/`critical`), proven by
  `theme_role_from_severity_preserves_the_exact_pre_m10_7_colors`, so
  the ember theme's own look is unchanged exactly as required. `Normal`
  is deliberately what `Severity::Healthy` maps to (a role is a
  presentation concept, not a health concept); `Muted` and `Selected`
  are the two genuinely new roles this milestone's own UI needed
  (de-emphasized text; the cursor/multi-select marker).
  New `severity_glyph()` closes the concrete gap reconnaissance
  identified: table row severity was color-only
  (`Style::default().fg(theme.severity(...))` with no accompanying
  marker) -- reconnaissance also confirmed a "mono" theme already
  exists where every `Color` is `Color::Reset` (zero color information
  whatsoever), making this a real, not hypothetical, gap. Each row's
  marker prefix now carries both the M10.1 selection glyph and a
  distinct severity glyph (`' '`/`'!'`/`'✗'`/`'?'` for healthy/warning/
  critical/unknown) side by side, so a monochrome terminal can still
  tell them apart. Every OTHER place this document's own M10.7 contract
  named (mutation risk, denied/pending/partial/stale) was checked
  against `workflow_report`/`drain_report`/`bulk_report`/`bookmark`
  status rendering and confirmed already text-word-based, not color-
  only (`"DENIED"`, `"[stale]"`, `"REPLACED"`, `"[eligible]"`/
  `"[unsupported]"`, etc.) -- no further gap found there, so no changes
  were needed to those renderers.
  **Second real gap closed, found by re-reading this section's own
  "Invalid theme config falls back to a built-in default with a
  visible warning, never a crash" requirement against the actual code**
  (not hypothetical -- `Config::resolve()` had exactly this bug): an
  invalid `theme` string previously `bail!`ed inside `Config::resolve`,
  and (via the same `?`-propagation chain M10.6 already found and fixed
  for keymaps) crashed the entire application at startup on a config
  typo, before a single frame ever rendered. Fixed the same way as
  M10.6's keymap fix: the `bail!` was removed (with a comment
  explaining why this specific check, unlike the neighboring
  `max_objects`/`max_bytes`/`request_timeout_secs` resource-safety
  bounds, is safe to relax -- those remain hard failures, confirmed
  unchanged by `resource_bounds_still_hard_fail_unlike_the_purely_
  cosmetic_theme`), and `State::new` now checks the resolved theme name
  itself and folds a visible `"Unknown theme ..., using default
  (ember)"` warning into `state.error` alongside any keymap warning
  (both can coexist in one message, proven by `both_a_malformed_
  keymap_and_theme_are_reported_together`) -- never silently rewriting
  the user's own config string, only warning; `ui::Theme::named()`'s
  own pre-existing `_ => ember-look` fallback arm was already graceful
  and needed no change.
  Evidence: 3 new unit tests in `ui` (every `Severity` variant's glyph
  is distinct; `Theme::role`'s colors for the ember theme exactly match
  the pre-M10.7 `severity()` mapping; the "mono" theme's roles all
  collapse to `Color::Reset` while `severity_glyph` still distinguishes
  healthy from critical, proving the non-color channel actually carries
  information when color cannot); 2 new unit tests in `app::state` (an
  unrecognized theme falls back with a visible warning, the settings
  string itself left unrewritten; keymap and theme warnings combine
  into one message when both are broken at once) plus 1 existing test's
  fallback path was already covered; 2 new unit tests in `config` (an
  unrecognized theme name no longer fails `resolve()`, proving startup
  survives it; genuine resource-safety bounds still hard-fail, proving
  the relaxation was scoped to theme only, not a blanket removal of
  validation). Full locked suite green (376 unit, up from 369; 76
  fake-HTTP unchanged), `cargo fmt --check` and `cargo clippy
  --all-targets -- -D warnings` clean, full M1-M8B interactive
  regression (`accept-m8b.py`) reconfirmed green on `kind-sauron-test`.
  No deviation from this document's own M10.7 contract. **Next: M10.8**
  (Cross-feature UX integration / persistence / migration -- where
  M10.4's workspaces, M10.5's bookmarks, and this slice's own theme
  config finally get a real round trip through `Config`/`Settings`),
  continuing directly in this session.
- 2026-09-21: M10.8 (Cross-feature UX integration / persistence /
  migration) implemented. **Scoping finding, recorded rather than
  silently assumed**: re-reading M10.6/M10.7's own actual scope
  confirmed neither introduced a new in-app editor for keymaps or
  themes -- both remain configured by editing `config.toml`'s existing
  `keys`/`theme` fields directly (M10.6 was UI/diagnostics work over an
  already-functional engine; M10.7 was presentation + fail-safe
  validation). So "keymap-save"/"theme-save" named in this section's own
  original prose have no new write path to build -- those two fields
  were already part of `Settings` and already round-trip through
  `Config::load`/`resolve` since before M10 started. The real, concrete
  new persistence work is exactly M10.4's workspaces and M10.5's
  bookmarks, which is what this slice actually built.
  `Workspace`/`Bookmark` (`src/app/workspace.rs`/`src/app/bookmark.rs`)
  gained `Serialize`/`Deserialize` (`deny_unknown_fields`, matching
  `Settings`'s own convention) directly -- no separate "persisted"
  mirror struct to keep in sync. `Config` gained three new top-level
  fields: `version: u32` (`CONFIG_VERSION = 1`; 0 means "no version
  present," covering both a genuine pre-M10 file and a fresh
  `Default`; every field this milestone added already has its own
  `#[serde(default)]`, so nothing actually branches on this number
  today -- it exists for a FUTURE change that alters a field's
  *meaning*, mirroring `mutation::journal::SCHEMA_VERSION`'s own "exists
  for the next migration, not this one" precedent), `workspaces:
  BTreeMap<String, Workspace>`, `bookmarks: BTreeMap<String, Bookmark>`
  -- all three manually spliced out of the raw TOML table in
  `Config::load` before the remainder is parsed as `Settings` (mirroring
  the exact pattern `clusters`/`contexts` already used), so `Settings`'s
  own `deny_unknown_fields` never sees them and a pre-M10 file with
  neither key present loads with both maps empty, no hard break.
  New `Config::save(path)`: the first *write* path this file has ever
  had. Atomic (serialize to a `.config.toml.tmp-<pid>-<nanos>` file in
  the same directory, `0600` permissions on Unix, then `rename()` over
  the real path -- a crash mid-write leaves either the old file intact
  or the new one complete, never a half-written `config.toml`); always
  stamps `version = CONFIG_VERSION` regardless of what was loaded;
  `Config::default_path()` extracted so `save()` and `load()` share the
  exact same default-location logic rather than duplicating it.
  `Runtime::new` now loads `workspaces`/`bookmarks` from the already-
  loaded `Config` instead of always starting empty; every
  `save_workspace`/`delete_workspace`/`save_bookmark`/`delete_bookmark`
  now calls a new `persist_config()` that syncs both maps into
  `self.config` and writes to disk -- a write failure is surfaced
  visibly via `state.error` ("saved for this session, but could not
  write to disk: ...") but never rolls back the in-memory change,
  matching this app's own "denied/failed is visible, never silently
  reverted state the user just set" convention throughout M7-M10.
  **Found and fixed before any test ran** (not a shipped regression): the
  existing `runtime()` test helper passed `config_path: None`, which
  would have made every workspace/bookmark-saving test in this entire
  file write to the REAL `Config::default_path()` on whatever machine
  ran `cargo test` -- caught immediately by re-reading `persist_config`
  against the test harness before running anything, fixed by giving
  `runtime()` a unique scratch path per call
  (`std::env::temp_dir().join("sauron-test-config-<pid>-<counter>.toml")`),
  confirmed by inspecting `~/.config/sauron/` after a full test run and
  finding only the pre-existing, unrelated `mutations.jsonl`.
  Evidence: 5 new unit tests in `config` (save-then-load round-trips
  workspaces/bookmarks byte-for-byte; a pre-M10 file with neither new
  field loads with both maps empty and `version == 0`; `save()` called
  repeatedly leaves no leftover `.tmp-` files in the directory; a
  genuinely unknown top-level field is still a hard load error, proving
  `deny_unknown_fields` was not weakened anywhere in this change); 4 new
  app-level tests (a workspace save actually reaches disk, readable back
  via a fresh `Config::load`; a workspace delete also reaches disk; a
  bookmark save actually reaches disk; a workspace saved by one
  `Runtime` is loaded fresh by a second `Runtime` constructed against
  the same path -- the actual cross-session contract this slice exists
  for, not just an in-process round trip). Full locked suite green (383
  unit, up from 376; 76 fake-HTTP unchanged), `cargo fmt --check` and
  `cargo clippy --all-targets -- -D warnings` clean, full M1-M8B
  interactive regression (`accept-m8b.py`) reconfirmed green on
  `kind-sauron-test`. **Next: M10.9** (Combined acceptance / full
  regression / soak -- the final M10 slice), continuing directly in this
  session.

## Final acceptance checklist

- [ ] Every M10.1-M10.9 slice implemented and individually ACCEPTED in
      this document's own Journal (or explicitly, evidence-backed
      DEFERRED, mirroring M9.6's precedent — never silently dropped).
- [ ] Every bulk mutation action goes through the unmodified M7/M8/M8B
      gateway (`policy::evaluate` -> `Workflow` -> `preflight`/`commit`/
      `verify`) — no bulk-only bypass path, no CLI shell-out, no second
      policy engine.
- [ ] BULK != BYPASS proven: every bulk test in the matrix above passes
      with individual target records inspected, not just aggregate counts.
- [ ] No target ever authorized merely because another target in the same
      bulk operation was authorized (verified by test, not just by code
      review).
- [ ] No hidden automatic retry anywhere in the bulk path.
- [ ] Selection model (M10.1) never silently retargets on relist/reorder/
      filter/kind/namespace/context change; stale/missing selections are
      explicit.
- [ ] Config model (M10.8) is the single persisted config surface — no
      second file/format for workspaces, bookmarks, keymaps, or themes.
- [ ] No mutation-authorization-shaped state (verified-cluster flag,
      confirmations, in-flight workflows, credentials) is ever persisted
      by workspaces or bookmarks — proven by an explicit serialization
      test, not just informal review.
- [ ] Keymap help overlay reflects effective (resolved/overridden)
      bindings, never hardcoded defaults; malformed keymap config fails
      safely with an actionable diagnostic, never crashes startup.
- [ ] Themes never make health/safety meaning color-only; monochrome/
      no-color terminal still distinguishes every required state via
      text/symbol/style. Default theme visually unchanged.
- [ ] Invalid config/workspace/keymap/theme reload retains prior working
      policy/keymap state rather than crashing or silently corrupting it
      (the exact ROADMAP M10 deliverable line: "Invalid reload retains
      policy/keymap").
- [ ] Per-context config scope (cluster/context layering) works for every
      new M10 setting category, matching the ROADMAP M10 deliverable line
      "per-context scope."
- [ ] Bookmark identity survives/detects UID replacement correctly,
      matching the ROADMAP M10 deliverable line "mark identity."
- [ ] Production sees zero writes and zero mutating dry-runs across every
      M10 action, for the entire milestone.
- [ ] 32x9 works. Terminal restoration works, including under a bulk
      operation interrupted mid-run.
- [ ] Full M1-M9 regression (`accept-m*.py`, unmodified) passes.
- [ ] Combined M10 acceptance run twice clean.
- [ ] Soak (M10.9) is acceptably stable under the "observed stability
      only" honesty standard — no leak-freedom claims.
- [ ] Docs reconciled: this file, `HANDBOOK.md`, `docs/RUNBOOK.md`,
      `README.md`, `docs/ROADMAP.md` checkpoint line.
- [ ] `docs/M9B_A_ACCEPTANCE.md`/`docs/M9B_B_ACCEPTANCE.md` confirmed
      still PLANNED — NOT STARTED and untouched by any M10 change.
- [ ] Worktree clean.
- [ ] Local annotated tag `m10-accepted` created — never pushed without
      explicit authorization.
