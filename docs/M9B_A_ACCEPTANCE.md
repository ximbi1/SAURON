# M9B-A — Native Helm Engine, Operational Parity: planning ledger

Status: **PLANNED — NOT STARTED.** This document is a future,
ready-to-execute milestone contract, written so implementation can begin
months later without reconstructing the architecture from chat history.
It converts `docs/NATIVE_HELM_ENGINE_RESEARCH.md`'s rollback/uninstall
findings (its original pass, §1-21) into a slice-by-slice acceptance
ledger, following the exact document conventions `docs/M8B_ACCEPTANCE.md`
and `docs/M9_ACCEPTANCE.md` established: scope definition and contract
design first, updated slice by slice only once implementation actually
starts. **Nothing in this file authorizes touching code, cluster, or CI.**
M10 may begin, and is expected to begin, before this milestone starts.
The existence of this document is not authorization to start MH0 or any
other slice below — that requires separate, explicit approval, exactly as
the research document's own closing line states.

## Purpose

M9.5 (ACCEPTED) built read-only Helm inspection: decode a release Secret,
sanitize it, never write. M9.6 (DEFERRED, 2026-09-20) found no safe path
to rollback/uninstall within M9's own timeline and explicitly asked a
future milestone to walk the evidence-backed path its own write-up
sketched. `docs/NATIVE_HELM_ENGINE_RESEARCH.md` is that future
investigation: a source-verified (not assumed) map of Helm's own
rollback/uninstall call graphs, storage semantics, apply-protocol
divergence (CSA vs. SSA), hook sub-lifecycle, and failure semantics, with
a milestone plan (MH0-MH9) and a full differential-testing strategy.

**M9B-A's purpose**: build the native Rust Helm lifecycle/runtime needed
to safely *operate* existing Helm releases — rollback, uninstall, and the
storage/history/lifecycle/manifest/apply primitives those two actions
require — without shelling out to `helm`, and prove it via differential
testing against the real `helm` CLI as test oracle. This is explicitly
**not** "just make rollback work": it defines an engine boundary
(storage, manifest, apply, lifecycle, actions — per the research
document's own §15/§16 architecture) sufficient for honest operational
parity on releases the engine can positively detect as compatible with
its current capability, with everything outside that envelope refused
explicitly, never silently degraded.

M9B-A does **not** cross into chart execution (values handling, chart
loading/rendering, install, upgrade, CRD-install-first, OCI). That is
M9B-B's job, and M9B-A must not depend on it (see "Relationship between
M9B-A and M9B-B" below) — existing Helm releases already contain their
own rendered manifests and release state, so rollback/uninstall never
need a chart, a template engine, or a repo/OCI client
(`docs/NATIVE_HELM_ENGINE_RESEARCH.md` §17, reaffirmed by §22's own
"a native rollback/uninstall engine never needs a Helm template engine at
all").

## Relationship to prior milestones

M9B-A reuses, unmodified in spirit, everything M7/M8/M8B/M9 already built,
restated per this project's own established "Relationship to M7/M8"
convention:

- `mutation::{MutationIntent, MutationTarget, MutationEffect, MutationRisk,
  PolicyReason, PolicyDecision, PolicyEvaluation, ConfirmationRequirement,
  Confirmation, MutationOutcome, Verification}` — the pure model. M9B-A
  adds new `source_action` values (`helm_rollback`, `helm_uninstall`) and,
  where genuinely needed, new `PolicyReason`/`MutationOutcome`/
  `Verification` variants for Helm-specific outcomes this model does not
  already express (e.g. a pending-release refusal, a hook-presence
  refusal) — it does not fork the model.
- `mutation::policy::evaluate` — the single policy engine. Helm rollback/
  uninstall are new *inputs* to this engine (new source_actions, new risk
  classifications), never a parallel decision path.
- `mutation::workflow::{Built, Workflow}` — the same shared shell. Every
  M9B-A action composes an internal engine call into a `Built` exactly
  like `scale`/`restart`/`helm`-adjacent M9 actions do, following M8B.5's
  own documented-exception precedent if an action is genuinely composite
  (e.g. rollback's own pre-flight compatibility-detection step, hook
  refusal check, and apply step may need the same kind of ordered,
  independently-reportable-step treatment Drain established).
- `kube::mutation::{preflight, commit, verify}` — the single execution
  gateway and single verification entry point. M9B-A must not add a
  second gateway, a second TOCTOU mechanism, or a second verification
  dispatcher (`docs/NATIVE_HELM_ENGINE_RESEARCH.md` §12/§15 restates this
  as a hard constraint, not a suggestion). The engine's `apply`/`lifecycle`
  layers produce an *effect description*; `kube::mutation::commit`/
  `verify` remain the only place a write actually happens and gets
  verified.
- `mutation::journal::{Journal, Phase, Record}` — the same append-only,
  redacted, bounded journal. No second journal file or format. Per the
  research document's own §11.3 finding: a Helm rollback/uninstall's
  journal entry records *intent* (source release name/namespace/
  from-revision/to-revision) and *outcome*, never the decoded chart
  values/manifest/notes — this is a new, explicit design decision for
  M9B-A.1/A.2 to make concrete, not something the existing generic
  payload-hash redaction automatically covers for Helm-shaped structured
  data.
- `Document.workflow`-style dedicated fields, the `"mutation"` keymap
  mode, and `mutation::view::workflow_report` — the same shared UI shell,
  extended only where a Helm action has more than one meaningful step or
  a genuinely different verification shape, mirroring Drain's own
  precedent.
- `Command::{...}` + `parse()` grammar + `command_names()` — the same
  single command registry. `:helm_rollback`/`:helm_uninstall` (exact
  grammar TBD at M9B-A.6) are discoverable the same way every other
  guarded command is; no second palette, no hidden command.
- `src/integrations/helm.rs` / `src/kube/helm.rs` (M9.5) — reused
  verbatim for the read side: `decode_release`/`sanitize`'s existing
  bounded-gunzip/base64 discipline, `HelmReleaseView`, `RELEASE_SECRET_TYPE`,
  the `pub(crate)`-only visibility discipline on the raw decode path, and
  `kube::helm::read_release`'s own UID/type re-verification pattern. A
  write-capable engine widens this boundary by exactly one narrow write
  primitive (`kube::helm::write_release`, M9B-A.2), never by relaxing
  `Object::new`'s unconditional Secret redaction or adding a second, more
  permissive Secret path (research §11, recommendation).
- The `ConnectOptions.mutation_test_cluster_verified` /
  `--mutation-test-cluster-verified` distinction (never `!readonly`, never
  a context-name heuristic) — unchanged, reused as-is for all M9B-A live
  acceptance.
- `scripts/test-cluster.sh` / `scripts/test-cluster-m9.sh`'s guarded
  Docker/API identity check convention — M9B-A adds its own
  `m9b-a-*`-prefixed cases (see Live-cluster strategy) following the
  exact same non-heuristic shape, never weakening or sharing code with
  the existing scripts (each remains a sibling script, per M9's own
  stated rationale for why `test-cluster-m9.sh` did not touch
  `test-cluster.sh`).

### Design test (carried over from M8/M8B/M9's own ledgers, unchanged)

At the end of M9B-A, adding a further guarded Helm rollback/uninstall
refinement should still look like: 1) parse the semantic user action, 2)
validate its specific arguments against the engine's own compatibility
detection, 3) build a `MutationIntent` (or an explicit, documented
composite sequence, mirroring Drain's own exception), 4) provide a
semantic preview renderer, 5) hand it to the existing M7 infrastructure.
It should NOT require a new confirmation subsystem, a new policy
subsystem, a new journal, a new transport safety model, a new UID model,
or new TOCTOU logic, and it should NOT require shelling out to `helm`. If
any M9B-A action needs one of those, stop and reconsider before
implementing — the design is wrong.

## Invariants (carried forward, restated explicitly for this milestone)

Every invariant below is load-bearing and applies to every M9B-A slice
without exception, unless a slice explicitly proves and documents a
narrower, deliberate divergence (see "Deliberate divergences from Helm
upstream" below — divergence is only ever additive safety, never relaxed
safety):

- **UNKNOWN ≠ ZERO ≠ HEALTHY.** A release the engine cannot positively
  classify as compatible is `Unknown`/refused, never treated as "safe by
  default" or "definitely broken."
- **UID ≠ NAME.** Every release/revision identity check is by UID of the
  backing Secret, never by release name or revision number alone
  (research §12.1-12.2).
- **COMMIT ≠ OBSERVED EFFECT.** `MutationOutcome` (was the request
  accepted) and `Verification` (does a fresh read match) stay distinct
  facts, never collapsed, exactly as every M7/M8/M9 action already
  requires.
- **LOOKS EQUIVALENT ≠ BEHAVES EQUIVALENT.** The single organizing
  principle of this entire milestone (research §13, §21, §39). Producing
  the same apparent end state is never the acceptance bar; remaining
  compatible with Helm's own expectations (so the real `helm` CLI can
  keep operating on a release SAUR-ON touched) is. Every differential
  test in this document exists specifically to falsify "looks
  equivalent," not to confirm it.
- **No runtime shell-out to `helm`/`kubectl`, ever.** Restated from
  M9.5/M9.6/every prior milestone's own design rule. The real `helm`
  CLI's role in this milestone is exclusively as an offline **TEST
  ORACLE** — invoked only from the test harness (`scripts/test-cluster-*`
  live-test scripts), never from any code path SAUR-ON ships or runs at
  runtime.
- **No direct release-Secret mutation as a shortcut around lifecycle
  semantics.** M9.6's own explicit rejection of "hand-edit the storage
  record" as an implementation shortcut is restated as binding here —
  every write goes through the engine's own storage/lifecycle layers
  (revision allocation, status-machine transitions, supersede bookkeeping),
  never a raw PATCH to a release Secret's fields.
- **No second policy/confirmation/journal/verification system.** Restated
  from the Design test above; the engine composes, it never forks.
- **No hidden automatic mutation retry.** Real Helm's own
  `retry.RetryOnConflict` wrapping every API call (research §12.5) is
  explicitly **not** ported — a 409/Conflict is surfaced as an explicit
  `MutationOutcome`/`Verification` fact and only a human-initiated
  re-invocation may retry, exactly like every other M7/M8/M9 action.
- **Production remains read-only.** Every M9B-A write action is strictly
  forbidden against production, exactly like every M8/M8B/M9 write
  action. All writes only against explicitly verified isolated test
  clusters via the existing `mutation_test_cluster_verified` flag — never
  inferred from context name.
- **Real Helm CLI is allowed as TEST ORACLE only** — never a runtime
  dependency, never invoked by the shipped binary, never a fallback path.
- **Raw Helm Secret contents never enter Object/Store/Timeline/graph/
  journal.** Restated and *extended* from M9.5's read-only contract: this
  now also covers the write path's own in-memory unredacted release
  record (research §11.2) — it must be as narrowly scoped as
  `decode_release`'s existing return value, never crossing into any
  generic pipeline.
- **No logs/panic/errors leaking values/manifest/notes/raw release
  payload.** Every new error type introduced by this milestone inherits
  M9.5's own `error_display_never_embeds_raw_payload_content` test
  discipline — a structural guarantee (no variant carries payload bytes),
  restated at runtime by an equivalent test in every new module.
- **Exact current revision identity and target revision identity
  revalidated at commit.** Both the release's current-revision Secret UID
  *and* the specific historical revision Secret UID being rolled back to
  must be re-verified fresh at commit time, not merely at preview time
  (research §12.1-12.2) — a concurrent real `helm` operation or a
  `MaxHistory` prune between preview and commit must be caught, not
  assumed away.
- **`pending-*` release state must fail closed unless the slice
  explicitly defines and proves a safe behavior.** Per research §12.3:
  SAUR-ON deliberately does **not** match Helm's own laxity here (Helm
  itself will race two concurrent `helm rollback` invocations against a
  pending release, per its own documented lack of locking) — any observed
  `pending-install`/`pending-upgrade`/`pending-rollback` status on the
  target release is a hard precondition failure, reported explicitly,
  never silently proceeded past or silently retried.
- **Unsupported Helm semantics must reject explicitly, never silently
  degrade.** Hooks present for the relevant event, SSA-managed live
  resources the engine cannot yet safely interoperate with, non-Secret
  storage drivers, CRD/cluster-scoped resources ahead of the slice that
  proves it — each is an explicit, named, user-visible refusal with a
  stated reason, never a silent partial attempt (this is the exact
  "simple releases only" shortcut M9.6 already rejected once; M9B-A must
  not reintroduce it under a different name).

### Deliberate divergences from Helm upstream (documented, not accidental)

Per the research document's own instruction, every place SAUR-ON's safety
model is intentionally *stricter* than real Helm is recorded here, so a
future differential-test failure against real `helm`'s own looser
behavior is correctly read as "expected, documented divergence," not as a
compatibility bug:

1. **No hidden retry on 409/Conflict** (research §12.5). Real Helm wraps
   every mutating API call in `retry.RetryOnConflict`. SAUR-ON surfaces
   the conflict explicitly instead. This is the single most consequential
   divergence in this document, and it must be called out in every
   fixture whose real-Helm behavior includes a silent retry the engine
   will not reproduce.
2. **Stronger TOCTOU / UID checks than Helm's own storage layer**
   (research §7/§12.1-12.2). Helm's own `Deployed()` comment admits
   concurrent invocations corrupt its database; SAUR-ON's UID-keyed
   revalidation at both preview and commit time is strictly more careful
   than upstream requires of itself.
3. **Refusal on an already-`pending-*` release state** (research §12.3).
   Real Helm's own `Run` does not defensively check this; SAUR-ON does,
   and reports the refusal rather than racing.

No other divergence is authorized by this document. Any future slice
proposing an additional divergence must document it here, in this same
format (what upstream does, what SAUR-ON does instead, why), before
implementing it — never silently.

## Safety boundary

Production remains **READ-ONLY**, unchanged from every prior milestone.
Every M9B-A write action — rollback, uninstall, and any intermediate
engine-internal write (release storage create/update) — is strictly
forbidden against production. All M9B-A live writes use only an
isolated, explicitly verified test cluster via the existing
`mutation_test_cluster_verified` flag and its guard-script convention
(see Live-cluster strategy). Never infer authorization from context name.
Never push/publish anything without explicit authorization. The real
`helm` CLI, when used as test-harness tooling to construct starting
fixtures or as a differential oracle, is pure test-harness tooling —
exactly like M9.5's own `demo-release` fixture was created via the real
`helm` CLI as "pure test-harness tooling" per that document's established
convention — never a runtime dependency of the shipped binary.

## Explicit non-goals for M9B-A

- **Values handling, chart loading, chart rendering, `template`,
  `install`, `upgrade`, CRD-install-first behavior, OCI pull/push,
  classic repo pull, provenance/PGP verification, `helm test` as a
  distinct action, `helm diff`'s upgrade/install-case, `helm lint`, `helm
  repo` management, dependency management, the plugin system.** All of
  these belong to M9B-B (where in scope at all — several are permanently
  out of SAUR-ON's mission per the research document's Tier 3
  classification) or are explicitly excluded from both milestones. M9B-A
  must not depend on any of them (see "Relationship between M9B-A and
  M9B-B").
- **A generic "Helm client" surface.** The engine exposes exactly
  `rollback`/`uninstall` actions returning `mutation::workflow`-shaped
  `Built`s — never a general-purpose read/write Helm library surface any
  other caller could reach for (research §16, §21).
- **Hook execution, in the first supported envelope.** Per research §9's
  own explicit recommendation ("(B) — a later slice, not (A)
  mandatory-from-day-one, and emphatically not (C) rejected forever"),
  M9B-A.0-A.6 explicitly refuse to operate on any release whose target
  manifest contains hooks for the relevant event; the hook engine itself
  is M9B-A.7, landing before any hook-bearing release can be operated on,
  never skipped forever.
- **SSA support, in the first supported envelope.** M9B-A.0-A.6 operate
  only on CSA (three-way-merge, v3-style)-managed releases, with an
  explicit hard refusal for releases showing SSA-managed live resources;
  SSA support is M9B-A.8, not a permanent exclusion.
- **Non-Secret storage drivers** (ConfigMap, SQL, Memory). Secret is
  Helm's own documented default and what M9.5's existing exception
  already targets (research §17); no evidence found that any target
  cluster needs another driver.
- **Publishing `crates/helm-engine` as a standalone crate.** Per research
  §16/§21, this stays an internal workspace crate until the full
  differential fixture matrix passes against both CSA and SSA oracles —
  a decision this document does not revisit.
- **Bulk operations** (multi-release rollback/uninstall). Belongs to M10
  or a later bulk-operations extension, never folded into M9B-A — every
  M9B-A action targets exactly one release per intent.
- **Plugin/headless/scripted mutation execution.** Belongs to M12,
  unchanged from every prior milestone's own non-goal list.
- **`kubectl`/`helm` CLI passthrough of any kind at runtime.** Restated
  as an explicit non-goal, not merely an invariant, so it appears in both
  places a future reader might look.

## Relationship between M9B-A and M9B-B

**M9B-B depends on accepted primitives from M9B-A. M9B-A must NOT depend
on M9B-B.** This dependency direction is load-bearing and is the single
most important structural decision in both documents.

Concretely: rollback/uninstall must not require chart rendering, chart
fetching, Go template, Sprig, repo handling, or OCI — because existing
Helm releases already contain the rendered manifests and release state
needed for operational lifecycle work (research §17's own source-verified
finding, restated at §22: "a native rollback/uninstall engine never needs
a Helm template engine at all"). Every M9B-A slice below is checked
against this constraint explicitly as part of its own acceptance
criteria — if any M9B-A slice is found, during implementation, to need
something from M9B-B's scope, that is treated as a design defect to be
fixed (by finding the missing primitive inside M9B-A's own boundary, per
the research document's own architecture), never as grounds for silently
importing an M9B-B dependency.

The reverse dependency (M9B-B needing M9B-A) is expected and correct:
M9B-B's install/upgrade actions reuse M9B-A's storage/lifecycle/apply
layers verbatim (research §23's own "precise reuse breakdown" table),
and M9B-B's own ledger states this explicitly.

## Proposed slice ledger

Refined from the brief's suggested M9B-A.0-A.11 against the research
document's own MH0-MH9 dependency graph (§18) and its explicit hook/SSA
deferral recommendation (§9, §17). Ordering rationale follows each slice's
own contract below; the overall shape mirrors MH0-MH9 closely because the
research document's own milestone plan is already the product of a
dedicated feasibility investigation, not a fresh guess.

| Slice | Scope | Verdict |
| --- | --- | --- |
| M9B-A.0 | Acceptance contract + architecture freeze (this document, formalized; corresponds to research MH0) | PLANNED — NOT STARTED |
| M9B-A.1 | Internal workspace crate skeleton (`crates/helm-engine`) | PLANNED — NOT STARTED |
| M9B-A.2 | Release model + storage round-trip compatibility (read reuse + new write primitive; corresponds to MH1) | PLANNED — NOT STARTED |
| M9B-A.3 | Lifecycle/history/status semantics (state machine; corresponds to MH2) | PLANNED — NOT STARTED |
| M9B-A.4 | Manifest parsing/sort/resource-policy/delete semantics | PLANNED — NOT STARTED |
| M9B-A.5 | CSA reconciliation path (three-way-merge-equivalent apply/delete; corresponds to MH3) | PLANNED — NOT STARTED |
| M9B-A.6 | First guarded native rollback/uninstall for supported CSA releases, no hooks (corresponds to MH5/MH6, hook-refusal guard in place) | PLANNED — NOT STARTED |
| M9B-A.7 | Hook engine (corresponds to MH4) | PLANNED — NOT STARTED |
| M9B-A.8 | SSA reconciliation + CSA→SSA compatibility behavior (corresponds to MH7) | PLANNED — NOT STARTED |
| M9B-A.9 | Diff/test/read-completion surfaces where appropriate (rollback/uninstall-case diff-preview per research §33/§37; `helm test` deferred to M9B-B unless it is judged to fit here — see M9B-A.9's own contract) | PLANNED — NOT STARTED |
| M9B-A.10 | SAUR-ON M7 gateway integration (corresponds to MH9) | PLANNED — NOT STARTED |
| M9B-A.11 | Full differential acceptance / regression / soak (corresponds to MH8, expanded with M1-M9 regression per this project's own combined-acceptance convention) | PLANNED — NOT STARTED |

**Ordering rationale**: M9B-A.0-A.4 build the substrate (storage,
lifecycle state machine, manifest handling) with no cluster-mutating
apply logic yet, mirroring MH0-MH2's own "no cluster mutation yet" gating.
M9B-A.5 (CSA apply) must land before M9B-A.6 (rollback/uninstall) can
compose it — this is a hard dependency, not a suggestion, since rollback/
uninstall's own call graphs (research §3-§5) both bottom out in the same
apply/delete primitive. M9B-A.6 explicitly ships *before* the hook engine
(M9B-A.7) and SSA (M9B-A.8) — narrower, honestly-labeled first scope per
research §17/§21 — with both gated by an explicit hard-refusal guard
rather than silent unsupported-case handling. M9B-A.9 (diff/test/read-
completion) is placed after A.6-A.8 because rollback-case diff-preview is
a free byproduct of the apply layer only once that layer actually exists
(research §33) and `helm test`, if included here at all, is a strict
subset of the hook engine (research §29) and therefore cannot precede
M9B-A.7. M9B-A.10 (gateway integration) is last among the build slices
because it is the "wire it into the app" step, mirroring M9.2/M9.4's own
precedent of proving the engine's correctness in isolation (fake-HTTP +
live + differential) before any UI wiring. M9B-A.11 closes the milestone
with the same combined-acceptance/regression/soak discipline every prior
milestone's own final slice used.

## Per-slice contracts

Each slice below covers, per the brief's required structure: purpose,
scope, explicit non-goals, architecture/invariants, security constraints,
mutation-gateway requirements, exact stop-and-ask decisions, unit tests,
fake-HTTP tests, live tests, differential Helm-oracle tests,
failure-injection tests, acceptance criteria, documentation updates, and
commit/tag expectations. All of it is a proposal to be reviewed, not a
commitment, exactly like every prior milestone's own per-slice contracts
were before implementation began.

### M9B-A.0 — Acceptance contract + architecture freeze

**Purpose**: formalize `docs/NATIVE_HELM_ENGINE_RESEARCH.md` into a
written, versioned compatibility contract precise enough that a second
engineer can implement M9B-A.1 from this document alone, without
re-reading upstream Helm source from scratch (research MH0's own stop
condition, restated as this slice's acceptance bar).

**Scope**: this document itself, reviewed and signed off; resolution of
the two §20 "must-close-before-first-write" items that gate the earliest
slices (see "Architectural questions" below) to the extent they can be
closed without writing engine code (e.g. reading, not yet implementing,
`k8s.io/apimachinery/pkg/util/strategicpatch`); an explicit, written
decision on the field-manager identity string (research §15/§19.5) and
the silent-409-retry divergence (already decided above, restated here as
the slice that makes it official); a documented differential-test harness
convention (real `helm` CLI as a versioned, pinned test-only dependency —
exact version(s) to pin, likely mirroring the research document's own
v3.22.0/v4.3.0 inspection targets, decided here not guessed at
implementation time).

**Explicit non-goals**: no code in `crates/helm-engine` yet — that is
M9B-A.1. No fixture cluster stood up yet.

**Architecture/invariants**: none newly introduced; this slice consumes
and ratifies the invariants section above.

**Security constraints**: none newly introduced at this slice (no code
exists yet to constrain).

**Mutation-gateway requirements**: none yet.

**Exact stop-and-ask decisions**: (1) is the field-manager string decided
here final for the milestone, or is it explicitly revisitable once
M9B-A.8's SSA work exposes a real conflict — stop and get explicit
sign-off before treating it as permanently fixed. (2) Which exact `helm`
CLI version(s) will be pinned as test oracles, and how they are
provisioned in CI/local dev (a real, non-trivial ongoing maintenance
cost per research §19.6) — stop and get explicit sign-off on the pinning
strategy before M9B-A.1 begins, since every later differential test
depends on this being decided once, not ad hoc per slice.

**Unit / fake-HTTP / live / differential / failure-injection tests**: none
(pure documentation slice).

**Acceptance criteria**: this document (M9B-A) exists, is internally
consistent with `docs/NATIVE_HELM_ENGINE_RESEARCH.md` and
`docs/M9_ACCEPTANCE.md`, and the two stop-and-ask decisions above are
recorded with explicit sign-off in this document's own Journal before
M9B-A.1 starts.

**Documentation updates**: this file. No other file rewritten.

**Commit/tag expectations**: no tag at this slice (mirrors M9/M8B's own
convention of tagging only at final combined acceptance, M9B-A.11).

### M9B-A.1 — Internal workspace crate skeleton (`crates/helm-engine`)

**Purpose**: stand up the crate boundary itself — the single largest
structural decision this milestone inherits from the research document's
own §16 analysis (internal workspace crate, not in-tree modules, not a
published crate).

**Scope**: `Cargo.toml` workspace member `crates/helm-engine`; the module
skeleton from research §15's architecture proposal (`storage`, `manifest`,
`apply`, `lifecycle`, `hooks` — module boundary present even before
implemented, per research §9's own "no hooks yet" gate being an explicit,
checkable absence rather than a silent one — `actions`, `compat_tests`);
zero behavioral logic yet, only types/module structure and the crate's
own narrow, `pub(crate)`-disciplined visibility rules established as a
compiler-enforced fact, not a comment (research §16's own stated
advantage of option (B) over (A)).

**Explicit non-goals**: no storage read/write logic yet (M9B-A.2). No
dependency on `crates/helm-engine` from the main `sauron` binary yet
beyond an empty `pub` surface — wiring into the app happens at M9B-A.10.

**Architecture/invariants**: the crate must not import
`resources::Object`, `Store`, `Timeline`, the graph, or the mutation
journal directly — its only interface to the rest of the codebase is
narrow, explicit function signatures (bytes/records in, effect
descriptions out), enforced at the module-visibility level, matching
M9.5's own `decode_release` `pub(crate)` discipline but now at a stronger,
crate-boundary level (research §16's own comparison table).

**Security constraints**: no raw Secret type may be `pub` anywhere in
this crate's public surface at this slice or any later one — restated
here as the founding rule the rest of the milestone must not violate.

**Mutation-gateway requirements**: none yet (no actions exist).

**Exact stop-and-ask decisions**: does `crates/helm-engine` reuse
`kube-rs`/`k8s-openapi` versions identical to the main crate's own
`Cargo.toml`, or does the workspace need a shared version-pin mechanism —
resolve before M9B-A.2 needs a `DynamicObject`.

**Unit tests**: module-visibility tests proving the crate's raw types are
not reachable from outside `crates/helm-engine` (a compile-fail test via
`trybuild` or an equivalent convention already used elsewhere in this
codebase, if any — otherwise decided here).

**Fake-HTTP / live / differential / failure-injection tests**: none at
this slice (no logic to test yet).

**Acceptance criteria**: `cargo build`/`cargo fmt --check`/`cargo clippy
--all-targets -D warnings` clean across the whole workspace including the
new crate; zero behavior change to the existing `sauron` binary (the new
crate is not yet depended upon).

**Documentation updates**: this document's Journal, recording the crate
layout decided.

**Commit/tag expectations**: no tag.

### M9B-A.2 — Release model + storage round-trip compatibility

**Purpose**: implement the `Release`-equivalent record type (research
§15's `storage::record`, mirroring `pkg/release/release.go`'s own field
shape: name/namespace/version/info/chart/config/manifest/hooks/labels)
and both directions of storage I/O — read (reusing M9.5's existing
`decode_release`/`sanitize` logic by direct dependency or vendored
equivalent, never reimplemented) and the **new** write primitive,
`write_release`, mirrored in shape from `kube::helm::read_release`: one
function, narrowly scoped, never generalized into a Secret-writing helper
any other caller could reach for (research §11.1). Corresponds to
research MH1.

**Scope**: `storage::secret_driver` (Secret-only, per this milestone's
own non-goal on other drivers); `storage::record`; `storage::encode_decode`
(the decode side reused, the encode side — gzip+double-base64 — newly
built as the literal inverse); no lifecycle/history logic yet (that is
M9B-A.3) — this slice writes releases nobody asked it to *compute the
content of*; content is hand-fixtured, exactly like research MH1's own
stop condition specifies.

**Explicit non-goals**: no revision allocation logic (M9B-A.3). No apply/
delete logic (M9B-A.5). No rollback/uninstall action yet (M9B-A.6).

**Architecture/invariants**: `write_release` takes an already-validated,
in-memory release record and performs exactly one bounded Create/Update —
never a batch, never exposed to a generic mutation payload path (research
§11.1, restated verbatim as this slice's own binding contract). The full
unredacted release record exists in memory only transiently, local to the
function performing storage I/O, dropped on return — never cached, never
logged, never passed to any generic pipeline (research §11.2, extending
M9.5's own read-side guarantee to the write side).

**Security constraints**: every new error type in this slice's modules
must have its own `error_display_never_embeds_raw_payload_content`-style
test (M9.5's own precedent, restated as a binding requirement here, not a
suggestion). No `Debug` derive on any type that could carry raw release
bytes in a way that could leak into a panic message (research §11.2).

**Mutation-gateway requirements**: none yet — this slice's write path is
exercised only by its own direct unit/fake-HTTP/live tests, not yet
wired through `mutation::workflow`/`kube::mutation` (that happens at
M9B-A.10). This is a deliberate ordering choice: prove storage
correctness in isolation before composing it under the gateway.

**Exact stop-and-ask decisions**: is the encode side's exact byte-for-byte
gzip/base64 framing verified identical to what real Helm's own
`encodeRelease` produces (matters for §13's "self-decode-only is not
sufficient evidence" requirement) — this needs an explicit differential
check before M9B-A.2 is considered complete, not assumed from "gzip is
gzip."

**Unit tests**: `Release` record round-trips through `encode_decode`
losslessly for a range of hand-constructed fixtures (empty values, deeply
nested values, unicode, large manifest near the M9.5 decompression-bomb
boundary reused unchanged); every distinct encode failure mode has its
own explicit error variant, never a collapsed "write failed."

**Fake-HTTP tests**: `write_release` issues exactly one bounded Create/
Update against a fake HTTP endpoint with the expected Secret shape
(labels: `name`/`owner: helm`/`status`/`version`/`modifiedAt` or
`createdAt`, never both — research §7); no retry on a simulated
transient failure.

**Live tests**: write a hand-fixtured release record to a real Secret in
an isolated namespace, then read it back via the **real `helm` CLI**
(`helm get manifest`/`helm get values`/`helm status` against that
release name) and confirm it parses and matches expectations — this is
the "self-decode-only is not sufficient evidence" requirement (research
§13) made concrete as this slice's own core live test.

**Differential Helm-oracle tests**: for an identical hand-specified
`Release` struct, compare the Secret real `helm` itself would have
written (constructed via the real CLI performing an equivalent operation
against a disposable release) against the engine's own written Secret —
byte-for-byte decode equivalence, per research MH1's own stated stop
condition.

**Failure-injection tests**: a write that fails partway (simulated
transport error after the request is sent but before a response is
observed) must surface as an explicit ambiguous outcome, never silently
assumed to have succeeded or failed (mirrors `Verification::Unknown`'s
existing semantics).

**Acceptance criteria**: MH1's own stop condition, verbatim: "real
`helm`-written Secret vs engine-written Secret for an identical
hand-specified `Release` struct decode identically."

**Documentation updates**: this document's Journal; `crates/helm-engine`'s
own module-level doc comments carrying the load-bearing security contract
forward (mirroring `src/integrations/helm.rs`'s own module doc comment
style).

**Commit/tag expectations**: no tag.

### M9B-A.3 — Lifecycle/history/status semantics

**Purpose**: implement the pending-*/deployed/failed/superseded/
uninstalling/uninstalled status-machine transitions, strictly-monotonic
revision allocation, and history query/sort/supersede/prune logic — pure
state-machine logic, no cluster mutation yet. Corresponds to research MH2.

**Scope**: `lifecycle::status_machine`, `lifecycle::revision`,
`storage::history` (query/sort-by-revision/supersede-all-prior-deployed/
`MaxHistory` prune). Every row of research §10's failure table gets its
own reproduced state transition.

**Explicit non-goals**: no cluster-facing apply/delete (M9B-A.5). No
actual rollback/uninstall action composing these primitives yet
(M9B-A.6).

**Architecture/invariants**: revision allocation is strictly
`current.Version + 1` for every transition — no "revision reuse" path
anywhere (research §4.2, §7). Pruning (`MaxHistory`) keeps the currently
deployed revision unconditionally and is documented here as **not atomic**
with the create it makes room for, mirroring upstream's own admitted gap
(research §7) — this is not "fixed" by the engine; the engine's own
crash-mid-prune behavior must be defined and tested (see failure-injection
below), not silently assumed safe.

**Security constraints**: none new beyond M9B-A.2's (this slice is pure
in-memory logic).

**Mutation-gateway requirements**: none yet.

**Exact stop-and-ask decisions**: does the engine's own `pending-*`
hard-refusal invariant (this document's own stricter-than-upstream
divergence #3) get implemented as a check inside `lifecycle::
status_machine` itself, or as a precondition check in the future
`actions::rollback`/`actions::uninstall` composition layer (M9B-A.6) —
resolve before M9B-A.6 needs to call it, since it affects where the unit
tests for this behavior belong.

**Unit tests**: every §10 failure-table row (research document, "Case"
column) has a passing unit test reproducing the exact resulting state
(MH2's own stated stop condition, verbatim); revision monotonicity under
repeated transitions; supersede-all-prior-deployed correctness
(research's own citation of upstream issue #2941/#12556's defensive
pattern); `MaxHistory` prune keeping the deployed revision unconditionally;
the `pending-*` hard-refusal check itself, independent of which layer
ends up calling it.

**Fake-HTTP / live tests**: none required at this slice (pure
state-machine logic, no cluster I/O) — deferred to M9B-A.5/A.6 where
this logic is first exercised against a real cluster.

**Differential Helm-oracle tests**: none required at this slice (state
transitions are verified against the research document's own
source-derived table, which is itself the oracle for this layer); real
differential cluster tests begin at M9B-A.5.

**Failure-injection tests**: a simulated crash between prune and create
(research §7's own flagged gap) must leave the engine's own history in a
state whose *shape* matches upstream's documented gap (short by one slot,
not corrupted) — this is the first slice-level attempt at research §20
item 7's "reproduce a mid-operation process crash" experiment, narrowed
to the pure state-machine layer (no cluster involved yet); resolve
whether this experiment closes §20 item 7 fully or only partially before
M9B-A.6 relies on its conclusion.

**Acceptance criteria**: MH2's own stop condition, verbatim: "every §10
row has a passing unit test reproducing the exact resulting state."

**Documentation updates**: this document's Journal.

**Commit/tag expectations**: no tag.

### M9B-A.4 — Manifest parsing/sort/resource-policy/delete semantics

**Purpose**: implement manifest split/sort-by-kind (install/uninstall
kind-order tables, taken literally from research §7's upstream-cited
lists) and the `helm.sh/resource-policy: keep` filter, read from the
**stored** manifest's annotations, never the live object's current
annotations (research §5.5's own explicitly-cited subtlety a naive
reimplementation would get wrong).

**Scope**: `manifest::split_sort`, `manifest::resource_policy`. No apply/
delete execution yet (M9B-A.5) — this slice produces the *ordered,
filtered resource list*, it does not act on it.

**Explicit non-goals**: no CRD-install-first special casing (that is
M9B-B's `install` scope, per research §22 — rollback/uninstall never
install a CRD).

**Architecture/invariants**: `resource-policy: keep` is evaluated from
the release's own recorded manifest annotations at the time of storage,
never from a fresh GET of the live object (research §5.5) — a resource
whose live annotation was added later out-of-band is NOT protected by
this filter; one whose recorded annotation says `keep` but was removed
live IS still protected. This must be a tested, documented behavior, not
an implementation accident.

**Security constraints**: none new.

**Mutation-gateway requirements**: none yet.

**Exact stop-and-ask decisions**: none identified beyond the Architectural
questions section below (CRD three-way-merge specifics are M9B-A.5's/
A.8's concern, not this slice's).

**Unit tests**: kind-sort ordering matches the research document's own
cited upstream literal lists for both install-order and uninstall-order;
`resource-policy: keep` filter reads stored-manifest annotations, proven
via a fixture where the live object's annotation differs from the stored
manifest's own annotation in both directions (research §5.5's own two
scenarios, both must have a dedicated test).

**Fake-HTTP tests**: none required (pure manifest-text logic).

**Live tests**: none required at this slice.

**Differential Helm-oracle tests**: sort order and keep-filter output for
a real chart's rendered manifest (taken from an M9.5-readable existing
release) matches the ordering/filtering real `helm`'s own
`releaseutil.SortManifests`/`filterManifestsToKeep` would produce for the
same input — a hand-verified comparison against the research document's
own cited source, not yet a live differential operation (that starts at
M9B-A.6).

**Failure-injection tests**: malformed manifest documents (unparseable
YAML, missing `kind`) produce an explicit error, never a silently-skipped
document.

**Acceptance criteria**: kind-order and resource-policy behavior matches
the research document's own §5.5/§7 findings exactly, proven by the unit
tests above.

**Documentation updates**: this document's Journal.

**Commit/tag expectations**: no tag.

### M9B-A.5 — CSA reconciliation path

**Purpose**: the single highest-risk primitive in this entire milestone
(research §8, §19.1): a three-way-merge-equivalent apply (matching
`strategicpatch.CreateThreeWayMergePatch`'s actual algorithm, not a
hand-rolled approximation) plus propagation-policy-aware delete with
NotFound-as-success. Corresponds to research MH3.

**Scope**: `apply::csa`, `apply::delete`. Explicit hard-refusal guard for
any release showing SSA-managed live resources (deferred to M9B-A.8) and
for any release whose target manifest contains hooks for the relevant
event (deferred to M9B-A.7) — both guards must exist and be tested at
this slice, even though neither capability is implemented yet, per
research §9/§17's own "explicit refusal, not silent skip" requirement.

**Explicit non-goals**: no SSA path (M9B-A.8). No hook execution
(M9B-A.7). No rollback/uninstall action composing this yet (M9B-A.6).

**Architecture/invariants**: this is the slice that must close research
§20 item 1 before it can be considered complete — `strategicpatch.
CreateThreeWayMergePatch`'s own algorithm (list merge-key handling,
`patchMergeKey` strategic-merge tag semantics) must be independently
studied from `k8s.io/apimachinery` source, not assumed from "three-way
merge sounds simple." The field manager sent with every PATCH/CREATE/
DELETE is the explicit, documented string decided at M9B-A.0 — never
`"helm"`, never silently impersonating the real CLI (research §15/§19.5).
Delete-on-rollback (the "original.Difference(target)" branch, per
research fixture #5) and create-on-rollback (fixture #4) are both
distinct code paths within this apply layer, not implied by patch alone.

**Security constraints**: none new beyond M9B-A.2's write-path
constraints — this slice's apply calls carry no additional Secret-body
exposure risk (it operates on already-decoded, in-memory manifest
documents, which may themselves contain a chart-embedded `Secret`
*resource* — fixture #3's own explicit target — which must never be
treated as the release-storage Secret and must flow through this layer's
ordinary create/patch/delete path like any other resource, never
specially redacted or specially exposed).

**Mutation-gateway requirements**: `apply::csa`/`apply::delete`'s actual
cluster-mutating calls are dispatched as new *executor branches* under
the existing `kube::mutation` gateway (mirroring M9.2/M9.4's own
precedent of "no second gateway"), not a parallel client. This is the
slice where that composition is first proven, even though the full
rollback/uninstall action wiring is M9B-A.6/A.10.

**Exact stop-and-ask decisions**: (1) research §20 item 1 (the
strategic-merge algorithm study) must be closed — explicit sign-off that
the study is sufficient before writing the apply logic, not merely before
merging it. (2) research §20 item 7 (a real crash-mid-write experiment)
should be attempted here against a live disposable cluster (`kill -9` the
process mid-commit against a throwaway namespace, per the research
document's own suggested design) before this slice's failure-state claims
are trusted for M9B-A.6's own rollback composition — stop and get
explicit sign-off on the experiment's design before running it, since it
is deliberately destructive to the test harness process itself.

**Unit tests**: three-way-merge patch generation against hand-constructed
old/new/live triples covering: field present in old, absent in new
(explicit removal, even if live differs — this is the "silently overwrite
an operator's live edit" hazard research §8 names explicitly, and the
test must assert the *documented* Helm-compatible behavior, not a
"safer" invented one); field never in old or new (left alone); list-valued
fields with a strategic-merge key (the "real, non-trivial porting work"
research §19.1 flags) — a dedicated fixture set, not a single happy-path
test.

**Fake-HTTP tests**: apply issues the exact expected sequence of create/
patch/delete calls for a given old/new resource-set diff, with the
documented field-manager string attached to every call; delete issues
`NotFound`-tolerant calls (a 404 on delete is success, never an error);
no retry on a simulated 409.

**Live tests**: apply a hand-fixtured three-way-merge scenario against a
real disposable namespace and confirm the resulting live object matches
the expected merged state exactly (not "looks close").

**Differential Helm-oracle tests**: fixtures #1, #2, #4, #5, #6, #11-14
(research §14) — baseline apply/delete correctness, two-way-vs-three-way
patch divergence, create-on-rollback, delete-on-rollback, the
manually-modified-live-resource-then-rollback fixture (§14 fixture #6,
explicitly named "the single riskiest correctness fixture"), PVC
delete-propagation, CRD/CR two-way-merge path, already-missing-resource
no-op, API-conflict divergence — run via the CSA-only path since SSA
does not exist yet.

**Failure-injection tests**: API conflict (409) on patch/delete surfaces
as an explicit outcome, never retried (this document's own divergence
#1); the crash-mid-write experiment from the stop-and-ask decision above.

**Acceptance criteria**: research MH3's own stop condition, verbatim:
"Fixtures #1-6, #11-14 (§14) pass differentially against real
Helm v3-managed releases."

**Documentation updates**: this document's Journal, including the
outcome of the §20 item 1 study and the crash-mid-write experiment
(promoting both from "explicit unknown" to "resolved, evidence recorded"
or "still open, here is why").

**Commit/tag expectations**: no tag.

### M9B-A.6 — First guarded native rollback/uninstall for supported CSA releases

**Purpose**: compose M9B-A.2-A.5 into the first real, guarded
`rollback`/`uninstall` actions, exactly per research §4/§5's own call
graphs, for releases the engine can positively detect as compatible
(CSA-managed, no hooks for the relevant event). Corresponds to research
MH5/MH6, deliberately combined into one slice here since both actions
share the overwhelming majority of their machinery (research §6's own
shared-primitives table) and both need the same hook/SSA refusal guard
proven before either can safely ship.

**Scope**: `actions::rollback`, `actions::uninstall`; the explicit
compatibility-detection step (does the target manifest contain hooks for
the relevant event; does any live resource show SSA management) that
runs before any cluster mutation and produces a hard, explicit refusal
with a stated reason on a positive hit — never a silent partial attempt.
Both actions still return `mutation::workflow`-shaped preview/build
output for exercise by this slice's own tests, but full `Command`/
gateway wiring is M9B-A.10 (kept separate deliberately, mirroring how
M9.2/M9.4 proved their own executor dispatch before any UI wiring).

**Explicit non-goals**: no hook execution (M9B-A.7) — refused explicitly
instead. No SSA (M9B-A.8) — refused explicitly instead. No `:helm_rollback`/
`:helm_uninstall` command grammar yet (M9B-A.10).

**Architecture/invariants**: rollback creates a **new** revision
(`current.Version + 1`); it never rewrites or reactivates the old
revision's own Secret (research §4.2) — proven by a dedicated test
asserting the rolled-back-to content lives in a *new* revision number.
The new revision is persisted twice — once as `pending-rollback` before
any cluster mutation, once as `deployed`/`failed` after (research §4.3)
— and this two-write shape is preserved deliberately, not "simplified"
to a single write, because it is exactly what research §10's
crash-mid-operation failure table depends on being true. Hooks run twice
around rollback's apply step in real Helm (pre/post-rollback) — since
hooks are out of scope here, the compatibility-detection guard must
positively confirm their absence before proceeding, never merely "not
implemented, so skipped." Uninstall only ever acts on the **latest**
revision by version number (research §5.1) — never parameterized by
revision. Re-uninstalling an already-`Uninstalled` release with
`KeepHistory=false` is a purge, not an error (research §5.2) — this
specific "already done is success" case must be a tested behavior, not
an accidental omission.

**Security constraints**: inherits M9B-A.2's write-path constraints in
full; additionally, the journal entry for both actions records intent
(source_action, release name/namespace, from-revision, to-revision for
rollback) and outcome only — never the decoded chart values/manifest/
notes (research §11.3) — this is the slice where that design decision
becomes real code, not just a stated intention.

**Mutation-gateway requirements**: both actions compose M9B-A.5's
executor-dispatch-branch apply/delete calls and M9B-A.2's write-primitive
storage calls under the same `kube::mutation` gateway; still no second
gateway. TOCTOU: both the release's current-revision Secret UID and (for
rollback) the specific target historical revision's own Secret UID are
re-verified fresh at commit time (this document's invariant, restated as
binding for this slice specifically, since it is the first slice where
both identities exist simultaneously).

**Exact stop-and-ask decisions**: (1) what exact preview/confirmation
strength these two actions receive under `mutation::policy::evaluate` —
proposed: `RequireStrongerConfirmation` unconditionally for both,
matching M8B.6's own "highest-risk single operation" precedent and the
research document's own repeated emphasis that these are genuinely
higher-risk than an ordinary PATCH — but this needs explicit sign-off
before implementation, not an assumed default. (2) exact preview text
must explain the "creates a new revision, does not restore/resurrect the
old one" fact plainly (research §4.2) so a user invoking rollback is not
misled into thinking "v1" reappears rather than a new "v4" mirroring it —
stop and review the exact wording before shipping.

**Unit tests**: hook-presence detection correctly identifies every
hook-bearing fixture and refuses before any cluster call; SSA-management
detection correctly identifies a live-resource `managedFields` fact
(hand-fixtured, since no real SSA-managed release fixture exists until
M9B-A.8) and refuses; rollback's two-phase (`pending`→terminal) persist
sequence is exercised in isolation from cluster I/O via the M9B-A.3 state
machine; uninstall's "already-Uninstalled + purge" path.

**Fake-HTTP tests**: rollback's full commit sequence (pending-write,
apply, supersede-all-prior-deployed, terminal-write) against a fake
endpoint, asserting exact call order and exact payload shape at each
step; uninstall's delete-then-status-update sequence; both actions'
refusal paths issue **zero** cluster-mutating calls when the
compatibility-detection guard fires (a dedicated regression test, mirroring
M8B.4's own "429 never triggers a fallback DELETE" test discipline).

**Live tests**: against a real, disposable, hook-free, CSA-managed Helm
release (created via the real `helm` CLI as pure test-harness tooling,
mirroring M9.5's own `demo-release` convention) on the isolated test
cluster: rollback to an immediately-prior revision, verified via a fresh
GET of both the new release Secret and the affected live resources;
uninstall (with and without `--keep-history`-equivalent behavior),
verified via a fresh GET confirming the expected resources are gone (or
kept, per `resource-policy: keep`) and the release Secret(s) reflect the
expected terminal state.

**Differential Helm-oracle tests**: fixtures #16-17, #21 (research §14)
plus the already-covered #1-6/#11-14 from M9B-A.5, now exercised through
the full action rather than the bare apply layer; the "subsequent real
`helm upgrade`" **critical test** (research §13) — after the engine
performs rollback/uninstall, the real `helm` CLI must still successfully
run `helm status`/`helm history`/`helm rollback`/`helm upgrade`/`helm
uninstall` against the touched release without error or reported
corruption. This critical test gates M9B-A.6's own acceptance, not a
final smoke test.

**Failure-injection tests**: every applicable research §10 row is
reproduced end-to-end through the composed action (not just the isolated
state-machine layer M9B-A.3 already covered) — mid-apply failure leaves
`current=Superseded, new=Failed` per research's own table; a simulated
crash between the pending-write and the apply step leaves a permanently
`pending-rollback` release, and a **fresh invocation of rollback against
that same release is refused** by this document's own `pending-*`
hard-refusal invariant (proving the deliberate divergence from upstream's
own laxity actually holds, not just documented).

**Acceptance criteria**: research MH5/MH6's own stop conditions
(verbatim): "Fixtures #1-9, #16-17, #21 pass differentially, including
the 'subsequent real `helm upgrade`' critical test" (rollback) and
"Fixtures #10, #13-15, #18-20 pass differentially" (uninstall) — note
fixtures #7-9 (hooks) are **not** required to pass yet, since hooks are
explicitly refused at this slice, not silently attempted; those three
fixtures move to M9B-A.7's own acceptance criteria instead.

**Documentation updates**: this document's Journal; a new
`docs/M9B_A_COMPAT_MATRIX.md`-style artifact is explicitly **not** created
speculatively here — if the differential fixture results warrant a
dedicated compatibility-matrix document, that is decided and named at
this slice's own implementation time, not guessed at in this planning
ledger.

**Commit/tag expectations**: no tag (mirrors M9's own precedent of tagging
only at the milestone's final combined-acceptance slice).

### M9B-A.7 — Hook engine

**Purpose**: implement the full hook sub-lifecycle (research §9):
weight-then-name-sorted execution, per-event dispatch (pre/post-rollback,
pre/post-delete, and — opportunistically, since it is the same engine —
pre/post-install/upgrade for M9B-B's later benefit, though M9B-A itself
only needs the rollback/uninstall events), default `before-hook-creation`
delete policy when a hook specifies none, the hardcoded
`CustomResourceDefinition`-is-never-deleted-by-hook-policy carve-out,
watch-until-ready for Job/Pod (no-op wait for other kinds), and
failure-triggered log capture plus abort-before-mutation semantics.
Corresponds to research MH4.

**Scope**: `hooks::engine` (the module boundary already exists from
M9B-A.1; this slice fills it in). Rollback's `HookPreRollback`/
`HookPostRollback` and uninstall's `HookPreDelete`/`HookPostDelete` events
are wired into M9B-A.6's own actions, replacing the hard-refusal guard
with real execution for hook-bearing releases.

**Explicit non-goals**: `helm test`'s own filter/`--logs` extension is
**not** built here unless M9B-A.9 explicitly decides it belongs in this
milestone rather than M9B-B (see M9B-A.9's own contract) — this slice
builds the hook *engine*, not every consumer of it.

**Architecture/invariants**: hook resources whose delete-policy includes
`hook-succeeded` are deleted in reverse execution order after every hook
in the batch succeeds; a hook failure aborts the entire action at that
point (research §9) — rollback: before any resource `Update`; uninstall:
before `deleteRelease`, since `pre-delete` runs first. `CustomResourceDefinition`
is never deleted by hook-delete-policy, hardcoded, regardless of the
hook's own declared policy (research §9's own explicit citation of this
carve-out) — a dedicated test must prove this holds even when a hook
explicitly declares a delete policy that would otherwise delete it.

**Security constraints**: hook log capture is a **new** place raw secrets
could leak (a hook Job's stdout can legitimately contain anything,
including credentials a chart's hook script printed) — per research
§11.4, this is treated as its own explicit, separately-reviewed security
decision at this slice, not inherited implicitly from the values/manifest
redaction model, because log content has no structure to redact by key.
This slice must not ship log capture without an explicit, written
decision on what (if anything) is safe to surface, mirroring M9.5's own
"Notes are deliberately never shown" precedent as the default posture
unless a narrower, reviewed exception is proposed and accepted here.

**Mutation-gateway requirements**: hook resource create/delete calls are
new executor-dispatch branches under the same `kube::mutation` gateway,
composed with M9B-A.5's apply/delete layer, not a parallel implementation.

**Exact stop-and-ask decisions**: the hook-log-capture security decision
above is an explicit stop-and-ask item — implementation must not proceed
past "hooks execute, but do we show their logs" without a written
decision recorded in this document's Journal first.

**Unit tests**: hook sort order (weight then name); default delete policy
applied when none declared; CRD-never-deleted-by-hook-policy holds even
against an explicit contrary declaration; abort-before-mutation ordering
for both rollback (before `Update`) and uninstall (before `deleteRelease`).

**Fake-HTTP tests**: hook Job/Pod creation, watch-until-ready polling
sequence, delete-by-policy calls, in the exact order research §9's call
graph specifies; a failing hook issues zero further mutating calls beyond
what the failure-handling itself requires (no accidental partial apply
after an abort).

**Live tests**: a real hook-bearing release (created via the real `helm`
CLI as test-harness tooling) with a pre-rollback and post-rollback hook,
rolled back by the engine, confirming both hooks actually ran (observed
via the hook Job's own completion) in the correct order and were cleaned
up per their declared (or default) delete policy.

**Differential Helm-oracle tests**: fixtures #7, #8, #9 (research §14) —
hook execution correctness, weight ordering, hook-failure abort semantics
— now run against the composed rollback/uninstall actions from M9B-A.6,
completing those actions' own fixture coverage.

**Failure-injection tests**: a hook that exits nonzero aborts the action
before any resource mutation (rollback) or before `deleteRelease`
(uninstall), and the release's own stored state reflects "no state change
yet" for rollback / the correct pre-delete-hook-failure shape for
uninstall, per research §10's own row for this exact case.

**Acceptance criteria**: research MH4's own stop condition, verbatim:
"Fixtures #7-9 pass differentially." Additionally, the hook-log-capture
security decision is written down and either implemented per its own
explicit scope or explicitly deferred with a stated reason (never
silently shipped without the decision existing).

**Documentation updates**: this document's Journal, including the
hook-log-capture security decision in full (mirroring M9.5's own
in-document security-decision write-up style).

**Commit/tag expectations**: no tag.

### M9B-A.8 — SSA reconciliation + CSA→SSA compatibility behavior

**Purpose**: implement the Server-Side Apply path (research §8's "v4"
regime) and the regime-detection logic that decides, per release and per
live resource, whether CSA or SSA is the correct apply strategy — since
Helm itself records no "which apply strategy was used" field anywhere in
the release object; it must be inferred from the live object's
`managedFields` at operation time (research §8's own central finding).
Corresponds to research MH7.

**Scope**: `apply::ssa`, plus the regime-detection function shared by
both apply paths. Closes the SSA hard-refusal guard from M9B-A.5/A.6,
letting rollback/uninstall now operate on SSA-managed releases too.

**Explicit non-goals**: no CSA→SSA *migration* logic beyond what
correctness requires here (a full "help a user deliberately migrate a
release from CSA to SSA management" feature, if ever wanted, is not
scoped by this slice — only correct *detection and interoperation* with
whichever regime a release is already in).

**Architecture/invariants**: this slice must independently close research
§20 item 5 first — `k8s.io/client-go/util/csaupgrade`'s
`UpgradeManagedFields` behavior (cited, not read, in the original
research pass) must be read in full before this slice's SSA-compatibility
claims are trusted, not merely before merging. The engine's own field
manager identity (decided at M9B-A.0, used consistently since M9B-A.5)
must never silently impersonate `"helm"` — restated here because SSA's
own field-manager-based conflict semantics make an incorrect manager
string a *guaranteed* future conflict with the real `helm` CLI, not
merely a theoretical risk (research §8/§15/§19.5).

**Security constraints**: none new beyond prior slices.

**Mutation-gateway requirements**: SSA apply calls are a new executor
branch under the same gateway, using `kube-rs`'s own `Api::patch`/
`PatchParams::apply` (already SSA-capable per research §15's own
architecture answer) — no new patch mechanism outside the existing
gateway's dispatch model.

**Exact stop-and-ask decisions**: research §20 item 5's closure is a hard
stop-and-ask gate — this slice must not proceed past regime-detection
design until that reading is done and its conclusion (reproduce vs.
deliberately reject `UpgradeManagedFields`'s exact behavior, with a
stated reason either way) is recorded here.

**Unit tests**: regime-detection correctly classifies a hand-fixtured
`managedFields` array as CSA-managed vs. SSA-managed vs. ambiguous (and
an ambiguous classification is an explicit refusal, never a guess);
field-manager string is attached correctly on every SSA call.

**Fake-HTTP tests**: SSA apply issues the expected `PATCH` with `?
fieldManager=<string>&force=<bool>` semantics (or the `kube-rs` equivalent
call shape) against a fake endpoint; a conflicting field-manager response
(409-shaped for SSA) surfaces explicitly, never force-applied silently.

**Live tests**: against a real, disposable release installed via the real
Helm **v4** CLI (SSA-default, per research §8's own live-confirmed
finding) on the isolated test cluster: rollback performed by the engine
via the SSA path, verified via a fresh GET of the live resources'
`managedFields` showing the engine's own documented manager string, not
`"helm"`.

**Differential Helm-oracle tests**: fixture #22 (research §14) — a
release originally installed/upgraded via Helm v4 (SSA-default), rolled
back by the native engine, then successfully operated on again by the
real `helm` CLI (`helm upgrade`, per research MH7's own stated stop
condition) without a spurious field-ownership conflict.

**Failure-injection tests**: an SSA conflict (409 with a `managedFields`
ownership mismatch) surfaces as an explicit outcome, never silently
force-applied and never silently retried (this document's own divergence
#1, restated for the SSA-specific conflict shape, which is a materially
different wire response than a plain optimistic-lock 409).

**Acceptance criteria**: research MH7's own stop condition, verbatim:
"Fixture #22 passes; a release rolled back by the engine can be correctly
`helm upgrade`d afterward regardless of which apply strategy produced its
history."

**Documentation updates**: this document's Journal, including the
research §20 item 5 closure write-up.

**Commit/tag expectations**: no tag.

### M9B-A.9 — Diff/test/read-completion surfaces where appropriate

**Purpose**: ship the "free" or near-free surfaces the research document
identifies as byproducts of the apply/hook layers already built, without
scope-creeping into anything gated on chart rendering (M9B-B's territory).

**Scope**: rollback/uninstall-case diff-preview (research §33's "compute
the patch, don't apply it" mode switch over the same apply-strategy
engine M9B-A.5/A.8 already built — zero new apply logic, only a
render-instead-of-commit path reused by `Workflow.dry_run`, matching M7's
own existing `dry_run: Option<MutationOutcome>` field). `helm history`'s
own read-completion gap from M9.5 (research §35: a thin UI-only
presentation layer over data the generic Secret-list pipeline already
surfaces — explicitly **no new engine capability**, and explicitly
**matching Helm's own flat revision list, never inventing a derived
"restores revision N" column**, per research §35's own recommendation).
`helm get manifest`'s full-body / `helm get hooks` gaps from M9.5
(research §34) are explicitly **not** closed here — they remain the
standing, deliberate M9.5 security decisions research §34 confirms were
not oversights; reopening them is a separate, security-review-first
decision this slice does not make.

**Explicit non-goals**: `helm test` as its own action is **not** included
in this slice's scope by default — research §29 classifies it as "~90%
shared, thin filter/log-dump wrapper" around the hook engine, genuinely
usable from either M9B-A (it needs only M9B-A.7's hooks, nothing from
M9B-B) or M9B-B (per the brief's own suggested slice ledger, which lists
it under M9B-A's candidate scope too: "helm test lifecycle if it fits the
operational boundary"). **Decision, made explicit here rather than left
ambiguous**: `helm test` is included in M9B-A's scope, at this slice,
because it depends only on M9B-A.7 (hooks) and nothing from M9B-B
(rendering/install/upgrade) — matching the milestone-dependency
discipline this document's own "Relationship between M9B-A and M9B-B"
section requires. If a future implementer finds a concrete reason this
is wrong, that is a documented deviation from this ledger, not a silent
substitution.

**Architecture/invariants**: `helm test`'s log-capture semantics inherit
M9B-A.7's own hook-log-capture security decision unchanged — no separate
redaction model invented for test-hook logs specifically.

**Security constraints**: none new beyond M9B-A.7's hook-log decision,
reused here.

**Mutation-gateway requirements**: diff-preview reuses `Workflow.dry_run`
unchanged; `helm test`'s hook execution reuses M9B-A.7's own gateway
dispatch unchanged — no new gateway surface introduced by this slice.

**Exact stop-and-ask decisions**: none beyond the `helm test`
scope-placement decision already made explicit above.

**Unit tests**: diff-preview render mode produces the same patch content
the commit path would have applied, without issuing any mutating call;
`helm history`'s presentation layer matches the flat revision list
exactly, with no derived column.

**Fake-HTTP tests**: diff-preview issues zero mutating calls even when
given a fixture that would otherwise apply cleanly; `helm test`'s
`--filter`-equivalent name-based hook selection partitions correctly.

**Live tests**: diff-preview against a real disposable release shows the
same patch a subsequent real commit would apply (verified by actually
committing afterward and confirming no surprise); `helm test` against a
real `helm.sh/hook: test`-annotated fixture executes and reports
pass/fail correctly.

**Differential Helm-oracle tests**: research §33's rollback-diff case
(free extension of the apply layer, explicitly not gated on chart
rendering); the `helm test` fixture from research §29/MH17's own stated
stop condition ("native test execution + `--logs`-equivalent output
matches real `helm test --logs`").

**Failure-injection tests**: a `helm test` hook failure is reported
distinctly from a hook failure during rollback/uninstall (same
underlying mechanism, different caller context — must not be collapsed
into one ambiguous "hook failed" message without saying which action
triggered it).

**Acceptance criteria**: diff-preview and `helm history` ship with zero
new engine capability beyond what M9B-A.5/A.8's apply layer and the
generic Secret-list pipeline already provide (proven by code review, not
just tests, per this slice's own "free byproduct" framing); `helm test`
meets research MH17's own stop condition, verbatim.

**Documentation updates**: this document's Journal.

**Commit/tag expectations**: no tag.

### M9B-A.10 — SAUR-ON M7 gateway integration

**Purpose**: wire M9B-A.6/A.9's proven actions into the app-visible
surface — `Command::{HelmRollback, HelmUninstall}` (or equivalent exact
grammar, decided here), `mutation::workflow`-shaped builders, and the
existing `workflow_report` UI shell — exactly mirroring M9.2/M9.4's own
"no new gateway needed" precedent, now proven for Helm-shaped actions
too. Corresponds to research MH9.

**Scope**: command grammar; `mutation::workflow::{helm_rollback,
helm_uninstall}` builders returning `Built`; preview rendering (including
the diff-preview from M9B-A.9 if decided to surface interactively at this
slice, or deferred — decide explicitly, don't default silently);
`source_action` values `helm_rollback`/`helm_uninstall` for journal
correlation, following M9.0's own established naming convention.

**Explicit non-goals**: no new confirmation/policy/journal/TOCTOU
subsystem — this slice's entire acceptance bar (below) is proving that
claim true, not merely asserting it.

**Architecture/invariants**: restates this document's own Design test
verbatim as the acceptance criterion for this specific slice (see below).

**Security constraints**: the app-level command handler must select the
release the same way `open_helm_view`'s existing `:helm` command does —
using the already-selected object only to know *which* release to target,
never trusting its cached fields as authorization, with the engine's own
fresh-fetch re-verification (M9B-A.2/A.6) as the actual authority
(mirrors M9.5's own established pattern exactly).

**Mutation-gateway requirements**: this is the slice where the answer to
"did this need a second gateway" gets tested for real, against the app's
actual command/policy/journal wiring, not just the engine's own isolated
tests.

**Exact stop-and-ask decisions**: exact command grammar (`:helm_rollback
[REVISION]` / `:helm_uninstall`, or similar) — decide and record here
before implementing, following this project's own established
"grammar TBD, resolve before implementation" convention used at prior
milestones (e.g. M8B.2's own `:set_image` naming discussion).

**Unit tests**: command grammar parsing (revision argument required/
optional, exact error on malformed input); builder produces the correct
`MutationIntent`/`create_resource`-equivalent shape (this may need a new
`MutationEffect` variant if neither `Modify` nor `Delete` nor `Create`
cleanly fits a "compose a whole new engine action" shape — resolve this
as an explicit open question, not an assumption, before this slice's
unit tests are written).

**Fake-HTTP tests**: full command-to-commit round trip against a fake
endpoint, confirming policy/confirmation/journal all fire exactly as they
do for every other guarded action, with zero new dispatch mechanism
outside documented executor-branch extensions already proven at M9B-A.5/
A.6/A.8.

**Live tests**: interactive TUI coverage may be deferred to M9B-A.11's
combined acceptance script, mirroring M8.2/M8B.1's own established
precedent of not requiring every individual slice to carry its own
dedicated interactive scenario.

**Differential Helm-oracle tests**: none new at this slice — this slice
is app-wiring, not engine behavior; the differential proof already
happened at M9B-A.6/A.7/A.8.

**Failure-injection tests**: a stale selection (release advanced by a
concurrent real `helm` operation between the user's selection and commit)
is rejected the same way `HelmReadError::TargetReplaced` already rejects
a stale read selection (M9.5's own precedent, now proven for the write
path too).

**Acceptance criteria**: this document's own Design test, verbatim:
"adding this action required no new confirmation/policy/journal/TOCTOU
subsystem" — restated from `docs/M9_ACCEptance.md`'s own carried-over
discipline for M9.4's rollback wiring, now proven true (or, if false,
explicitly written up as a real finding, not silently patched around).

**Documentation updates**: this document's Journal; `HANDBOOK.md`'s own
command reference, if that document lists guarded commands (checked at
implementation time against whatever `HANDBOOK.md` looks like then).

**Commit/tag expectations**: no tag yet — M9B-A.11 is the milestone's
final, tagged slice.

### M9B-A.11 — Full differential acceptance / regression / soak

**Purpose**: the milestone's combined acceptance, mirroring every prior
milestone's own final-slice structure (M8B.7, M9.7) and research MH8's
own stated scope: run the full differential fixture matrix (research
§14, all fixtures now applicable except any still explicitly out of
M9B-A's scope — hooks/SSA/CSA are all in scope by this point, given
M9B-A.7/A.8 preceded this slice) as a dedicated live-cluster suite against
**both** a real `helm` v3.x and v4.x binary as oracles, plus full
M1-M9 regression, plus a soak.

**Scope**: `scripts/test-cluster-m9b-a.sh` (or equivalent, mirroring the
`test-cluster-m9.sh` naming convention) with its own guarded Docker/API
identity check, `m9b-a-fixtures`/`m9b-a-reset`/`m9b-a-test` cases; a
dedicated differential-test harness invoking both pinned `helm` binaries
(decided at M9B-A.0) against the same fixture set; full regression of
every existing `accept-*.py` script through M9 (or whatever M10+ has
added by the time this milestone is actually implemented — regression
scope is "everything accepted so far," not a fixed list frozen at
planning time); a soak (duration TBD, 75 minutes proposed as the
established convention from M8.6/M8B.7) rotating through guarded Helm
rollback/uninstall operations with the same self-restoring/no-drift
discipline every prior soak established, independent per-section error
handling from the start.

**Explicit non-goals**: no new engine capability — this slice proves
what M9B-A.1-A.10 already built, it does not add to it. If a gap is found
here, it is a bug (per this project's own bug-discipline convention:
reproduce, classify, root cause, regression-proof, full locked recheck,
replay, only then continue), not a scope addition.

**Architecture/invariants**: none new; this slice validates every
invariant in this document's own Invariants section, end to end.

**Security constraints**: reconfirms that no journal entry, log line, or
error message produced across the full fixture matrix ever contains a
raw release payload, value, manifest body, or note — a dedicated
grep/audit pass across the full soak's own captured output, not just
per-slice unit tests.

**Mutation-gateway requirements**: reconfirms zero second-gateway
behavior across the full fixture matrix.

**Exact stop-and-ask decisions**: whether any fixture requiring a feature
not yet built by this point is possible at all (it should not be, since
M9B-A.7/A.8 precede this slice) — if one is found, stop, because it
indicates a scope gap earlier in this ledger, not something to patch at
acceptance time.

**Unit / fake-HTTP tests**: none new; this slice is live/differential/
regression, reusing every unit/fake-HTTP test already accumulated.

**Live tests**: the full fixture matrix (research §14), run twice per
this project's own "run combined acceptance twice" convention (M9.7,
M8B.7).

**Differential Helm-oracle tests**: every fixture in research §14
(excluding any this milestone deliberately never scoped — e.g. non-Secret
storage drivers, permanently out of scope) against both pinned `helm`
v3.x and v4.x binaries — research MH8's own stop condition, verbatim:
"Every fixture passes for both oracle versions; any fixture requiring a
feature not yet built is an explicit, named refusal, never a silent
pass."

**Failure-injection tests**: the crash-mid-write experiment from
M9B-A.5's stop-and-ask decision, repeated at full-system scope (not just
the isolated apply layer) if not already fully closed.

**Acceptance criteria**: MH8's own stop condition (verbatim, above); full
M1-M9 (or current) regression green; two clean combined-acceptance runs;
soak completed with the same honesty discipline ("observed stability
only, no leak-freedom claims") every prior soak used; clean working tree;
`fmt`/`clippy -D warnings` clean across the whole workspace including
`crates/helm-engine`.

**Documentation updates**: `HANDBOOK.md`, `docs/RUNBOOK.md`, `README.md`,
this document — reconciled, per every prior milestone's own final-slice
convention.

**Commit/tag expectations**: local annotated tag `m9b-a-accepted`, never
pushed without explicit authorization — mirroring `m9-accepted`'s own
convention exactly.

## Compatibility contract (restated from the brief's own required section)

Differential testing against real Helm is a first-class acceptance
requirement threaded through **every** slice above from M9B-A.2 onward,
never a final smoke test bolted on at M9B-A.11. For equivalent starting
fixtures, Path A (real Helm performs the operation) and Path B (SAUR-ON's
native engine performs the operation) are compared on, at minimum:
resulting Kubernetes resources (full spec diff, not presence/absence);
labels/annotations; `managedFields` (manager name and operation type,
`Apply` vs. `Update`); Apply-vs-Update semantics; release revision;
release status; full release history; stored release metadata (verified
by decoding the engine's output with the *real* `helm get` commands, not
a self-decode-only check); hook execution/resources/order; resource-policy
behavior; delete behavior; failure/partial-state behavior; and, as the
**critical follow-up gating every fixture**, whether the real `helm` CLI
can still successfully operate on the release afterward (`helm status`/
`helm history`/`helm rollback`/`helm upgrade`/`helm uninstall`, used in
the test harness only, never at runtime).

**CSA-gating fixtures**: research §14 fixtures #1-6, #11-14, #16-17, #21
gate CSA compatibility — every one of these must pass through the
CSA-only path (M9B-A.5/A.6) before that path is considered acceptance-
ready, independent of whether SSA (M9B-A.8) has landed yet.

**SSA-gating fixtures**: research §14 fixture #22 (a release originally
installed/upgraded via Helm v4/SSA-default, rolled back by the native
engine, then successfully re-`upgrade`d by real Helm) is the sole fixture
that specifically gates SSA compatibility — it cannot pass until M9B-A.8
exists, and its passing is that slice's own acceptance bar, restated here
so both documents agree on which fixture proves which regime.

**Hook-gating fixtures**: research §14 fixtures #7, #8, #9 gate hook
compatibility, proven at M9B-A.7, independent of the CSA/SSA split above
(a hook-bearing release may be either CSA- or SSA-managed; the hook
engine itself does not care which apply regime is in effect once M9B-A.8
exists — until then, hook fixtures are only exercised against CSA-managed
releases).

## Live-cluster strategy

Mirrors `docs/M9_ACCEPTANCE.md`'s own resolved live-test strategy
(dedicated, disposable cluster, never the primary regression cluster).
**Decision deferred to M9B-A.0, not guessed here**: whether M9B-A reuses
the existing `sauron-m9` cluster (Helm needs no controller, per M9's own
finding that Helm's live evidence used the existing `kind-sauron-test`
cluster rather than the controller-hosting `sauron-m9`) or stands up its
own dedicated `sauron-m9b-a` cluster, given this milestone's own write
operations (unlike M9.5's read-only ones) and its destructive
crash-mid-write failure-injection experiments (M9B-A.3/A.5) may warrant
isolation from both the primary regression cluster and any controller
workload. Whichever is decided, the same non-heuristic, externally-proven
identity-guard-script convention (`test-cluster-m9.sh`'s own three-part
check: kubeconfig existence, Docker label match, API-server-URL match)
must be replicated for this milestone's own script, as a sibling script,
never a shared/parameterized one (mirroring M9's own explicit rationale
for keeping `test-cluster.sh` and `test-cluster-m9.sh` independent).

Both pinned `helm` CLI binaries (v3.x and v4.x, decided at M9B-A.0) must
be available wherever live/differential tests run — recorded here as an
ongoing maintenance cost (research §19.6), not a one-time setup detail.

## Security model (restated, consolidated)

Builds directly on M9.5's already-accepted model, widened by exactly one
narrow write primitive (`kube::helm::write_release`, M9B-A.2) and one new,
separately-reviewed decision (hook-log-capture, M9B-A.7). Every other
M9.5 guarantee — unconditional `Object::new` Secret redaction, no generic
raw-Secret helper, `pub(crate)`-only decode path, bounded gunzip/base64,
Notes permanently excluded, redacted values by default — is inherited
unchanged, never relaxed, restated verbatim rather than re-derived at
implementation time.

## Documentation obligations

`HANDBOOK.md`, `docs/RUNBOOK.md`, `README.md`, and this document itself
are reconciled only at M9B-A.11 (this milestone's final slice), mirroring
every prior milestone's own convention of updating shared documentation
once at combined acceptance rather than incrementally per slice — except
where a slice's own contract above explicitly calls out an earlier
documentation update (e.g. M9B-A.7's hook-log-capture security
write-up, which must exist in this document's Journal at that slice, not
deferred).

## Architectural questions that MUST be resolved before implementation

Populated from `docs/NATIVE_HELM_ENGINE_RESEARCH.md`'s §20 explicit
unknowns list, plus the field-manager/retry decisions already made above
(restated here as closed, for completeness). None of these are filled in
with guesses anywhere in this document.

1. **`strategicpatch.CreateThreeWayMergePatch`'s own algorithm** (research
   §20 item 1) — not yet read from `k8s.io/apimachinery` source (only
   *that* Helm calls it and *with what inputs* is confirmed). **Blocks**
   M9B-A.5 from starting. **Resolved by**: a dedicated source study of
   `k8s.io/apimachinery/pkg/util/strategicpatch`, recorded in this
   document's Journal before M9B-A.5's apply logic is written.
2. **HIP-0023's own proposal text** (research §20 item 2) — only its
   implemented consequence (v4.3.0 source) was confirmed, not the
   proposal's own stated rationale. **Blocks** M9B-A.8 only if a design
   choice there needs to be justified against Helm maintainers' own
   stated reasoning, not merely against observed behavior. **Resolved
   by**: fetching and reading the HIP-0023 text directly, only if M9B-A.8
   implementation surfaces a case where behavioral evidence alone is
   insufficient to decide.
3. **Readiness semantics from Helm's `pkg/kube/wait.go`/`ready.go`** —
   only `Job`/`Pod` readiness (`watchUntilReady`) was confirmed; the newer
   `kube.Wait`/`WaitWithJobs` interfaces referenced in `rollback.go` were
   fetched as files but not read line-by-line (research §20 item 3).
   **Blocks** any slice implementing `Wait`-gated behavior for rollback/
   uninstall (M9B-A.6, since both actions' call graphs include an
   optional `Wait`/`WaitForDelete` step). **Resolved by**: a full read of
   both files before M9B-A.6's `Wait`-handling code is written, not
   assumed simple.
4. **Exact CSA→SSA managedFields migration behavior**
   (`k8s.io/client-go/util/csaupgrade`'s `UpgradeManagedFields`, research
   §20 item 5) — cited from the v4 source comment, not read. **Blocks**
   M9B-A.8 specifically (this is the literal mechanism that slice must
   either reproduce or deliberately reject). **Resolved by**: reading the
   referenced package in full before M9B-A.8's regime-detection design is
   finalized — this document's own M9B-A.8 contract already states this
   as a hard stop-and-ask gate.
5. **ConfigMap storage-driver relevance** (research §20 item 6) — not
   experimentally verified; low priority given this milestone's own
   Secret-only non-goal. **Does not block** any current slice (no
   ConfigMap-driver work is scoped). **Resolved by**: nothing planned;
   revisit only if a future cluster is found using the ConfigMap driver
   in practice, which would itself be new evidence this document does not
   currently have.
6. **Crash-mid-write experiment design** (research §20 item 7) — the §10
   failure table is source-derived, not independently reproduced by
   actually killing a process mid-write. **Blocks** M9B-A.6's own
   failure-state claims from being fully trusted (M9B-A.3 attempts a
   partial closure at the pure state-machine layer; M9B-A.5's own
   stop-and-ask decision calls for the full cluster-facing experiment).
   **Resolved by**: the explicit, sign-off-gated experiment design
   proposed at M9B-A.5 (`kill -9` the process mid-commit against a
   throwaway namespace), executed before M9B-A.6's failure-injection tests
   are considered conclusive.
7. **Exact `--dry-run=server` mechanics inside `pkg/kube/client.go`**
   (research §40 item 1, carried forward since M9B-A.9 touches
   diff-preview) — confirmed that the real `KubeClient` and
   `IsReachable()` are used and the render→build→apply path is identical
   to a real install/upgrade, but not confirmed exactly how a `metav1`-
   style dry-run parameter attaches to the request. **Blocks** M9B-A.9
   only if its diff-preview implementation needs to assert byte-level
   equivalence to Helm's own dry-run wire behavior rather than merely
   reusing `kube-rs`'s own existing dry-run parameter (which this
   document's own M7-reuse discipline already prefers). **Resolved by**:
   a direct wire-level check only if M9B-A.9 surfaces a concrete
   discrepancy; otherwise not required, since M9B-A.9 reuses `kube-rs`'s
   own mechanism rather than porting Helm's.
8. **Exact OCI tag normalization logic / real-world `oci-client`
   adoption / `helm2oci` relevance** (research §20 items 2/3/4 of the
   extension pass, §40 items 2-4) — **does not block any M9B-A slice**;
   these are exclusively M9B-B concerns (OCI pull/push), listed here only
   because the brief's own required-unknowns list names them explicitly.
   Restated in `docs/M9B_B_ACCEPTANCE.md`'s own Architectural questions
   section as the document that actually owns them.
9. **Exact rollback default-description string** (research §40 item 5) —
   not independently re-verified byte-for-byte; relevant only to a
   "history" presentation layer trying to show rollback-specific
   description text verbatim. **Blocks** nothing load-bearing (M9B-A.9's
   own contract already recommends matching Helm's flat list without a
   derived column, sidestepping the need for this string entirely) —
   **does block** only if a future slice decides to surface Helm's exact
   description text verbatim rather than SAUR-ON's own wording.
   **Resolved by**: a direct read of `rollback.go`'s exact string
   construction, only if that future decision is made.

## Bugs / limitations (placeholder)

None yet — implementation has not started. This section exists so future
implementation work has a designated place to record this project's own
established "bug discipline": reproduce, classify (app/harness/fixture/
environment), root cause, regression-proof, full locked recheck, replay,
only then continue. Nothing here is retroactively filled in from any
prior milestone; each milestone's own acceptance document remains the
authoritative record of its own bugs.

## Journal

- 2026-09-21: This document created as a planning/acceptance ledger,
  converting `docs/NATIVE_HELM_ENGINE_RESEARCH.md`'s rollback/uninstall
  findings (§1-21, MH0-MH9) into an M9B-A slice ledger per explicit
  request. Status: **PLANNED — NOT STARTED**. No code touched. No
  cluster touched. No commit made as part of authoring this document
  beyond what the requesting instruction explicitly permitted. M10 is not
  blocked by this document's existence and may proceed independently.

## Final milestone acceptance checklist

To be completed only when M9B-A implementation actually finishes — listed
here now as the target bar, not as a claim of current status:

- [ ] M9B-A.0-A.11 all ACCEPTED, each with its own recorded evidence in
  this document's Journal (unit/fake-HTTP/live/differential counts,
  mirroring every prior milestone's own journal-entry style).
- [ ] Full research §14 differential fixture matrix passes against both
  a pinned `helm` v3.x and v4.x oracle, with any deliberately-out-of-scope
  fixture (if any remain) refused explicitly, never silently skipped.
- [ ] The "subsequent real `helm` CLI compatibility" critical test passes
  for every fixture, not just a final smoke subset.
- [ ] Every documented divergence from Helm upstream (no-hidden-retry,
  stronger TOCTOU, pending-state refusal) is proven by a dedicated test,
  not merely asserted in prose.
- [ ] Full M1-M9 (or current) regression green, run via every existing
  `accept-*.py` script unmodified.
- [ ] Combined acceptance run twice, clean.
- [ ] A soak completed with the same "observed stability only" honesty
  discipline every prior soak used.
- [ ] `cargo fmt --check` / `cargo clippy --all-targets -D warnings`
  clean across the whole workspace, including `crates/helm-engine`.
- [ ] `HANDBOOK.md`, `docs/RUNBOOK.md`, `README.md`, this document
  reconciled.
- [ ] Working tree clean.
- [ ] Local annotated tag `m9b-a-accepted` created, never pushed without
  explicit authorization.
- [ ] `docs/M9B_B_ACCEPTANCE.md` confirmed still consistent with whatever
  M9B-A actually shipped (its own primitives may have been refined during
  implementation in ways this planning-time document could not predict)
  before M9B-B implementation begins.
