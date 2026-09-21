# M9B-B — Chart Execution / Native Helm Runtime: planning ledger

Status: **PLANNED — NOT STARTED.** This document is a future,
ready-to-execute milestone contract, written so implementation can begin
months later without reconstructing the architecture from chat history.
It converts `docs/NATIVE_HELM_ENGINE_RESEARCH.md`'s extension pass
(§22-40, MH10-MH18) into a slice-by-slice acceptance ledger, following the
exact document conventions `docs/M8B_ACCEPTANCE.md`, `docs/M9_ACCEPTANCE.md`,
and `docs/M9B_A_ACCEPTANCE.md` established: scope definition and contract
design first, updated slice by slice only once implementation actually
starts. **Nothing in this file authorizes touching code, cluster, or CI.**
M10 may begin, and is expected to begin, before either M9B milestone
starts — this document's existence changes nothing about that. The
existence of this document is not authorization to start M9B-B.0 or any
other slice below — that requires separate, explicit approval, exactly as
`docs/M9B_A_ACCEPTANCE.md`'s own closing line, and the research document's
own closing lines (§21, §39), state.

## Purpose

`docs/M9B_A_ACCEPTANCE.md` (PLANNED — NOT STARTED) scoped the native Rust
engine needed to *operate* existing Helm releases — rollback, uninstall,
and the storage/lifecycle/apply/hook primitives those two actions require
— without a chart, a template engine, or a repo/OCI client, because an
existing release already carries its own rendered manifest and state
(research §17, reaffirmed §22). **M9B-B's purpose** is the milestone M9B-A
explicitly declined to be: extending that proven operational engine into
*chart execution* — values handling, chart loading, chart rendering
(Go-template + Sprig compatibility), `template`, `install`, `upgrade`,
CRD-install-first semantics, and OCI pull/push where the research
recommends it. This is the point at which SAUR-ON crosses from "operate
existing Helm releases" into "native Helm runtime in Rust" (per the
brief's own framing, restated verbatim as this document's purpose
statement).

Scope follows the research document's own three-tier recommendation
(§37, §39), not a fresh judgment call made in this document:

- **Build natively** where required for semantic compatibility: chart
  archive/loading model, values coalescing, template execution
  (Go-template + Sprig compatibility), chart object model, rendered
  manifest assembly, install lifecycle, upgrade lifecycle, CRD-install-
  first behavior, and classic-repo provenance/PGP verification (§37
  Tier 1).
- **Build via an existing crate** where the research found a mature,
  actively-maintained one: OCI Distribution transport via `oci-client`
  (`oras-project/rust-oci-client`) — reimplementing OCI Distribution from
  scratch is explicitly rejected by the research (§28.2, §37 Tier 2)
  unless later evidence invalidates that choice, in which case that
  reversal must be recorded here with its own evidence, never silently
  substituted.
- **Explicitly out of scope**, per the research's own Tier 3
  classification, because they are chart-author/publisher-persona
  concerns outside SAUR-ON's operations-console mission, not
  not-yet-scheduled work: `helm repo` management (add/update/index),
  dependency/`Chart.lock` management, the plugin system, `helm lint`
  (§37 Tier 3, §39 "WHAT WE SHOULD NOT IMPLEMENT").

This document does **not** silently expand M9B-B into "clone all Helm
commands." Every item in scope below traces to a specific research
section; every item excluded traces to the same research's own Tier 3
verdict or to M9B-A's already-settled boundary — nothing here is invented
fresh.

## Relationship to prior milestones

M9B-B reuses, unmodified in spirit, everything M7/M8/M8B/M9/M9B-A already
built, restated per this project's own established "Relationship to
M7/M8" convention (mirroring `docs/M9B_A_ACCEPTANCE.md`'s own equivalent
section, extended here for the primitives M9B-B specifically reuses from
M9B-A):

- `mutation::{MutationIntent, MutationTarget, MutationEffect, MutationRisk,
  PolicyReason, PolicyDecision, PolicyEvaluation, ConfirmationRequirement,
  Confirmation, MutationOutcome, Verification}` — the pure model, unchanged.
  M9B-B adds new `source_action` values (`helm_install`, `helm_upgrade`,
  `helm_template` where it produces a preview-only `Built`) and, only
  where genuinely needed, new `PolicyReason`/`MutationOutcome`/
  `Verification` variants this model does not already express — it does
  not fork the model, exactly as M9B-A's own restated rule.
- `mutation::policy::evaluate` — the single policy engine, unchanged;
  install/upgrade are new *inputs*, never a parallel decision path.
- `mutation::workflow::{Built, Workflow}` — the same shared shell,
  composed the same way M9B-A's rollback/uninstall composed it, following
  the Drain/M8B.5 documented-exception precedent for genuinely composite
  actions (install/upgrade are more composite than rollback/uninstall —
  render, then diff, then apply, then hooks — and this slice ledger's own
  per-slice contracts decide, explicitly, whether that composite shape
  needs its own ordered-step reporting convention or fits `Built` as-is).
- `kube::mutation::{preflight, commit, verify}` — the single execution
  gateway, unchanged. M9B-B's apply calls are new *executor branches*
  under this gateway (mirroring M9B-A.5/A.8's own CSA/SSA branches),
  never a second gateway, second TOCTOU mechanism, or second verification
  dispatcher.
- `mutation::journal::{Journal, Phase, Record}` — the same append-only,
  redacted, bounded journal. Install/upgrade's journal entries record
  intent (release name/namespace, chart reference, target revision) and
  outcome, never rendered manifest bodies, resolved values, or chart
  contents — restated and *extended* from M9B-A.6's own values/manifest
  redaction decision, now covering chart-execution-specific payloads
  (rendered templates, coalesced values, OCI blob contents) that M9B-A
  never had to consider because it never rendered anything.
- `Document.workflow`-style dedicated fields, the `"mutation"` keymap
  mode, and `mutation::view::workflow_report` — the same shared UI shell,
  extended only where an install/upgrade action has more than one
  meaningful step or a genuinely different verification shape.
- `Command::{...}` + `parse()` grammar + `command_names()` — the same
  single command registry; `:helm_install`/`:helm_upgrade`/`:helm_template`
  (exact grammar TBD at M9B-B.7/B.8's own contract) are discoverable the
  same way every other guarded command is.
- **From M9B-A directly** (the load-bearing reuse this document is
  required to state explicitly, per "Relationship between M9B-A and
  M9B-B" below): `crates/helm-engine`'s `storage` module (release
  read/write, M9B-A.2) for persisting install/upgrade-created revisions;
  `lifecycle` (status machine, revision allocation, history/supersede/
  prune, M9B-A.3) for install's revision-1 creation and upgrade's
  `current.Version + 1` allocation; `manifest` (split/sort/resource-policy,
  M9B-A.4) for ordering install/upgrade's freshly-rendered manifest the
  same way rollback orders a stored one; `apply` (CSA/SSA reconciliation,
  M9B-A.5/A.8) verbatim for install's create path and upgrade's
  create/patch/delete diffing, per research §22-23's own "identical call
  shape" finding; `hooks::engine` (M9B-A.7) verbatim for
  pre/post-install/upgrade events, which M9B-A.7's own contract already
  built "opportunistically... for M9B-B's later benefit" — M9B-B consumes
  that opportunistic work rather than rebuilding it; the differential-
  testing harness pattern itself (pinned `helm` v3.x/v4.x oracles, the
  `scripts/test-cluster-m9b-a.sh`-style guarded identity-check convention)
  reused as a sibling script, never shared/parameterized code, mirroring
  M9B-A's own stated rationale for keeping `test-cluster.sh` and
  `test-cluster-m9.sh` independent.
- `src/integrations/helm.rs` / `src/kube/helm.rs` (M9.5) — reused
  unchanged for the read side, exactly as M9B-A already reuses it; M9B-B
  adds no new read primitive here (its own new storage writes are M9B-A's
  `write_release`, called with a chart-execution-produced record, not a
  new write primitive).

### Design test (carried over from M8/M8B/M9/M9B-A's own ledgers, unchanged)

At the end of M9B-B, adding a further guarded install/upgrade refinement
should still look like: 1) parse the semantic user action, 2) validate
its specific arguments (chart reference, values overrides) against the
engine's own chart-loading/rendering/compatibility layer, 3) build a
`MutationIntent` (or an explicit, documented composite sequence,
mirroring Drain's own exception, now stretched further than M9B-A's own
rollback/uninstall composite by the render→diff→apply→hook pipeline), 4)
provide a semantic preview renderer (including a rendered-manifest
diff-preview where research §33 makes it free), 5) hand it to the
existing M7 infrastructure. It should NOT require a new confirmation
subsystem, a new policy subsystem, a new journal, a new transport safety
model, a new UID model, a new TOCTOU logic, or a second gateway, and it
should NOT require shelling out to `helm`. If any M9B-B action needs one
of those, stop and reconsider before implementing — the design is wrong,
exactly per M9B-A's own restated design test.

## Invariants (carried forward, restated explicitly for this milestone)

Every M9B-A invariant applies unchanged to M9B-B (UID ≠ NAME, COMMIT ≠
OBSERVED EFFECT, LOOKS EQUIVALENT ≠ BEHAVES EQUIVALENT, no runtime
shell-out, no direct release-Secret mutation shortcut, no second
policy/confirmation/journal/verification system, no hidden automatic
mutation retry, production remains read-only, real Helm CLI is test
oracle only, raw Helm Secret contents never enter Object/Store/
Timeline/graph/journal, no logs/panic/errors leaking raw payload content,
exact revision identity revalidated at commit, `pending-*` fails closed,
unsupported semantics reject explicitly never silently degrade) — restated
here by reference, not copy-pasted a second time, to avoid this document
and M9B-A's drifting out of sync on wording. Any M9B-B slice that needs a
narrower, deliberate divergence from one of these follows the same
"documented, not accidental" discipline M9B-A established (see
"Deliberate divergences" below).

M9B-B adds the following **new** invariants, specific to chart execution,
none of which M9B-A needed:

- **RENDER DETERMINISM.** For a fixed chart, fixed values, fixed
  `Capabilities`/`Release` context, and fixed Kubernetes version, the
  renderer produces byte-identical output across repeated invocations.
  Non-determinism (map key ordering, floating timestamp injection, `now`-
  style Sprig functions used without an explicit, documented exception)
  is a bug, not an accepted characteristic — this is the property the
  entire differential-fixture strategy below depends on being true.
- **RENDERING GATE.** Native `install`/`upgrade` may not be unlocked
  until the renderer compatibility gate (defined below) passes against
  its own acceptance bar. A renderer that "mostly works" is not a
  renderer the engine is permitted to install/upgrade with — it is
  refused, not silently shipped with caveats. This is the concrete
  instantiation of the brief's own instruction: "do not unlock native
  install until the renderer acceptance gate passes."
- **VALUES NEVER SUBSTITUTE FOR MANIFEST.** Install/upgrade's diffing and
  apply logic operate on the *rendered manifest*, never on raw values —
  mirroring M9B-A.4's own "resource-policy read from the stored manifest,
  not the live object" discipline, restated here for the render boundary
  specifically: a values-coalescing bug must never be silently masked by
  the apply layer "doing something reasonable" with malformed input; it
  must surface as an explicit render-time error.
- **NO SILENT CHART-ACQUISITION FALLBACK.** If a chart reference cannot
  be resolved through an in-scope path (local, OCI if M9B-B.9/B.10 land,
  classic-repo if M9B-B.11 lands), the engine refuses explicitly. It never
  falls back to shelling out to `helm pull`/`helm repo` or to a
  best-effort guess at chart contents.
- **OCI TRANSPORT VIA `oci-client`, NEVER HAND-ROLLED.** Per research
  §28.2/§37 Tier 2: OCI Distribution protocol mechanics are not
  reimplemented from scratch. If a future slice finds a concrete gap in
  `oci-client` that cannot be worked around, that is a stop-and-ask
  decision recorded in this document's Journal, not a silent decision to
  hand-roll the missing piece.
- **CRD-INSTALL-FIRST IS PERMANENT, HOOK-DELETE NEVER TOUCHES CRDS.**
  Restated from research §22: CRDs installed via the CRD-first path are
  never deleted by SAUR-ON (matching Helm's own documented behavior —
  permanent unless a human deletes them), and hook-delete-policy
  hard-codes `CustomResourceDefinition` as never-deletable regardless of
  a hook's own declared policy — the same carve-out M9B-A.7 already
  proved for rollback/uninstall's hook engine, restated here because
  install/upgrade is the path that actually creates CRDs in the first
  place.

### Deliberate divergences from Helm upstream (documented, not accidental)

M9B-B inherits M9B-A's three documented divergences (no hidden 409/
Conflict retry; stronger TOCTOU/UID checks than Helm's own storage layer;
refusal on an already-`pending-*` release state) unchanged — install and
upgrade are new *callers* of the same apply/storage layers, not a reason
to relax any of the three. **No new divergence is introduced by this
document.** Any future M9B-B slice proposing one must document it here,
in the same format M9B-A established (what upstream does, what SAUR-ON
does instead, why), before implementing it — never silently. Candidate
areas a future slice might propose a divergence for (not pre-approved,
listed only so a future author knows where to look): render-time
non-determinism handling (Helm itself has none it needs to handle, since
it always re-renders fresh — SAUR-ON's render-determinism invariant above
is stricter by construction, not a Helm-upstream divergence at all, since
there is no upstream behavior to diverge from here); OCI auth-flow
conflict resolution if `oci-client`'s behavior differs observably from
`oras-go`'s (research §28.1's own flagged unverified-adoption risk).

## Safety boundary

Production remains **READ-ONLY**, unchanged from every prior milestone.
Every M9B-B write action — install, upgrade, and any intermediate
engine-internal write (release storage create/update, CRD creation, OCI
push if M9B-B.10 lands) — is strictly forbidden against production. All
M9B-B live writes use only an isolated, explicitly verified test cluster
via the existing `mutation_test_cluster_verified` flag and its
guard-script convention (see Live-cluster strategy below), never inferred
from context name. Never push/publish anything (including an OCI chart
push, M9B-B.10) without explicit authorization — an OCI push is treated
with the same seriousness as a cluster mutation, not as "just an
artifact upload." The real `helm` CLI, and the real chart corpus used for
differential rendering (Bitnami, ingress-nginx, cert-manager,
prometheus-community, per research MH11's own stated corpus), are pure
test-harness tooling — exactly like M9.5's `demo-release` and M9B-A's own
disposable-release fixtures — never a runtime dependency of the shipped
binary.

## Explicit non-goals for M9B-B

Per the research document's own Tier 3 classification (§37, §39
"WHAT WE SHOULD NOT IMPLEMENT"), restated here as binding, not merely
informative:

- **`helm repo` management** (add/update/index, research §30) — a
  chart-discovery/publishing-persona concern; no in-scope M9B-B action
  needs it (M9B-B assumes charts are already-fetched/local, or fetched
  via the narrow OCI/classic-repo-pull paths M9B-B.9/B.11 scope, never
  via repo bookkeeping).
- **Dependency / `Chart.lock` management** (`helm dependency
  update/build`, research §31) — chart-authoring/packaging concern;
  by the time a chart is installed its dependencies are assumed already
  flattened/vendored. M9B-B's chart-loading slice (M9B-B.1) reads an
  already-resolved `charts/` subchart tree; it does not resolve one.
- **The plugin system** (research §32) — structurally incompatible with
  a native, no-shell-out Rust engine.
- **`helm lint`** (research §36) — chart-authoring-time validation,
  irrelevant to operating an already-installed release; its one
  operationally-relevant cousin, `--dry-run=server`, is covered for free
  by the apply layer once install/upgrade exist, not a separate
  deliverable.
- **A generic "Helm client" surface.** Exactly like M9B-A's own
  equivalent non-goal: the engine exposes exactly
  `template`/`install`/`upgrade` (and, if M9B-B.9-B.11 land, chart
  pull/push) actions returning `mutation::workflow`-shaped `Built`s or
  render-only previews — never a general-purpose read/write Helm library
  surface any other caller could reach for.
- **`--reset-values`/`--reuse-values`/`--reset-then-reuse-values`'s
  exact byte-for-byte precedence semantics as a *separate* deliverable
  from M9B-B.8.** They are explicitly *in scope*, but scoped entirely
  inside M9B-B.8 (native upgrade) — not a reason to expand this
  document's slice count, called out here only so a reader does not
  mistake their absence from the slice ledger's own headline names as an
  omission.
- **`helm test`, `helm history`, `get` subcommands' remaining gaps,
  rollback/uninstall-case diff-preview.** All already scoped, or
  explicitly and permanently declined, inside `docs/M9B_A_ACCEPTANCE.md`
  (M9B-A.9) — restated here only to make explicit that M9B-B does not
  re-scope or duplicate them. `helm test` in particular was a candidate
  for either document per the research's own §29 finding; M9B-A.9's own
  contract already resolved that ambiguity in M9B-A's favor (it depends
  only on M9B-A.7's hooks, nothing from M9B-B) — this document does not
  revisit that decision.
- **`helm diff`'s upgrade/install-against-a-new-chart-version case as a
  distinct deliverable.** It is a free byproduct of M9B-B.3/B.4
  (renderer) plus the M9B-A apply layer (research §33) — not a separate
  slice, folded into M9B-B.8's own diff-preview scope where the contract
  below calls it out explicitly.
- **Bulk operations** (multi-release install/upgrade). Belongs to M10 or
  a later bulk-operations extension, never folded into M9B-B — every
  M9B-B action targets exactly one release per intent, mirroring M9B-A's
  own equivalent non-goal.
- **Plugin/headless/scripted mutation execution.** Belongs to M12,
  unchanged from every prior milestone's own non-goal list.
- **`kubectl`/`helm` CLI passthrough of any kind at runtime.** Restated
  as an explicit non-goal, not merely an invariant.
- **Publishing `crates/helm-engine` (or a new `crates/helm-render`/
  `crates/helm-oci` split, if one is decided at implementation time) as a
  standalone crate.** Stays internal-workspace-only until the full
  differential fixture matrix passes, mirroring M9B-A's own equivalent
  non-goal.

## Relationship between M9B-A and M9B-B

**M9B-B depends on accepted primitives from M9B-A. M9B-A must NOT depend
on M9B-B.** This is the single most important structural decision in
both documents, and this document does not weaken it in either
direction.

Concretely, restated from `docs/M9B_A_ACCEPTANCE.md`'s own "Relationship
between M9B-A and M9B-B" section, now from M9B-B's own side of the
boundary: rollback/uninstall must never require chart rendering, chart
fetching, Go template, Sprig, repo handling, or OCI — existing Helm
releases already contain the rendered manifests and release state those
two actions need (research §17, reaffirmed §22: "a native
rollback/uninstall engine never needs a Helm template engine at all").
If a future M9B-B implementer discovers that some M9B-A primitive is
missing a capability M9B-B needs, the correct response is to **extend
M9B-A's own primitive** (e.g. `storage::record`/`write_release` gaining a
field or code path M9B-A itself never needed but which is still a
storage-layer concern, not a rendering-layer one) and get that change
accepted under M9B-A's own acceptance discipline, never to have M9B-B
silently reach around M9B-A's boundary or duplicate a primitive M9B-A
already owns.

**What M9B-B DOES reuse/depend on from M9B-A** (the reverse direction,
expected and correct, per research §23's own "precise reuse breakdown"
table and restated in the "Relationship to prior milestones" section
above): `storage` (release read/write), `lifecycle` (status machine,
revision allocation, history/supersede/prune), `manifest` (split/sort/
resource-policy-filter, applied here to a freshly-rendered manifest
instead of a stored one), `apply` (CSA three-way-merge-equivalent and
SSA reconciliation — install's create path and upgrade's create/patch/
delete diffing route through the *exact same* `KubeClient.Update`-
equivalent primitive rollback/uninstall already use, per research §22-23's
own literal "IDENTICAL CALL SHAPE" finding), `hooks::engine` (pre/post-
install/upgrade events, which M9B-A.7 built with M9B-B's later benefit
already in mind, per that slice's own stated scope), and the
differential-testing harness pattern itself (pinned oracle binaries,
guarded-cluster-identity convention, fixture-matrix discipline). None of
these primitives is forked or duplicated inside `crates/helm-engine`'s
new `chart`/`render`/`oci` modules (research §39's own "RECOMMENDED
ENGINE BOUNDARY" restated) — those new modules produce inputs (a rendered
manifest, a resolved chart) that flow *into* the existing `manifest`/
`apply`/`lifecycle`/`storage`/`hooks` modules, never around them.

If, during implementation, any M9B-B slice is found to need something
that would require M9B-A to depend on M9B-B to provide it, that is a
design defect in this document to be corrected before implementation
proceeds — never grounds for silently inverting the dependency direction.

## The renderer as a major compatibility subsystem

The brief is explicit, and this document restates it as binding: **the
template renderer is not a thin parser.** Research §25 is unambiguous
that no existing Rust crate offers real Helm-compatible Go-template +
Sprig behavior (§25.3-25.4), that the honest effort estimate for genuine
behavioral compatibility on real-world charts is **4-6 months of focused
engineering** (§25.5), and that "the last 20%" — whitespace-trim edge
cases, dot-scoping, `include`/`tpl` late-binding, subchart values-
cascading, crypto/regex edge functions — is exactly the part every known
prior-art Rust project (`gtmpl-rust`, stale since 2022; `lithos-sprig`,
v0.1.0 with explicit scope cuts) failed to finish, not merely failed to
start (§25.6, §39 "BIGGEST NEW TECHNICAL RISK"). M9B-B.3/B.4 below treat
this as its own deliberately-scoped, separately-reviewed 4-6-month
subsystem, not a rounding error against the rest of this milestone.

### What the renderer acceptance plan must define (per slice, detailed below)

- **Parser/evaluator compatibility**: a Go-template-syntax-compatible
  lexer/parser/executor (`{{ }}` delimiters, `{{- -}}` whitespace-trim,
  `if`/`else if`/`else`/`range`/`with`/`define`/`template`/`block`
  control structures, pipeline chaining where "the result of each command
  is passed as the last argument of the following command" [DOCS:
  pkg.go.dev/text/template]) — M9B-B.3.
- **Dot/context semantics**: `.`'s exact rebinding rules under `with`/
  `range`, `$`-rooted access to the original top-level context from
  inside a nested scope, dynamic struct-field/map-key/method-style
  dot-access over an arbitrary value tree (the real gap every prior-art
  crate examined by the research left unresolved, per §25.4's own
  `gotmpl` finding) — M9B-B.3.
- **Pipelines and functions**: multi-stage pipelines, variadic/error-
  returning function calls, the `must*`-family error-vs-panic convention
  Sprig itself established — M9B-B.3/B.4.
- **Sprig coverage**: a categorized compatibility matrix (date/time,
  string, math, list, dict, JSON, encoding, crypto, regex, semver,
  reflection, UUID, URL — per research §25.2's own ~13-category, ~100-170-
  function inventory), with every function classified as
  implemented-and-tested / implemented-with-documented-divergence /
  explicitly-not-implemented-and-refused — M9B-B.4.
- **Helm-added template functions**: `toToml`/`fromToml`/`toYaml`/
  `fromYaml`/`toJson`/`fromJson`, `required`, `lookup` (a genuinely
  live-cluster-querying function — its own scoped decision on whether the
  engine's `lookup` queries the real test cluster, a faked capability
  set, or refuses, decided explicitly, not defaulted), with `env`/
  `expandenv` deliberately *excluded*, matching Helm's own deletion of
  them from Sprig's default set (research §25.1) — M9B-B.4.
- **`include`/`template`/`tpl` behavior**: recursive, self-referential
  invocation with late-bound closures over the *current* render context
  — research §25.1/§25.5's own explicitly named hardest-to-get-right
  piece — M9B-B.3.
- **`required`/`fail` behavior**: exact error-propagation and message
  shape parity with upstream's own `required`/`fail` template functions
  — M9B-B.3/B.4.
- **Values lookup**: `.Values` traversal over the coalesced value tree
  (M9B-B.2's output), including nested-map/array access patterns real
  charts use pervasively — M9B-B.3.
- **`Chart`/`Release`/`Capabilities` objects**: exact field shape parity
  with `chartutil.ToRenderValues`'s own assembled context (research
  §25.1) — `Chart` from `chrt.Metadata`; `Release` from
  `ReleaseOptions{Name, Namespace, IsUpgrade, IsInstall, Revision}` plus
  the hardcoded `Service: "Helm"` field (a literal string every chart
  author may template-match against — must be reproduced verbatim, not
  approximated); `Capabilities` from either a discovery-based real
  cluster read (mirroring upstream's `getCapabilities()`) or an explicit
  fixed/faked set for `template`'s client-only mode — M9B-B.3/B.5.
- **Kubernetes version/capability injection**: `Capabilities.KubeVersion`/
  `Capabilities.APIVersions` sourced from the target cluster's real
  discovery data for install/upgrade, and from an explicit, documented
  fixed value for client-only `template` — a scoped decision this
  document does not default silently — M9B-B.3/B.5.
- **Whitespace/chomping behavior where observable**: `{{-`/`-}}` trim
  semantics reproduced exactly, proven against fixtures specifically
  targeting leading/trailing-whitespace-sensitive YAML (a real correctness
  hazard, since YAML is indentation-sensitive) — M9B-B.3.
- **Error compatibility**: render-time errors (missing key under
  `required`, a `fail` call, a template parse error) surface with
  message shape close enough to real Helm's own that a user comparing
  the two is not confused about *what* failed, even where exact byte-for-
  byte error text is not guaranteed — M9B-B.3/B.4.
- **Deterministic rendering fixtures**: a hand-constructed fixture corpus
  covering every item above in isolation, checked into the repo,
  differentially compared against real `helm template` output — M9B-B.3/
  B.4/B.5.
- **Real-world chart corpus tests**: at minimum the research's own named
  corpus (Bitnami, ingress-nginx, cert-manager, prometheus-community,
  research MH11) rendered end-to-end and compared byte-for-byte (or with
  every divergence individually documented and justified) against real
  `helm template` output for the same chart+values — M9B-B.5's own
  acceptance gate.

### Renderer compatibility matrix and the install/upgrade unlock gate

M9B-B.0 defines, and M9B-B.3-B.5 populate, a versioned compatibility
matrix (its own artifact — `docs/M9B_B_RENDERER_MATRIX.md` or an
equivalent name decided at M9B-B.0, not guessed here) with one row per
Sprig function / Go-template control construct / Helm-added function /
object-model field, each marked:

- **Implemented, tested** — a passing differential fixture exists.
- **Implemented, documented divergence** — behavior differs from
  upstream in a specific, recorded way (e.g. a `regex` flavor difference
  between Go's RE2 and Rust's `regex` crate, per research §25.5's own
  flagged long tail), with a stated reason and a stated blast radius.
- **Not implemented, explicit refusal** — a chart using this construct
  is refused at render time with a named, specific error, never silently
  skipped or approximated.

**The install/upgrade unlock gate**: native `install`/`upgrade` (M9B-B.7/
B.8) may not begin implementation until this matrix reaches an
explicitly-defined "good enough" bar — the exact bar (percentage of the
research's ~100-170-function Sprig inventory covered; which specific
real-world corpus charts must render byte-identically; whether any
"not implemented, explicit refusal" rows are tolerated in the initial
gate or must be zero) is a **stop-and-ask decision at M9B-B.0**, not
assumed here. What is not negotiable, restated from the brief: **the gate
exists, and it gates**, whatever its exact threshold is decided to be —
"the renderer mostly works" is never sufifcient to unlock install/upgrade
on its own authority.

## Proposed slice ledger

Refined from the brief's suggested M9B-B.0-B.12 against the research
document's own MH10-MH18 dependency graph (§38) and its own explicit
sequencing answer ("`template` must exist before `install` can — install's
render step *is* `template`'s render step," §38). Ordering below mirrors
MH10→MH18 closely, with OCI (M9B-B.9/B.10) and provenance (M9B-B.11)
placed, per the brief's own suggested order, after install/upgrade rather
than research §38's MH15-16 slot — this is a deliberate deviation from
the research document's own milestone numbering, justified because
research §39 itself states OCI/pull-push are "independent of MH10-15"
(no dependency edge either direction), so their exact slot is a scope/
sequencing choice, not a correctness constraint; the brief's own ordering
is adopted here as the binding one for this document.

| Slice | Scope | Corresponds to (research) | Verdict |
| --- | --- | --- | --- |
| M9B-B.0 | Acceptance contract + renderer compatibility definition | MH0-equivalent freeze, §25's compatibility-matrix requirement | PLANNED — NOT STARTED |
| M9B-B.1 | Chart model / archive loading | MH10 (chart-loading half) | PLANNED — NOT STARTED |
| M9B-B.2 | Values coalescing | MH10 (values half) | PLANNED — NOT STARTED |
| M9B-B.3 | Template-engine compatibility core | MH11 (grammar/executor/dot-semantics/object-model) | PLANNED — NOT STARTED |
| M9B-B.4 | Sprig compatibility surface | MH11 (function library) | PLANNED — NOT STARTED |
| M9B-B.5 | Template command/path | MH12 | PLANNED — NOT STARTED |
| M9B-B.6 | CRD-first install semantics | MH13 (CRD-install-first half) | PLANNED — NOT STARTED |
| M9B-B.7 | Native install | MH13 (install action) | PLANNED — NOT STARTED |
| M9B-B.8 | Native upgrade | MH14 | PLANNED — NOT STARTED |
| M9B-B.9 | OCI pull integration | MH16 (pull half) | PLANNED — NOT STARTED |
| M9B-B.10 | OCI push integration if justified | MH16 (push half) | PLANNED — NOT STARTED |
| M9B-B.11 | Provenance / verification path | MH15 | PLANNED — NOT STARTED |
| M9B-B.12 | Full differential compatibility acceptance / regression / soak | MH18 (final case) + full-surface combined acceptance | PLANNED — NOT STARTED |

**Ordering rationale**: M9B-B.0 closes the renderer-compatibility-gate
threshold decision before any rendering code exists, mirroring M9B-A.0's
own "formalize before implementing" discipline. M9B-B.1-B.2 (chart
loading, values coalescing) must precede the renderer (M9B-B.3-B.4)
because the renderer's own `.Values`/`.Chart` context is built from their
output (research §25.1's own `chartutil.ToRenderValues` call graph) —
MH10 before MH11, unchanged from research's own dependency edge.
M9B-B.3-B.4 (grammar/executor, then Sprig) are split because the grammar/
executor is the harder, more architecturally load-bearing half (dot-
access, `include`/`tpl` late-binding) and Sprig's function library,
while large, is "mostly mechanical" per research §25.5 — sequencing the
hard architectural piece first means the mechanical piece is added
against a settled foundation, not the reverse. M9B-B.5 (`template`
command) is placed immediately after the renderer is proven in isolation,
matching research §38's own MH12 slot and its own stated reason: `helm
template` shares install's exact render call with cluster-touching side
effects switched off (research §24) — it is the renderer's own first
real consumer and the natural place to run the full real-world corpus
gate. M9B-B.6-B.7 (CRD-first, then install) come next because install's
own call graph (research §22) installs CRDs *before* rendering
(`installCRDs` runs before `getCapabilities()`), so the CRD-first special
case is logically prior even though it is a small, self-contained
addition compared to install's own remaining scope; install itself
cannot exist before the renderer (M9B-B.3-B.5) and M9B-A's SSA path
(M9B-A.8) both exist, per research §22's own dependency note. M9B-B.8
(upgrade) follows install because, per research §23's own precise-reuse
table, upgrade's core reconciliation skeleton is largely install's own
machinery plus M9B-A's rollback (for `--atomic`) — sequencing it after
both means it has real primitives to compose rather than forward
references. M9B-B.9-B.11 (OCI pull, OCI push, provenance) are placed
after the cluster-facing chart-execution actions exist because a chart
fetched via OCI or a classic repo is only useful once install/upgrade can
consume it — research §39 confirms no technical dependency forces this
order, so it is chosen for narrative/product coherence ("can install a
chart already in hand" before "can also fetch a chart from a registry"),
per the brief's own suggested ordering. M9B-B.12 closes the milestone
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

### M9B-B.0 — Acceptance contract + renderer compatibility definition

**Purpose**: formalize this document plus `docs/NATIVE_HELM_ENGINE_
RESEARCH.md`'s §22-40 into a written, versioned compatibility contract
precise enough that a second engineer can implement M9B-B.1 from this
document alone; and, specifically, close the renderer-compatibility-gate
threshold decision the "Renderer compatibility matrix" section above
deliberately leaves open.

**Scope**: this document itself, reviewed and signed off; the exact
"good enough to unlock install/upgrade" threshold for the renderer
compatibility matrix (percentage of Sprig functions covered, which real-
world corpus charts must render byte-identically, whether zero
"explicit refusal" rows are required); a written go/no-go decision that
the 4-6-month rendering-engine investment (research §25.5) is worth it
for this project's actual roadmap, per research §39's own "MUST-HAVE
BEFORE EXPANDING BEYOND ROLLBACK/UNINSTALL" item 2 — this is a product/
scope decision, not a technical unknown, and this document does not make
it on the implementer's behalf; a written decision on which of classic-
repo-pull and OCI-pull/push (or both, or neither) the project actually
needs, per research §39's item 3; confirmation of which pinned `helm` CLI
version(s) M9B-A.0 already decided are reused here (no new pinning
decision expected, since M9B-B tests the same oracle binaries against a
larger fixture set).

**Explicit non-goals**: no code in `crates/helm-engine`'s new `chart`/
`render`/`oci` modules yet — that begins at M9B-B.1. No real-world chart
corpus fixtures collected/vendored yet (M9B-B.5's own concern, though the
corpus *list* may be finalized here).

**Architecture/invariants**: none newly introduced; this slice consumes
and ratifies the Invariants section above, including the new RENDER
DETERMINISM / RENDERING GATE / VALUES NEVER SUBSTITUTE FOR MANIFEST / NO
SILENT CHART-ACQUISITION FALLBACK / OCI-VIA-CRATE-ONLY / CRD-PERMANENCE
invariants.

**Security constraints**: none newly introduced at this slice (no code
exists yet to constrain); this slice does record, in writing, that
rendered chart output (Notes.txt, arbitrary values-derived strings) is
subject to the same "never enter Object/Store/Timeline/graph/journal
unredacted" discipline M9.5/M9B-A already established, extended
explicitly to cover render-produced content this document is the first
to introduce.

**Mutation-gateway requirements**: none yet.

**Exact stop-and-ask decisions**: (1) the renderer-compatibility-gate
threshold (above) — must be recorded with explicit sign-off before
M9B-B.3 begins. (2) the go/no-go on the 4-6-month rendering investment
itself (research §39 item 2) — must be recorded with explicit sign-off
before M9B-B.1 begins, since M9B-B.1-B.2 are only useful in service of a
renderer that is actually going to be built. (3) which of OCI pull/push/
classic-repo-provenance the project actually needs (research §39 item 3)
— must be recorded before M9B-B.9-B.11 begin, though it may be decided
later than (1)/(2) since those slices are independent of the renderer
track. (4) the exact real-world chart corpus list for M9B-B.5's gate
(Bitnami/ingress-nginx/cert-manager/prometheus-community proposed by
research MH11 — confirm or revise, don't assume the proposal is final
without sign-off).

**Unit / fake-HTTP / live / differential / failure-injection tests**:
none (pure documentation slice, mirroring M9B-A.0).

**Acceptance criteria**: this document exists, is internally consistent
with `docs/NATIVE_HELM_ENGINE_RESEARCH.md` and `docs/M9B_A_ACCEPTANCE.md`,
and all four stop-and-ask decisions above are recorded with explicit
sign-off in this document's own Journal before M9B-B.1 starts.

**Documentation updates**: this file; `docs/M9B_B_RENDERER_MATRIX.md` (or
equivalent, named here) created as an empty, structured skeleton (rows
defined, all unpopulated) — populated incrementally starting at M9B-B.3.

**Commit/tag expectations**: no tag at this slice, mirroring every prior
milestone's own convention of tagging only at final combined acceptance
(M9B-B.12).

### M9B-B.1 — Chart model / archive loading

**Purpose**: implement the chart object model and archive-loading logic
— `Chart.yaml` parsing, chart-directory and `.tgz` tarball loading,
`templates/`/`crds/`/`charts/` (subchart)/`values.yaml`/`values.schema.json`
tree assembly — assuming charts are already-fetched/local (per this
document's own non-goal on dependency *resolution*). Corresponds to
research MH10's chart-loading half.

**Scope**: `chart::model` (the Rust equivalent of `chart.Metadata`/
`chart.Chart`'s field shape — name/version/apiVersion/dependencies/
`values.yaml` defaults/`Files`); `chart::load` (directory and tarball
loaders, recursive subchart assembly mirroring `recAllTpls`'s own
recursion pattern minus the template-execution part, which is
M9B-B.3's); `chart::crds` (reads the `crds/` directory's contents as raw
files, does not template them — restated from research §22's own
explicit "never templated" finding). No dependency *resolution* — a
chart's `charts/` subchart directory is read as-is, exactly as it
exists on disk, never re-resolved from `Chart.lock` against a repo index.

**Explicit non-goals**: no values coalescing yet (M9B-B.2). No template
execution yet (M9B-B.3). No OCI/classic-repo fetch yet (M9B-B.9/B.11) —
this slice's own fixtures are local directories/tarballs the test harness
provides directly.

**Architecture/invariants**: chart loading is read-only, local-filesystem-
or-provided-bytes-only at this slice — no network I/O anywhere in
`chart::load`. The loader is strict about `Chart.yaml`'s `apiVersion`
field (v1 vs v2 chart shape differences, if the research corpus surfaces
any — a fixture-driven discovery, not assumed identical). Tarball loading
reuses the same bounded-decompression discipline M9.5's `decode_release`
already established (a chart tarball is an attacker-influenceable input
once OCI/repo fetch exists, even though this slice's own fixtures are
locally trusted) — the bound is decided here, applied consistently from
the first line of tarball-reading code, not retrofitted later.

**Security constraints**: a malformed or maliciously-crafted chart
archive (path traversal via `../` in tarball entry names, a decompression
bomb, an absurdly deep subchart nesting) must fail closed with a named
error, never partially load or panic — this is the first place M9B-B
accepts untrusted-shaped input, so this slice's own security review is
explicit, not inherited implicitly from a later slice.

**Mutation-gateway requirements**: none yet — chart loading is pure,
no cluster I/O.

**Exact stop-and-ask decisions**: does the chart model need to support
both Helm v2 and v3 `Chart.yaml` `apiVersion` shapes, or is v2 assumed
extinct in the target chart population — resolve before this slice's
fixture corpus is finalized, since it changes what "loads correctly"
means for older charts.

**Unit tests**: chart-directory loading round-trips a hand-constructed
fixture into the correct `chart::model` shape; tarball loading matches
directory loading for an identical chart packaged both ways; subchart
recursion assembles a multi-level umbrella-chart fixture into the correct
nested structure; `crds/` contents are read as raw bytes, never parsed as
templates (a dedicated test proving a `crds/`-directory file containing
`{{ .Values.foo }}`-shaped text is *not* templated); path-traversal and
decompression-bomb fixtures are refused with a named error.

**Fake-HTTP tests**: none required (no network I/O at this slice).

**Live tests**: none required at this slice (pure local-filesystem
logic).

**Differential Helm-oracle tests**: a hand-fixtured chart directory,
loaded by both the native engine and `helm show chart`/`helm show
values`/`helm show all` (or an equivalent inspection command against the
real CLI), agrees on the parsed metadata/values-defaults/file-tree shape
— MH10's own chart-loading half of its stop condition.

**Failure-injection tests**: a chart archive with an unreadable/corrupt
entry (truncated tarball, unreadable permission bits in a test-controlled
fixture) surfaces an explicit load error, never a partial chart silently
treated as complete.

**Acceptance criteria**: chart-directory and tarball loading agree with
each other and with the real `helm` CLI's own chart-inspection output for
the fixture corpus; all malicious-input fixtures refuse cleanly.

**Documentation updates**: this document's Journal; `crates/helm-engine`'s
own module-level doc comments for `chart::model`/`chart::load`/
`chart::crds`.

**Commit/tag expectations**: no tag.

### M9B-B.2 — Values coalescing

**Purpose**: implement `chartutil.CoalesceValues`/`CoalesceTables`/
`coalesceGlobals`'s exact merge semantics — "values in a higher-level
chart always override values in a lower-level dependency chart," scalar/
array wholesale replacement, map merging, per-subchart values-scoping to
`Values.<subchart-name>` plus shared `global`, and the distinct
null-preserving `MergeValues` variant upgrade's diff-preview path needs.
Corresponds to research MH10's values half.

**Scope**: `chart::values::coalesce` (the `CoalesceValues`-equivalent,
null-stripping); `chart::values::merge` (the `MergeValues`-equivalent,
null-preserving, needed later by M9B-B.8's diff-preview and by
`--reset-then-reuse-values`); `chart::values::globals` (the
"experimental," explicitly-cited-as-unusual `coalesceGlobals` nested-
table-under-`global` reversal, reproduced exactly as documented rather
than "simplified" — research §25.1 explicitly flags this as an
in-source-commented oddity worth reproducing faithfully, not smoothing
over).

**Explicit non-goals**: no template execution yet (M9B-B.3) — this slice
produces the *coalesced value tree* the renderer will later consume; it
does not render anything. No `reuseValues`'s upgrade-specific
`--reset-values`/`--reuse-values` orchestration yet (M9B-B.8) — this
slice builds the two underlying merge primitives (`CoalesceValues`/
`CoalesceTables` roughly correspond to `chart::values::coalesce`;
`MergeValues` to `chart::values::merge`) that `reuseValues`'s four modes
will later compose, per research §23's own citation of exactly which
primitives each mode calls.

**Architecture/invariants**: subchart value isolation is structural, not
convention — a subchart's rendering context must not be able to observe
a sibling subchart's own values subtree, only its own (`Values.<name>`)
and `global`, enforced by the coalescing step producing a scoped tree per
subchart, not by trusting the renderer to respect a naming convention
(research §25.1's own cited isolation property, restated as a binding
architectural requirement here).

**Security constraints**: none new beyond M9B-B.1's — coalescing is pure
in-memory logic over already-loaded, already-bounded input.

**Mutation-gateway requirements**: none yet.

**Exact stop-and-ask decisions**: does `coalesceGlobals`'s "experimental"
nested-table reversal need to be reproduced exactly as currently
implemented in the pinned Helm version, or does it need version-gating
if the pinned v3.x/v4.x oracles disagree on this specific behavior —
resolve by direct differential test before this slice is considered
complete, not assumed identical across versions.

**Unit tests**: scalar/array replace-wholesale vs. map-merge behavior for
every documented `CoalesceValues` precedence case; per-subchart isolation
(a fixture proving subchart A cannot see subchart B's own values); global
propagation into every subchart's own scope; `MergeValues`'s null-
preservation contrasted directly against `CoalesceValues`'s null-
stripping for the same input, in the same test, to make the divergence
impossible to miss in review.

**Fake-HTTP tests**: none required (pure in-memory logic).

**Live tests**: none required at this slice.

**Differential Helm-oracle tests**: a hand-fixtured umbrella chart with
nested subcharts and a `global` value, coalesced by both the native
engine and a values-only extraction from real `helm template --debug`'s
own printed computed values (or an equivalent real-CLI values-dump
mechanism, decided at implementation time), agree exactly — MH10's own
"verified via a values-only differential test (no rendering yet)" stop
condition, verbatim.

**Failure-injection tests**: a values override referencing a path that
does not exist in the chart's own `values.yaml` defaults is handled per
Helm's own documented behavior (additive, not an error) — a dedicated
test proving the engine does not spuriously reject a valid override.

**Acceptance criteria**: MH10's own stop condition, verbatim: "a
hand-fixtured chart+values pair coalesces identically to `helm
template`'s own internal coalescing, verified via a values-only
differential test (no rendering yet)."

**Documentation updates**: this document's Journal.

**Commit/tag expectations**: no tag.

### M9B-B.3 — Template-engine compatibility core

**Purpose**: the single largest new undertaking in this entire milestone
(research §25, §37, §39's own "BIGGEST NEW TECHNICAL RISK"): a
Go-template-syntax-compatible lexer/parser/executor with real dot/context
semantics, pipelines, `include`/`template`/`tpl`/`required`/`fail`
behavior, and the `Chart`/`Release`/`Capabilities`/`Values`/`Files`/
`Subcharts` object model — built as its own deliberately-scoped subsystem,
not a byproduct of anything else. Corresponds to research MH11's
grammar/executor half.

**Scope**: `render::engine` (lexer/parser/executor for Go-template
syntax — `{{ }}`, `{{- -}}` whitespace trim, `if`/`else if`/`else`/
`range`/`with`/`define`/`template`/`block`, pipeline chaining, `$`-rooted
context access); `render::context` (the `Chart`/`Release`/`Capabilities`/
`Values`/`Files`/`Subcharts` builtin-object assembly, mirroring
`chartutil.ToRenderValues`'s exact field shape including the hardcoded
`Release.Service = "Helm"` literal); `render::include` (the `include`/
`template`/`tpl` late-binding machinery, built as its own module given
research §25.5's explicit naming of this as the hardest-to-get-right
piece); `render::builtins` (`required`/`fail`/`toToml`/`fromToml`/
`toYaml`/`fromYaml`/`toJson`/`fromJson` — the Helm-added, non-Sprig
functions `pkg/engine/funcs.go` registers directly). `lookup` is
explicitly deferred to this slice's own stop-and-ask decision below, not
assumed in scope by default. Sprig's actual function *library* is
deliberately deferred to M9B-B.4 — this slice's own fixtures use a
minimal, hand-written stand-in func map sufficient to exercise the
grammar/executor/context/include machinery in isolation, per this
document's own stated ordering rationale (hard architectural piece
before mechanical function-library piece).

**Explicit non-goals**: no Sprig function library (M9B-B.4). No
`template` command wiring (M9B-B.5). No install/upgrade wiring (M9B-B.6-
B.8). No live-cluster `Capabilities` discovery yet — this slice's own
tests supply a fixed, hand-constructed `Capabilities` value; live
discovery is M9B-B.5/B.7's concern once a real cluster target exists.

**Architecture/invariants**: this is the slice the RENDER DETERMINISM
invariant is first tested against — every executor operation must be
free of incidental non-determinism (map iteration order, especially,
since Rust's default `HashMap` iteration order is randomized per-process
unlike Go's own `text/template`, which the Helm ecosystem has come to
rely on producing *some* stable — if not formally guaranteed — ordering
in practice for `range`-over-map cases charts empirically depend on; this
must be resolved with an explicit, deterministic ordering choice, e.g. a
`BTreeMap`/sorted-keys strategy, not left to incidental behavior). Dot-
scoping is implemented as an explicit, testable stack/frame model (not an
ambient global), so `with`/`range`'s rebinding and `$`'s "always the
original top-level context" semantics are structurally guaranteed, not
merely usually-correct. `include`/`tpl`'s late-binding must correctly
re-enter the executor with a *new* dot-context while sharing the *same*
template-definition namespace — the specific interaction research §25.5
flags as where "renders something similar" diverges hardest from
"renders identically."

**Security constraints**: `tpl`'s ability to execute an arbitrary
string as a template (sourced from `.Values`, i.e. attacker-influenceable
if values ever come from an untrusted source) is treated as a genuine
input-validation boundary — bounded recursion depth (a `tpl`-calling-
`tpl`-calling-`tpl` cycle, or a chart triggering effectively unbounded
recursion via `include`, must fail with a named "recursion limit
exceeded" error, never stack-overflow the process) — mirroring M9.5's own
bounded-gunzip discipline applied to a new class of input (template
recursion depth, not compressed-byte size).

**Mutation-gateway requirements**: none yet — rendering is pure, no
cluster I/O (beyond the deferred `lookup` decision below).

**Exact stop-and-ask decisions**: (1) whether `lookup` is implemented in
this slice at all, and if so, whether it queries a real cluster
(requiring a `kube-rs` client threaded into the render context — a
capability install/upgrade will have but `template`'s client-only mode
explicitly will not, per research §24) or is deferred to M9B-B.7 once a
cluster-facing action exists — resolve before this slice's `render::
builtins` module is considered complete, don't leave it half-implemented
silently. (2) the map-iteration-order determinism strategy (above) —
resolve and record before any fixture depending on `range`-over-map
ordering is written, since fixtures written against an accidental
ordering would be fragile, not evidence of correctness.

**Unit tests**: every control-construct (`if`/`else if`/`else`/`range`/
`with`/`define`/`template`/`block`) against hand-constructed fixtures,
including nested combinations; `{{- -}}` whitespace-trim fixtures
targeting YAML-indentation-sensitive output specifically (not just
"trims whitespace" in the abstract); `$`-rooted context access from
inside a `range`/`with`-rebound scope; `include`/`tpl` late-binding
fixtures, including a self-referential `include` case; `required`/`fail`
error-shape fixtures; recursion-depth-limit fixture (a deliberately
runaway `include`/`tpl` chain is refused, not a stack overflow).

**Fake-HTTP tests**: none required unless the `lookup` stop-and-ask
decision above resolves to "queries a real cluster in this slice," in
which case a fake-HTTP fixture for `lookup`'s own API call shape is
required.

**Live tests**: none required at this slice (pure rendering logic,
unless `lookup`'s stop-and-ask decision pulls in cluster discovery, in
which case a minimal live smoke test against the isolated test cluster
is added).

**Differential Helm-oracle tests**: the deterministic-rendering-fixture
corpus (this document's own "Renderer compatibility matrix" section)
rendered by both the native engine and real `helm template` against a
minimal stand-in chart (using this slice's own hand-written func-map
stand-in for anything Sprig would normally provide, explicitly noted as
"minimal, not representative of Sprig coverage" in the fixture's own
documentation) — proving the grammar/executor/context/include machinery
in isolation, ahead of Sprig's own much larger differential surface
(M9B-B.4).

**Failure-injection tests**: a malformed template (unbalanced `{{`/`}}`,
an undefined `template` reference, a `range` over a non-iterable value)
produces an explicit parse/execution error, never a partial/garbled
render silently returned as if successful.

**Acceptance criteria**: the deterministic-rendering-fixture corpus
passes differentially against real `helm template` for every construct
listed in the "What the renderer acceptance plan must define" section
above (excluding Sprig-specific functions, M9B-B.4's own gate); the
renderer compatibility matrix's grammar/executor/context rows are
populated (implemented-tested / documented-divergence / explicit-refusal)
for the first time at this slice.

**Documentation updates**: this document's Journal;
`docs/M9B_B_RENDERER_MATRIX.md`'s grammar/executor/context/include rows
populated.

**Commit/tag expectations**: no tag.

### M9B-B.4 — Sprig compatibility surface

**Purpose**: port Sprig's ~100-170-function library (research §25.2) on
top of M9B-B.3's grammar/executor, function-by-function, each entry
landing in the compatibility matrix as implemented-tested,
implemented-with-documented-divergence, or explicitly-refused — never
silently omitted without a matrix entry.

**Scope**: `render::sprig` (organized by research §25.2's own category
breakdown: date/time, string manipulation, math, list ops, dict/map ops,
JSON, b64/b32 encoding, cryptography, regex, semver, reflection, UUID,
URL parsing, plus the `must*`-family error-returning variants); explicit,
documented decisions for the long-tail items research §25.5 names as
disproportionately costly (byte-exact PRNG for crypto helpers like
`genSelfSignedCert`/`derivePassword`/`encryptAES`; Go RE2 vs. Rust
`regex`-crate flavor differences; locale-sensitive casing) — each either
ported with a stated fidelity caveat or explicitly refused with a named
reason, mirroring `lithos-sprig`'s own precedent of shipping v0.1.0 by
explicitly cutting randomness/regex/pluralization scope rather than
silently under-delivering them.

**Explicit non-goals**: `env`/`expandenv` are deliberately **excluded**,
matching Helm's own deletion of them from Sprig's default func map
(research §25.1) — not an oversight, a restated-here binding non-goal.
No install/upgrade wiring yet.

**Architecture/invariants**: every Sprig function lands with its own
matrix row before it is considered "in the renderer" — there is no
"mostly ported, undocumented gaps" state this slice is permitted to
leave behind. The mechanical bulk (string/math/list/dict/encoding, per
research §25.5's own "days, not weeks" estimate using `chrono`/`regex`/
`base64`/`sha2`/`semver`) is built first; the long tail (crypto/regex/
locale edge cases, research §25.5's own "3-6 weeks... plus an open-ended
tail" estimate) is tackled with an explicit, tracked list, not folded
silently into "done."

**Security constraints**: cryptographic Sprig functions
(`genSelfSignedCert`, `derivePassword`, `encryptAES`/`decryptAES`,
`sha256sum`, etc.) must not be assumed safe defaults for anything
SAUR-ON itself relies on for its own security properties — they are chart-
template conveniences the engine reproduces for compatibility, never
promoted to an internal cryptographic primitive SAUR-ON's own code
depends on. This must be a stated, reviewed boundary, not an
implementation accident.

**Mutation-gateway requirements**: none yet.

**Exact stop-and-ask decisions**: for each long-tail item (byte-exact
crypto PRNG behavior; regex-flavor differences; locale-sensitive casing)
— is exact upstream-byte-identical behavior required, or is a documented,
narrower-scoped divergence acceptable for that specific function — decide
function-by-function, recorded in the matrix, not as a single blanket
policy.

**Unit tests**: every implemented Sprig function has at least one
fixture exercising its documented behavior, matched against the real
Sprig/Helm output for representative inputs; every explicitly-refused
function has a fixture proving the engine refuses it by name (not by
generic "unknown function" error) when a chart references it.

**Fake-HTTP tests**: none required (Sprig functions are pure, no network
I/O, `env`/`expandenv` explicitly excluded).

**Live tests**: none required at this slice.

**Differential Helm-oracle tests**: a per-category differential fixture
set (mirroring research §25.2's ~13 categories), each function's output
compared against real `helm template`'s own rendering of an equivalent
minimal chart invoking that function — the compatibility matrix's own
primary evidence source.

**Failure-injection tests**: a chart invoking an explicitly-refused
function (or an implemented-with-documented-divergence function outside
its documented fidelity envelope, if that envelope is itself checkable at
render time) surfaces the named refusal/divergence-warning, never a
silent wrong answer.

**Acceptance criteria**: the compatibility matrix's Sprig rows are fully
populated (every one of research §25.2's ~13 categories has at least one
row per function, no function silently omitted from the matrix); the
matrix's own explicitly-refused-function count and coverage percentage
match, or are explicitly reconciled against, the threshold M9B-B.0
decided.

**Documentation updates**: this document's Journal;
`docs/M9B_B_RENDERER_MATRIX.md`'s Sprig rows populated in full.

**Commit/tag expectations**: no tag.

### M9B-B.5 — Template command/path

**Purpose**: ship `helm template`'s own client-only rendering path —
`ClientOnly` mode with a faked capability set (mirroring
`kubefake.PrintingKubeClient`'s own no-op-lookup behavior), extracting
NOTES.txt, sorting rendered manifests via install-order (research §22's
own `releaseutil.SortManifests(..., InstallOrder)`, reusing M9B-A.4's
existing `manifest::split_sort` module directly rather than
reimplementing it) — and run the renderer against the real-world chart
corpus for the first time. This is the renderer's own first real
consumer and this milestone's compatibility-gate-defining slice.
Corresponds to research MH12.

**Scope**: `actions::template` (client-only render, no cluster calls,
no `--dry-run=server` yet — restated non-goal below); wiring M9B-B.1-B.4's
chart-loading/values-coalescing/rendering pipeline end-to-end for the
first time; the real-world chart corpus (Bitnami, ingress-nginx,
cert-manager, prometheus-community, per M9B-B.0's own confirmed list)
vendored/fetched as fixed, pinned test fixtures.

**Explicit non-goals**: no `--dry-run=server` (that requires a real
cluster round-trip through the apply layer for server-side validation —
deferred to whichever of M9B-B.7/B.8's own contracts decides it's in
scope, since research §24/§40 item 1 leaves its exact wire mechanics an
open unknown not yet required to be closed). No install/upgrade action
yet (M9B-B.7/B.8). This slice's `Capabilities` is the fixed, faked
client-only set — live-cluster-discovered `Capabilities` for install/
upgrade is those slices' own concern.

**Architecture/invariants**: this is the slice where the RENDERING GATE
invariant is actually enforced as a stop condition — install/upgrade
implementation (M9B-B.6 onward) does not begin until this slice's
acceptance criteria (below) pass, per M9B-B.0's own decided threshold.

**Security constraints**: none new beyond M9B-B.1-B.4's; NOTES.txt
extraction inherits M9.5's own "Notes are deliberately never shown"
posture by default for anything surfaced through the generic mutation-
workflow UI — `template`'s own preview surface is the first place a
chart's rendered Notes could legitimately be shown to a user requesting
a template preview, and whether it is shown here (a genuinely different
context from M9.5's read-only release-inspection view, since the user is
explicitly asking to preview a chart they are about to install) is its
own stop-and-ask decision, not a silent default.

**Mutation-gateway requirements**: none — `template` never mutates
anything, cluster or otherwise; it is a pure render-and-display action,
though it still flows through `mutation::workflow`'s preview machinery
if that is how it is surfaced in the UI (a stop-and-ask decision, not
assumed).

**Exact stop-and-ask decisions**: (1) whether `template`'s rendered
Notes.txt is surfaced to the user, per the security-constraints note
above. (2) whether `template` is exposed as a genuine `mutation::
workflow`-shaped preview-only `Built`, or as a separate, simpler
read-only command outside the mutation gateway entirely (it never
mutates, so the gateway's confirmation/policy machinery may be pure
overhead for this specific action) — resolve before command-grammar
work begins, since it changes where the command lives in the registry.

**Unit tests**: end-to-end pipeline wiring (load → coalesce → render →
sort → NOTES-extract) for a hand-constructed umbrella chart, asserting
each stage's output is passed to the next unmodified beyond that stage's
own documented transformation.

**Fake-HTTP tests**: none required (client-only mode makes no cluster
calls by construction — a dedicated regression test asserts **zero**
HTTP calls are issued during a `template` invocation, mirroring M8B's own
"429 never triggers a fallback" zero-call-count test discipline applied
here to a different property).

**Live tests**: none strictly required (client-only, no cluster
dependency) — though a live smoke test confirming the corpus renders
identically whether the test harness runs online or fully offline is a
reasonable, cheap addition.

**Differential Helm-oracle tests**: the real-world chart corpus (Bitnami,
ingress-nginx, cert-manager, prometheus-community, at minimum, per
research MH11's own stop condition) rendered by the native engine and by
real `helm template`, for the same chart+values, producing byte-identical
(or individually documented and justified) manifests. **This is the
renderer-compatibility-gate's own primary acceptance evidence** — the
threshold decided at M9B-B.0 is checked against this corpus's actual
results here, for real, not merely asserted.

**Failure-injection tests**: a corpus chart that fails to render (an
unimplemented Sprig function it happens to use, a template construct not
yet covered) produces the compatibility matrix's own named refusal, is
individually triaged (does this block the gate, or is it an acceptable,
documented gap), and is never silently skipped from the corpus results
without that triage being recorded.

**Acceptance criteria**: MH11-12's own stop condition, verbatim:
"rendering a corpus of real charts (Bitnami, ingress-nginx, cert-manager,
prometheus-community, at minimum) produces byte-identical (or documented,
justified diff) manifests to real `helm template` output," **and** this
result meets or exceeds M9B-B.0's own decided renderer-compatibility-gate
threshold — if it does not, this slice does not pass, and M9B-B.6 onward
do not begin, regardless of how much of M9B-B.1-B.4's own work is
otherwise complete.

**Documentation updates**: this document's Journal, recording the actual
corpus results against the M9B-B.0 threshold — explicitly, not
summarized as "mostly works"; `docs/M9B_B_RENDERER_MATRIX.md` updated
with final corpus-derived evidence for every row it can inform.

**Commit/tag expectations**: no tag (the milestone's tag is M9B-B.12
only), but this slice is the natural point to record, in the Journal, a
clear PASS/FAIL against the rendering gate — a load-bearing checkpoint
even without a git tag attached to it.

### M9B-B.6 — CRD-first install semantics

**Purpose**: implement `installCRDs`'s own self-contained ~60-line
special case (research §22): install-time creation of a chart's `crds/`-
directory CRDs, tolerating `AlreadyExists`, followed by a fixed-60-second-
timeout wait and an explicit discovery-cache/REST-mapper invalidation so
later rendering/capability code can see the new CRD's types immediately.

**Scope**: `install::crds` (create-CRDs-first, tolerate-AlreadyExists,
fixed-timeout wait, discovery-cache invalidation); the hardcoded
`CustomResourceDefinition`-never-deleted-by-hook-policy carve-out is
already proven by M9B-A.7's hook engine — this slice's own job is only to
confirm the *install-time* CRD creation path itself, not to re-prove the
hook-delete carve-out a second time.

**Explicit non-goals**: no CRD *update* logic beyond what install itself
needs (Helm's own `installCRDs` never updates an existing CRD — it
tolerates `AlreadyExists` and moves on, a documented, restated-here
behavior this slice reproduces exactly, not "improves"). No CR (custom
resource *instance*, as opposed to the CRD definition) handling here —
CR instances flow through the ordinary manifest apply path (M9B-A.5/A.8),
unchanged.

**Architecture/invariants**: the discovery-cache/REST-mapper invalidation
sequencing must be replicated precisely — research §22's own explicit
warning that a chart whose own templates reference its own just-installed
CRD kind will fail to render if this sequencing is wrong. This is the
CRD-PERMANENCE invariant's install-time half: CRDs created here are never
deleted by any M9B-A or M9B-B action, ever, by construction (there is no
code path anywhere in this milestone that deletes a `CustomResourceDefinition`
resource — a structural, testable absence, not a policy comment).

**Security constraints**: none new beyond the apply layer's existing
ones — CRD creation is an ordinary create call through the same gateway,
carrying no special Secret-body exposure risk.

**Mutation-gateway requirements**: CRD create/wait calls are new
executor-dispatch branches under the existing `kube::mutation` gateway
(mirroring M9B-A.5/A.8's own precedent), never a parallel client.

**Exact stop-and-ask decisions**: the fixed 60-second CRD-wait timeout —
reproduced exactly as upstream's own hardcoded, non-configurable value,
or made configurable for SAUR-ON's own operational needs — decide and
record before implementing, since upstream's own choice not to make it
configurable may itself be a deliberate signal worth preserving.

**Unit tests**: CRD-creation-tolerates-AlreadyExists fixture; discovery-
cache invalidation is called in the correct order relative to CRD
creation and subsequent rendering (a fixture chart whose own template
references the CRD it just installed, proving the ordering matters and
is correct).

**Fake-HTTP tests**: CRD create calls issue in the documented order
(before capabilities discovery, before rendering) against a fake
endpoint; an `AlreadyExists` response on CRD create is tolerated, never
surfaced as an error.

**Live tests**: install a chart bundling both a CRD and a CR instance of
that CRD kind against the isolated test cluster; confirm the CRD is
created, the CR instance renders and applies successfully (proving the
discovery-cache invalidation actually let rendering see the new type),
and neither is ever deleted by a subsequent SAUR-ON-driven rollback/
uninstall/upgrade operation targeting an unrelated resource.

**Differential Helm-oracle tests**: the same CRD+CR fixture, installed
by real `helm install` and by the native engine, produce the same
resulting CRD/CR state, and the real `helm` CLI can subsequently operate
on the native-engine-installed release without error (this milestone's
own restated "critical follow-up" test, per M9B-A's own established
convention).

**Failure-injection tests**: a CRD-wait timeout (the CRD never becomes
established within 60s, simulated against a disposable test fixture)
surfaces as an explicit failure, never a silent proceed-anyway.

**Acceptance criteria**: research §22's own CRD-install-first behavior
reproduced exactly, proven by the fixtures above; the CRD-never-deleted
invariant holds structurally (no code path exists that could violate it).

**Documentation updates**: this document's Journal.

**Commit/tag expectations**: no tag.

### M9B-B.7 — Native install

**Purpose**: compose M9B-B.1-B.6 plus M9B-A's storage/lifecycle/apply/
hooks into the first real, guarded `install` action — chart
loading/coalescing/rendering, revision-1 creation, CRD-first handling,
`HookPreInstall`/`HookPostInstall`, resource creation/adoption, and status
transition to `Deployed`/`Failed`. Corresponds to research MH13.

**Scope**: `actions::install`; live-cluster `Capabilities` discovery
(the real counterpart to M9B-B.3/B.5's client-only faked set);
`existingResourceConflict`/`requireAdoption` (the pre-existing-resource
handling install shares with upgrade, research §22-23); `Install.Atomic`
(on-failure full uninstall of the just-created release, delegating
entirely to M9B-A's own `actions::uninstall`, never a parallel
reimplementation, mirroring research §22's own "delegates to Uninstall"
finding). Requires M9B-A.8 (SSA) to already exist, since install's
resource-creation path may need to interoperate with whichever apply
regime is in effect for the target cluster's Helm version expectations.

**Explicit non-goals**: no `--dry-run=server`'s exact wire-level mechanics
unless this slice's own implementation surfaces a concrete need to assert
byte-level equivalence to Helm's own dry-run behavior — otherwise, reuse
`kube-rs`'s own dry-run parameter directly (research §40 item 1's own
"not required unless a concrete discrepancy surfaces" framing). No repo/
OCI chart resolution yet (M9B-B.9-B.11) — this slice installs from an
already-loaded local chart only.

**Architecture/invariants**: revision-1 creation happens *before*
resources are applied (research §22's own `Releases.Create(rel)` ordering,
before `performInstall`) — mirroring M9B-A.6's own rollback two-phase-
write discipline, now for install's own single-phase-then-apply shape.
`Install.Atomic`'s failure path is a real call into M9B-A's own
`actions::uninstall`, not a hand-rolled cleanup — this is the slice where
the "M9B-B reuses M9B-A verbatim" claim gets tested for a second time,
now for a cross-action composition (install calling uninstall) rather
than a shared low-level primitive.

**Security constraints**: inherits M9B-A.2's write-path constraints in
full, extended to cover the newly-rendered manifest/values (never logged/
journaled unredacted, mirroring the "Relationship to prior milestones"
section's own journal-entry redaction restatement above).

**Mutation-gateway requirements**: install's create/CRD/hook calls are
new executor-dispatch branches under the same `kube::mutation` gateway,
composing M9B-A.5/A.8's apply layer and M9B-A.7's hook engine directly —
still no second gateway. TOCTOU: the release-name-availability check
(`availableName()`, research §22) is revalidated fresh at commit time,
not merely at preview time, mirroring M9B-A.6's own revision-identity
revalidation discipline applied to install's own precondition.

**Exact stop-and-ask decisions**: (1) what exact preview/confirmation
strength install receives under `mutation::policy::evaluate` — proposed:
at least as strong as M9B-A.6's rollback/uninstall
`RequireStrongerConfirmation`, since install can create arbitrary
cluster-scoped resources including CRDs — needs explicit sign-off, not an
assumed default. (2) whether `existingResourceConflict`/`requireAdoption`'s
exact adoption semantics (which existing resources may be "adopted" into
a new release vs. treated as a hard conflict) are reproduced exactly or
deliberately made stricter (mirroring this document's own "divergence is
only ever additive safety" rule) — decide and record before this slice's
resource-creation logic is written.

**Unit tests**: revision-1 creation precedes resource application in
isolation from cluster I/O (state-machine-level test, reusing M9B-A.3's
harness); `Install.Atomic`'s failure path correctly constructs and invokes
M9B-A's `actions::uninstall` rather than a hand-rolled equivalent (a
dedicated test asserting the *same function* is called, not a lookalike);
`existingResourceConflict`/`requireAdoption` classification against
hand-fixtured pre-existing-resource scenarios.

**Fake-HTTP tests**: install's full commit sequence (availableName check,
CRD creation if present, capabilities discovery, revision-1 write,
pre-install hook, resource create, post-install hook, terminal status
write) against a fake endpoint, asserting exact call order; a name
conflict on `availableName()` issues **zero** resource-creation calls.

**Live tests**: against a real, disposable, hook-free chart (no CRDs) on
the isolated test cluster: install, verified via a fresh GET of both the
new release Secret and the created live resources; a CRD-bundling chart
(composing M9B-B.6); a chart deliberately designed to fail
`performInstall` with `Atomic` set, verified the release is fully
uninstalled afterward (via `actions::uninstall`, confirmed by the same
verification M9B-A.6's own uninstall live test already established).

**Differential Helm-oracle tests**: fixture #23 (new, this milestone) —
baseline install correctness (a real-world corpus chart, install via
native engine, compared against real `helm install` for the same
chart+values: resulting resources, revision, release status, stored
metadata verified via the real `helm get` commands per this document's
own restated "self-decode-only is not sufficient evidence" rule);
fixture #24 — the "subsequent real `helm upgrade`/`helm uninstall`"
critical test, mirroring M9B-A.6's own critical-test convention, now for
install: after native install, the real `helm` CLI must successfully
operate on the release without error; fixture #25 — `Install.Atomic`
failure-and-cleanup, compared against real `helm install --atomic`'s own
failure behavior for an equivalent deliberately-failing chart.

**Failure-injection tests**: every applicable research §10-style failure
case reproduced for install specifically (mid-apply failure leaves the
release `Failed`, not silently `Deployed`); a crash between revision-1
write and resource application leaves a permanently `pending-install`
release, refused by a fresh install/uninstall attempt per this document's
own inherited `pending-*` hard-refusal invariant.

**Acceptance criteria**: research MH13's own stop condition, verbatim:
"installing the MH11 corpus via the native engine, then operating on the
result with the real `helm` CLI (`helm status`/`upgrade`/`uninstall`),
matches real-`helm`-installed behavior."

**Documentation updates**: this document's Journal.

**Commit/tag expectations**: no tag.

### M9B-B.8 — Native upgrade

**Purpose**: compose M9B-B.7's own machinery plus M9B-A's rollback
(for `--atomic`) into the first real, guarded `upgrade` action —
`prepareUpgrade`'s pending-release guard, `reuseValues`'s four modes,
chart re-rendering, new-resource diffing/adoption, `KubeClient.Update`
(the literal same call rollback uses, per research §23's own "IDENTICAL
CALL SHAPE" finding), and `--atomic`'s delegate-to-rollback failure path.
Corresponds to research MH14.

**Scope**: `actions::upgrade`; `chart::values::reuse` (the four
`reuseValues` modes — default/`ResetValues`/`ReuseValues`/
`ResetThenReuseValues` — composing M9B-B.2's `coalesce`/`merge`
primitives exactly per research §23's own citation of which primitive
each mode calls); new-resource diffing/adoption (shared helper with
install, research §23's own table entry); `--recreate-pods`' `recreate()`
call (the literal same function rollback already uses, per research §23's
own citation — reused, not reimplemented); `Upgrade.Atomic`'s delegate-to-
`actions::rollback` failure path, mirroring install's own delegate-to-
uninstall pattern from M9B-B.7.

**Explicit non-goals**: no chart-*fetch* for "upgrade to a version I
don't have locally yet" — that is a chart-acquisition concern (M9B-B.9-
B.11 if the chart comes via OCI/repo, or simply "the user already has the
chart locally," per research §30's own finding that repo management is
out of scope). This slice's upgrade always operates on an already-loaded
local chart, exactly like install.

**Architecture/invariants**: the `prepareUpgrade` pending-release guard
(`errPending` if the last release `IsPending()`) is a real, source-
confirmed pessimistic-lock-like check research §23 explicitly flags as
"the exception, not a refutation" of Helm's otherwise-lockless design —
restated here as binding: SAUR-ON reproduces this specific guard exactly
(it is *already* stricter-than-elsewhere upstream behavior, not a place
SAUR-ON needs to add its own extra strictness on top). `Upgrade.Atomic`'s
failure path is a real call into M9B-A's own `actions::rollback`, exactly
mirroring M9B-B.7's install-delegates-to-uninstall pattern — the second
proof point that cross-action composition, not duplication, is the
actual shape of this codebase.

**Security constraints**: inherits M9B-A.2/M9B-B.7's write-path
constraints in full; `reuseValues`'s `ReuseValues` mode reconstructs the
*previous* release's fully-coalesced values from stored release data —
this in-memory reconstruction is subject to the same "never enter Object/
Store/Timeline/graph/journal unredacted" discipline as any other decoded
release content, restated here because it is a genuinely new code path
reading previously-stored values back out, not merely writing new ones.

**Mutation-gateway requirements**: upgrade's diff/apply/hook calls are
new executor-dispatch branches under the same `kube::mutation` gateway,
composing M9B-A.5/A.8's apply layer (the *same* call rollback already
proved), M9B-A.7's hook engine, and (for `--atomic`) M9B-A.6's own
rollback action directly. TOCTOU: both the release's current-revision
Secret UID and the target chart/values identity are re-verified fresh at
commit time, mirroring M9B-A.6's own dual-identity revalidation now
applied to upgrade's own current-vs-target pair.

**Exact stop-and-ask decisions**: (1) confirmation strength for upgrade
under `mutation::policy::evaluate` — proposed at least as strong as
install's own (M9B-B.7's stop-and-ask decision), needs explicit sign-off.
(2) exact preview text for `reuseValues`'s four modes — a user invoking
upgrade without understanding which values mode is in effect risks
silently losing or silently retaining values they didn't intend; the
preview must state which mode is active and what it implies, wording
decided and reviewed before shipping, mirroring M9B-A.6's own "creates a
new revision, does not restore the old one" wording-review precedent.

**Unit tests**: `reuseValues`'s four modes against hand-constructed
fixtures proving each mode's documented precedence exactly (default-
copies-current-Config-if-vals-empty; `ResetValues`-ignores-current-
entirely; `ReuseValues`-rebuilds-old-coalesced-then-merges-new;
`ResetThenReuseValues`-merges-only-without-full-rebuild); new-resource
diffing/adoption reusing install's own classification logic (a dedicated
test proving the *same* function is called, not a duplicate); `Upgrade.
Atomic`'s failure path correctly invokes M9B-A's `actions::rollback` with
the correct target revision (most recent `Superseded`/`Deployed`, per
research §23's own citation).

**Fake-HTTP tests**: upgrade's full commit sequence (pending-check,
prepareUpgrade's render+diff, revision-N+1 write, pre-upgrade hook,
update, `--recreate-pods` if applicable, post-upgrade hook, supersede,
terminal write) against a fake endpoint, exact call order; a
`pending-*`-state release issues **zero** further calls beyond the guard
check itself.

**Live tests**: against a real, disposable, previously-installed release
on the isolated test cluster: upgrade to a new chart version with each
`reuseValues` mode in turn, verified via fresh GETs of both the new
release Secret and the affected live resources; `--recreate-pods`
verified against a Pod-owning resource; `--atomic` upgrade failure,
verified the release is rolled back (via `actions::rollback`) to its
prior state, confirmed by the same verification M9B-A.6's rollback live
test already established.

**Differential Helm-oracle tests**: fixture #26 (new) — baseline upgrade
correctness across a real-world corpus chart's two versions, native
engine vs. real `helm upgrade`; fixture #27 — each `reuseValues` mode,
individually, against real `helm upgrade --reset-values`/
`--reuse-values`/`--reset-then-reuse-values`; fixture #28 — `--atomic`
upgrade failure and rollback, compared against real `helm upgrade
--atomic`'s own behavior; fixture #29 — the "subsequent real `helm`
compatibility" critical test, now for upgrade (`helm status`/`history`/
`rollback`/`upgrade`/`uninstall` all still work against a
native-engine-upgraded release); the upgrade/install-against-a-new-chart
`helm diff` case (research §33, this document's own "free once the
renderer plus M9B-A's apply layer exist" byproduct) exercised here for
the first time, since this is the first slice where both halves actually
exist together.

**Failure-injection tests**: every applicable research §10-style failure
case reproduced for upgrade; a crash between the `pending-upgrade` write
and the apply step leaves a permanently `pending-upgrade` release,
refused by a fresh upgrade/rollback attempt per the inherited `pending-*`
hard-refusal invariant — this is the second slice (after M9B-A.6) to
prove this divergence holds, now against upgrade's own two-revision
(current + new) shape rather than rollback's single-target shape.

**Acceptance criteria**: research MH14's own stop condition, verbatim:
"fixture matrix extended with upgrade-specific cases (`--force`/
`--atomic`/each `reuseValues` mode); atomic-failure path verified to
correctly re-invoke MH5's rollback, not a reimplementation."

**Documentation updates**: this document's Journal.

**Commit/tag expectations**: no tag.

### M9B-B.9 — OCI pull integration

**Purpose**: fetch charts from an OCI registry via `oci-client`
(`oras-project/rust-oci-client`), producing the same in-memory chart
bytes M9B-B.1's loader already knows how to parse — never a parallel
chart-loading path. Corresponds to research MH16's pull half.

**Scope**: `oci::pull` (wraps `oci-client`'s manifest+blob-pull calls
behind SAUR-ON's own narrow module boundary, mirroring M9.5's own
`pub(crate)`-disciplined decode-path precedent); `oci::reference` (the
`+`/`_` OCI-tag-normalization logic for semver build metadata — research
§27/§40 item 2 flags the *exact* upstream file/function location as
unconfirmed, so this slice's own implementation is behavior-matched
against real Helm's observed pull behavior, not against a cited source
line that does not yet exist); `oci::media_types` (the confirmed media-
type constants — `ConfigMediaType`/`ChartLayerMediaType`/
`ProvLayerMediaType`/`LegacyChartLayerMediaType` — including legacy-
media-type acceptance on pull, per research §28 point 3).

**Explicit non-goals**: no OCI push yet (M9B-B.10). No `helm repo`-style
registry bookkeeping (credentials storage, registry aliasing) — a chart
reference is taken as an explicit, fully-qualified `oci://` URL each time,
never resolved through a locally-cached registry-alias table (restated
non-goal from this document's own "Explicit non-goals" section on `helm
repo` management). No OCI Distribution protocol reimplementation — `oci-
client` is the transport, full stop, per this document's own OCI-VIA-
CRATE-ONLY invariant.

**Architecture/invariants**: `oci::pull`'s only interface to the rest of
`crates/helm-engine` is "bytes in (a reference string), chart bytes out
(handed to M9B-B.1's loader)" — it never exposes `oci-client`'s own types
across the module boundary, mirroring M9B-A.1's own crate-boundary
discipline applied to a new external dependency rather than a new
internal module. Reference normalization (`+`→`_` for tags) must match
real Helm's *observed* behavior exactly (differential-tested, not derived
from an unread source line) since a mismatch here silently breaks interop
with charts already pushed by real Helm.

**Security constraints**: a pulled chart's bytes flow through the exact
same bounded-decompression/path-traversal-refusal discipline M9B-B.1
already established for local tarballs — an OCI-sourced chart is
*more*, not less, untrusted than a local fixture, so this slice adds no
new leniency, only a new transport in front of the same validation.
Registry authentication credentials (if any are needed for the isolated
test cluster's own registry) are never logged, journaled, or included in
any error message — a dedicated `error_display_never_embeds_raw_payload_
content`-style test extended to cover credential material specifically.

**Mutation-gateway requirements**: none — pulling a chart is a read-only
network operation, not a cluster mutation; it does not flow through
`kube::mutation` at all (a chart pull is not a Kubernetes API call).

**Exact stop-and-ask decisions**: (1) `oci-client`'s real-world
production-adoption risk beyond WASM-runtime (krustlet) contexts
(research §28.1/§40 item 3, explicitly flagged **UNVERIFIED** by the
research) — resolve with either direct evidence gathered during this
slice's own implementation experience, or an explicit written acceptance
of the residual risk, before treating `oci-client` as a permanently
settled dependency choice. (2) `helm2oci`'s actual scope/maturity
(research §28.1/§40 item 4, flagged **UNVERIFIED**) — worth a follow-up
look specifically for tarball↔OCI-artifact conversion if it turns out to
simplify this slice's own `oci::pull`/M9B-B.10's `oci::push` work; decide
whether that follow-up happens before or is skipped, don't leave it
silently unconsidered.

**Unit tests**: reference-string parsing/normalization (`+`↔`_`
conversion) against a table of known-good real-Helm-pushed tag examples;
legacy-media-type (`application/tar+gzip`) acceptance on pull, proven
against a fixture using the legacy type.

**Fake-HTTP tests**: `oci::pull` issues the expected manifest+blob-pull
call sequence against a fake OCI-registry-shaped endpoint (mirroring
`oci-client`'s own test conventions where possible); a registry auth
failure surfaces an explicit, named error, never retried silently.

**Live tests**: pull a real chart from a real OCI registry (a disposable/
test registry, or a well-known public one used read-only, decided at
implementation time) via the native engine, and confirm the resulting
chart bytes parse identically to what `helm pull oci://...` itself
produces for the same reference.

**Differential Helm-oracle tests**: a chart previously pushed by the real
`helm` CLI to an OCI registry, pulled by both the native engine and real
`helm pull`, decodes identically — MH16's own "byte-identically" stop
condition, pull half.

**Failure-injection tests**: a registry returning a malformed/unexpected
manifest (wrong media type, missing config blob) is refused explicitly,
never partially accepted; a network failure mid-pull surfaces an explicit
ambiguous-or-failed outcome, never silently retried (this document's own
inherited no-hidden-retry divergence, restated for OCI transport).

**Acceptance criteria**: research MH16's own stop condition, pull half:
"a chart pushed by [real Helm] round-trips through a real `helm pull
oci://...`... byte-identically" when pulled instead by the native engine.

**Documentation updates**: this document's Journal, including the two
stop-and-ask resolutions above; `crates/helm-engine`'s own module-level
doc comments for `oci::pull`/`oci::reference`.

**Commit/tag expectations**: no tag.

### M9B-B.10 — OCI push integration if justified

**Purpose**: push a chart tarball to an OCI registry via `oci-client`,
the inverse of M9B-B.9 — **conditional on M9B-B.0's own written decision
that push is actually needed** (this document's own non-goal section
already flags that pull and push are separately justified scope calls,
per research §39 item 3). If M9B-B.0 (or a later, equally explicit
decision recorded in this document's Journal) concludes push is not
needed, this slice is skipped entirely, recorded as "declined, not
deferred," mirroring this project's own established convention for
explicitly-declined scope (e.g. M9.6's own deferral write-up style)
rather than silently vanishing from the ledger. Corresponds to research
MH16's push half.

**Scope** (if undertaken): `oci::push` (config-blob derivation from a
chart's `Chart.yaml` — a fresh JSON marshal of the unpacked metadata, not
the raw tarball, per research §27's own finding — layer push via
`oci-client`, deterministic layer ordering by digest, provenance-layer
push if M9B-B.11 lands first); `oci::annotations` (the standard
`org.opencontainers.image.*` keys plus the two Helm-invented immutable
keys, `AnnotationVersion`/`AnnotationTitle`, protected from override by
chart-supplied custom annotations, per research §27.1).

**Explicit non-goals**: never push/publish anything against a real,
shared registry without explicit per-invocation authorization — this
slice's own safety boundary is at least as strict as this document's
general "never push/publish without explicit authorization" rule, made
concrete here because push is the one M9B-B action whose effect is
externally visible to other consumers of a registry, not merely to the
target Kubernetes cluster.

**Architecture/invariants**: strict-mode reference-vs-`Chart.yaml`
name/version validation is reproduced exactly (research §27's own
finding) — a push whose target reference disagrees with the chart's own
declared identity is refused, never silently reconciled by picking one
value over the other.

**Security constraints**: registry write credentials are never logged/
journaled, mirroring M9B-B.9's own credential-handling discipline; a
push failure partway through (manifest pushed but not all blobs, or vice
versa) must be surfaced as an explicit ambiguous-or-failed outcome —
`oci-client`'s own atomicity guarantees (or lack thereof) for a partial
push must be understood and documented here, not assumed safe.

**Mutation-gateway requirements**: an OCI push, unlike a chart pull, is
an external-effect-producing write — even though it is not a Kubernetes
API call and therefore does not literally route through `kube::mutation`,
it is treated with the *same seriousness* (explicit confirmation, no
silent retry, journaled intent/outcome) as a cluster mutation, per this
document's own restated Safety boundary. Whether this is implemented as
a literal extension of the `mutation::workflow`/`kube::mutation` gateway
or as a structurally parallel but equally-disciplined confirmation path
is a stop-and-ask decision, not a default.

**Exact stop-and-ask decisions**: (1) is push in scope at all — the
overriding decision this entire slice is conditioned on, per M9B-B.0.
(2) if in scope: does push reuse `kube::mutation`'s literal machinery, or
a parallel-but-equally-disciplined confirmation/journal path for a
non-Kubernetes-API write — resolve before implementation, since it
affects whether this slice counts as "the design test" fails or holds.

**Unit tests** (if undertaken): config-blob JSON derivation matches
real Helm's own; strict-mode name/version-vs-tag mismatch refusal;
immutable-annotation protection against a chart supplying its own
conflicting `AnnotationVersion`/`AnnotationTitle`.

**Fake-HTTP tests** (if undertaken): push issues the expected manifest+
blob-push sequence, deterministic layer ordering, against a fake
OCI-registry-shaped endpoint.

**Live tests** (if undertaken): push a chart to a real/disposable
registry via the native engine, then pull it back via the real `helm`
CLI, confirming it decodes identically.

**Differential Helm-oracle tests** (if undertaken): a chart pushed by the
native engine, pulled by real `helm pull`, decodes identically; and the
reverse (real Helm pushes, native engine pulls, from M9B-B.9) — both
directions of the round trip, per MH16's own stated stop condition.

**Failure-injection tests** (if undertaken): a partial-push failure
(manifest succeeds, a blob fails) leaves the registry in a state the
engine can detect and report explicitly on a follow-up attempt, never
silently assumed complete.

**Acceptance criteria**: either (a) this slice is explicitly declined,
recorded with its own written reason in this document's Journal, and the
milestone proceeds to M9B-B.11/B.12 without it; or (b) research MH16's
own stop condition, verbatim: "a chart pushed by the native engine
round-trips through a real `helm pull oci://...`, and vice versa,
byte-identically."

**Documentation updates**: this document's Journal, recording whichever
of (a)/(b) above actually happened.

**Commit/tag expectations**: no tag.

### M9B-B.11 — Provenance / verification path

**Purpose**: implement classic-chart-repo provenance/PGP verification
(research §26.1) — parsing a `.prov` file's PGP-clearsigned
chart-metadata + `SumCollection` (per-file SHA-256 digests), verifying
the signature against a loaded keyring, and recomputing/comparing the
tarball's own SHA-256 against the recorded digest. Corresponds to
research MH15.

**Scope**: `provenance::parse` (RFC 4880 clearsign framing, dash-
escaping, hash-algorithm armor headers — via a mature Rust OpenPGP crate,
`sequoia-openpgp` or `pgp`, decided here, never hand-rolled cryptography);
`provenance::verify` (signature-against-keyring check, SHA-256 recompute-
and-compare against the recorded `SumCollection`); classic repo-index
(`index.yaml`) parsing (research §26.1's own `IndexFile` model —
`serde_yaml` plus a semver crate, per research §37 Tier 2's own finding
that this "barely counts as via a crate rather than trivial to
hand-write") **only** to the extent needed to resolve a `repo/chart`-
shorthand reference the test harness provides directly — never repo
*management* (add/update/list), restated non-goal below.

**Explicit non-goals**: no `helm repo add/update/index` (restated,
binding, from this document's own "Explicit non-goals" section) — this
slice consumes an already-known repo URL/index the test harness supplies,
it does not manage a local repo cache. No classic-repo *fetch-and-cache*
persistence layer — fetching a chart tarball plus its `.prov` file given
a known URL is in scope; maintaining a `repositories.yaml`-equivalent
local registry of known repos is not.

**Architecture/invariants**: provenance verification is a pure,
self-contained check — signature valid, digest matches, or explicit
refusal; there is no partial-trust state ("signature valid but digest
mismatch" must refuse the chart entirely, never proceed with a warning).
The OpenPGP keyring itself (which public keys are trusted) is an explicit,
externally-supplied input to this module, never a hardcoded or
auto-discovered set — mirroring this document's own "no silent chart-
acquisition fallback" invariant applied to trust material specifically.

**Security constraints**: this is the first M9B-B slice whose entire
purpose is a security boundary (verifying chart authenticity/integrity)
rather than a compatibility one — it must be reviewed with that framing
explicitly, not folded into the same "just another chart-loading
concern" bucket as M9B-B.1. A verification failure is always fail-closed
(chart refused), and the specific failure reason (bad signature vs.
digest mismatch vs. untrusted key vs. malformed `.prov`) is surfaced
distinctly enough to be actionable without ever exposing raw key material
or unrelated keyring contents in an error message.

**Mutation-gateway requirements**: none — verification is a read-only,
pre-install check, not a cluster mutation.

**Exact stop-and-ask decisions**: (1) `sequoia-openpgp` vs. `pgp` (or
another mature OpenPGP crate) — decide and record the specific crate
choice before implementation, with the reasoning (maintenance status,
clearsign-support completeness, license compatibility) recorded here, not
assumed. (2) whether provenance verification is a hard gate (a chart
without a valid `.prov` when one was expected is refused outright) or an
advisory warning users can override — Helm's own `VerificationStrategy`
enum (`VerifyNever`/`VerifyIfPossible`/`VerifyAlways`/`VerifyLater`,
research §26.1) offers a spectrum; decide which of these SAUR-ON exposes,
if any beyond the strictest option, before implementing — the default
posture leans toward the strictest available option per this project's
own general security-conservatism precedent, but this needs explicit
sign-off, not an assumed default.

**Unit tests**: clearsign parsing against real, hand-fetched `.prov`
fixtures (from real charts, not synthetic ones, since RFC 4880 framing
has enough edge-case surface — dash-escaping, multi-line armor headers —
that synthetic fixtures risk testing the engine's own assumptions rather
than real-world format variance); signature-verification success and
failure paths (tampered signature, wrong key); digest-mismatch detection
(tampered tarball, correct signature).

**Fake-HTTP tests**: `index.yaml` parsing against a fixture repo index
(JSON and YAML variants, per research §26.1's own "auto-detects JSON vs.
YAML" finding); repo-shorthand resolution against a fixture index.

**Live tests**: fetch a real chart plus its real `.prov` file from a real
(or disposable, mirrored) classic chart repository, verify it natively,
and confirm the verification result agrees with what `helm pull --verify`
itself reports for the same chart.

**Differential Helm-oracle tests**: a real chart's `.prov` file, verified
by both the native engine and real Helm's own `provenance.Verify`
(exercised via `helm pull --verify` or `helm verify`), agree on
signature validity and digest match for both a genuine and a
deliberately-tampered fixture.

**Failure-injection tests**: a malformed `.prov` file (corrupted armor
headers, truncated clearsign block) is refused with a named parse error,
never partially trusted; an expired/revoked key in the keyring (if the
chosen OpenPGP crate surfaces revocation status) is treated as untrusted,
not merely "signature technically verifies."

**Acceptance criteria**: research MH15's own stop condition, verbatim:
"a chart fetched+verified natively decodes/hashes identically to what
`helm pull` would produce for the same URL," for both a genuine and a
deliberately-tampered fixture (the tampered case must be *refused*
identically to how real Helm refuses it, not merely "also fails" for a
different reason).

**Documentation updates**: this document's Journal, including the
OpenPGP-crate and verification-strictness stop-and-ask resolutions.

**Commit/tag expectations**: no tag.

### M9B-B.12 — Full differential compatibility acceptance / regression / soak

**Purpose**: the milestone's combined acceptance, mirroring every prior
milestone's own final-slice structure (M8B.7, M9.7, M9B-A.11) and
research §39's own full-surface scope: run the full extended differential
fixture matrix (research §14's original fixtures, still applicable via
M9B-A, plus this document's own new fixtures #23-#29+ from M9B-B.7/B.8,
plus M9B-B.9-B.11's own OCI/provenance round-trip fixtures) as a
dedicated live-cluster suite against both pinned `helm` v3.x and v4.x
binaries, plus full M1-M9B-A regression, plus a soak.

**Scope**: `scripts/test-cluster-m9b-b.sh` (or equivalent, mirroring
`test-cluster-m9b-a.sh`'s own naming convention) with its own guarded
Docker/API identity check, `m9b-b-fixtures`/`m9b-b-reset`/`m9b-b-test`
cases, as a sibling script to (never sharing code with)
`test-cluster-m9.sh`/`test-cluster-m9b-a.sh`; the real-world chart corpus
from M9B-B.5, re-run at full scale (install/upgrade, not just render);
full regression of every existing `accept-*.py` script through M9B-A (or
whatever M10+ has added by the time this milestone is actually
implemented — regression scope is "everything accepted so far," not a
fixed list frozen at planning time, restated from M9B-A.11's own
convention); a soak (duration TBD, 75 minutes proposed as the established
convention from M8.6/M8B.7/M9B-A.11) rotating through guarded install/
upgrade/template operations with the same self-restoring/no-drift
discipline every prior soak established.

**Explicit non-goals**: no new engine capability — this slice proves
what M9B-B.0-B.11 already built, it does not add to it. If a gap is found
here, it is a bug (per this project's own bug-discipline convention:
reproduce, classify, root cause, regression-proof, full locked recheck,
replay, only then continue), not a scope addition.

**Architecture/invariants**: none new; this slice validates every
invariant in this document's own Invariants section, end to end,
including RENDER DETERMINISM (the corpus must render identically across
repeated runs within this slice's own soak, not merely once) and the
RENDERING GATE (a final confirmation that install/upgrade's own
acceptance never rested on a renderer result weaker than M9B-B.0's
decided threshold).

**Security constraints**: reconfirms that no journal entry, log line, or
error message produced across the full fixture matrix — including
rendered manifest content, coalesced values, OCI credentials, and PGP key
material — ever leaks unredacted, a dedicated grep/audit pass across the
full soak's own captured output, mirroring M9B-A.11's own equivalent
audit, extended to cover this milestone's new content classes.

**Mutation-gateway requirements**: reconfirms zero second-gateway
behavior across the full fixture matrix, including the OCI-push
confirmation path from M9B-B.10 if that slice was undertaken.

**Exact stop-and-ask decisions**: whether any fixture requiring a feature
not yet built by this point is possible at all (it should not be, since
M9B-B.0-B.11 precede this slice) — if one is found, stop, because it
indicates a scope gap earlier in this ledger, not something to patch at
acceptance time; mirrors M9B-A.11's own identical decision.

**Unit / fake-HTTP tests**: none new; this slice is live/differential/
regression, reusing every unit/fake-HTTP test already accumulated.

**Live tests**: the full extended fixture matrix, run twice per this
project's own "run combined acceptance twice" convention (M9.7, M8B.7,
M9B-A.11).

**Differential Helm-oracle tests**: every fixture in research §14 plus
this document's own new fixtures against both pinned `helm` v3.x and
v4.x binaries — this document's own stop condition, mirroring MH8's
verbatim structure: "every fixture passes for both oracle versions; any
fixture requiring a feature not yet built is an explicit, named refusal,
never a silent pass," extended to also require the real-world chart
corpus renders/installs/upgrades identically (or with every divergence
individually documented) across both oracle versions.

**Failure-injection tests**: the crash-mid-write experiment convention
(M9B-A.5/A.11's own precedent), repeated at full-system scope for
install/upgrade specifically, if not already fully closed at M9B-B.7/B.8.

**Acceptance criteria**: the stop condition above (verbatim); full
M1-M9B-A (or current) regression green; two clean combined-acceptance
runs; a soak completed with the same "observed stability only, no
leak-freedom claims" honesty discipline every prior soak used; clean
working tree; `fmt`/`clippy -D warnings` clean across the whole
workspace including every `crates/helm-engine` module this milestone
added.

**Documentation updates**: `HANDBOOK.md`, `docs/RUNBOOK.md`, `README.md`,
this document — reconciled, per every prior milestone's own final-slice
convention; `docs/M9B_B_RENDERER_MATRIX.md` finalized as the milestone's
own durable compatibility record.

**Commit/tag expectations**: local annotated tag `m9b-b-accepted`, never
pushed without explicit authorization — mirroring `m9-accepted`/
`m9b-a-accepted`'s own convention exactly.

## Compatibility contract (restated from the brief's own required section)

Differential testing against real Helm is a first-class acceptance
requirement threaded through **every** slice above from M9B-B.1 onward,
never a final smoke test bolted on at M9B-B.12 — restated verbatim in
spirit from `docs/M9B_A_ACCEPTANCE.md`'s own equivalent section, extended
here for chart-execution-specific comparison axes.

Once rendering exists (M9B-B.3 onward), and once install/upgrade exist
(M9B-B.7/B.8), differential tests must compare **real Helm install/
upgrade vs. SAUR-ON native install/upgrade** on, at minimum: rendered
resources (full spec diff, not presence/absence); creation ordering
(install-order kind-sort, reused from M9B-A.4); CRD behavior (creation
timing, discovery-cache invalidation, permanence); hooks (execution,
ordering, delete-policy, log capture per M9B-A.7's own inherited
security decision); storage records (verified by decoding with the
*real* `helm get` commands, never self-decode-only, per this document's
own inherited "self-decode-only is not sufficient evidence" rule);
revision numbering; values (coalesced tree shape, `reuseValues` mode
outcomes); status transitions; `managedFields` (manager name and
operation type, Apply vs. Update — the CSA/SSA regime detection M9B-A.8
already built, now exercised by a larger fraction of the surface per
research §39's own finding that this "confirms the existing [compatibility
risk] category is load-bearing for a larger fraction of the surface");
SSA/CSA semantics; failures; rollback compatibility (via M9B-A's own
rollback, for `--atomic`); and, as the **critical follow-up gating every
fixture**, whether the real `helm` CLI can still successfully operate on
the release afterward (`helm status`/`helm history`/`helm rollback`/
`helm upgrade`/`helm uninstall`, used in the test harness only, never at
runtime).

**LOOKS EQUIVALENT ≠ BEHAVES EQUIVALENT** remains the single organizing
principle, restated verbatim from M9B-A: producing the same apparent
end state (a chart that "looks installed") is never the acceptance bar;
remaining compatible with Helm's own expectations (so the real `helm`
CLI can keep operating on a release SAUR-ON touched, and so a chart
authored for real Helm renders the same way through SAUR-ON's own
renderer) is.

**Renderer-gating fixtures**: the deterministic-rendering-fixture corpus
(M9B-B.3/B.4) plus the real-world chart corpus (M9B-B.5) gate whether
install/upgrade may begin implementation at all, per the RENDERING GATE
invariant — this is a *stronger* gating relationship than research §14's
original CSA/SSA/hook-gating-fixture structure, since a failing renderer
fixture blocks an entire downstream tier of slices, not just one slice's
own acceptance.

**Install-gating fixtures**: this document's own new fixtures #23-#25
(M9B-B.7) gate install compatibility, independent of whether OCI/
provenance (M9B-B.9-B.11) have landed.

**Upgrade-gating fixtures**: this document's own new fixtures #26-#29
(M9B-B.8) gate upgrade compatibility, and additionally require M9B-A.6's
rollback (for `--atomic`) to already be accepted.

**OCI/provenance-gating fixtures**: M9B-B.9-B.11's own round-trip
fixtures gate their respective sub-scopes independently of install/
upgrade — per research §39's own finding that OCI/pull-push carry no
dependency edge to/from the rendering/install/upgrade track.

## Live-cluster strategy

Mirrors `docs/M9B_A_ACCEPTANCE.md`'s own resolved live-test strategy
(dedicated, disposable cluster, never the primary regression cluster),
extended for chart-execution's own new needs. **Decision deferred to
M9B-B.0, not guessed here**: whether M9B-B reuses M9B-A's own
`sauron-m9b-a` cluster (if that milestone stood up a dedicated one) or
requires its own `sauron-m9b-b` cluster, given install/upgrade's own
CRD-creation and cluster-scoped-resource footprint, which is a materially
larger blast radius than rollback/uninstall's own release-scoped
operations. Whichever is decided, the same non-heuristic, externally-
proven identity-guard-script convention (`test-cluster-m9.sh`'s own
three-part check: kubeconfig existence, Docker label match, API-server-
URL match) must be replicated for this milestone's own script, as a
sibling script, never a shared/parameterized one.

Both pinned `helm` CLI binaries (v3.x and v4.x, decided at M9B-A.0, reused
unchanged here) must be available wherever live/differential tests run.
Additionally, M9B-B's own live-cluster strategy must account for: (1) the
real-world chart corpus (Bitnami, ingress-nginx, cert-manager,
prometheus-community, or whatever M9B-B.0 finalizes) being vendored/
pinned at specific versions, not floated against upstream's own latest —
an ongoing maintenance cost distinct from, and additional to, the `helm`
binary-pinning cost research §19.6 already flags; (2) if M9B-B.9-B.11
land, a disposable or read-only-mirrored OCI registry and/or classic
chart repository reachable from the test harness, itself its own small
piece of test infrastructure this document does not assume already
exists.

## Security model (restated, consolidated)

Builds directly on M9B-A's already-accepted (once implemented) model,
widened by: chart-execution's own new content classes subject to the
"never enter Object/Store/Timeline/graph/journal unredacted" discipline
(rendered manifest bodies, coalesced values, chart-embedded Secrets —
restated from research §14 fixture #3's own finding, now applicable to
install/upgrade's *own* newly-rendered manifests, not just a stored one);
M9B-B.1's own new untrusted-input boundary (chart archive loading —
bounded decompression, path-traversal refusal); M9B-B.3's own new
untrusted-input boundary (template recursion-depth limiting for `tpl`/
`include`); M9B-B.9/B.10's own credential-handling discipline for OCI
registry auth; M9B-B.11's own dedicated security-boundary review for PGP
provenance verification (fail-closed, no partial trust). Every M9.5/
M9B-A guarantee this document does not explicitly widen is inherited
unchanged, never relaxed, restated by reference rather than re-derived at
implementation time.

## Documentation obligations

`HANDBOOK.md`, `docs/RUNBOOK.md`, `README.md`, this document, and
`docs/M9B_B_RENDERER_MATRIX.md` are reconciled only at M9B-B.12 (this
milestone's final slice), mirroring every prior milestone's own
convention of updating shared documentation once at combined acceptance
rather than incrementally per slice — except where a slice's own contract
above explicitly calls out an earlier documentation update (e.g. every
slice's own compatibility-matrix population, which must exist in
`docs/M9B_B_RENDERER_MATRIX.md` at that slice, not deferred; M9B-B.0's
own four stop-and-ask decisions, which must exist in this document's
Journal before M9B-B.1 begins).

## Architectural questions that MUST be resolved before implementation

Populated from `docs/NATIVE_HELM_ENGINE_RESEARCH.md`'s §40 explicit
unknowns list (this extension's own dedicated unknowns section), plus
this document's own newly-introduced stop-and-ask decisions restated here
for completeness. Cross-referenced explicitly against
`docs/M9B_A_ACCEPTANCE.md`'s own "Architectural questions" section where
overlap exists. None of these are filled in with guesses anywhere in this
document.

1. **`--dry-run=server`'s exact wire-level mechanics inside `pkg/kube/
   client.go`** (research §40 item 1, carried forward from
   `docs/M9B_A_ACCEPTANCE.md`'s own item 7, which flagged it as relevant
   "only if M9B-A.9 ... needs [it]"). **This is the M9B-B-specific
   restatement**: confirmed that the real `KubeClient`/`IsReachable()`
   are used and the render→build→apply path is identical to a real
   install/upgrade, but not confirmed exactly how a `metav1`-style
   dry-run parameter attaches to the outgoing API-server request.
   **Blocks** M9B-B.7/B.8 only if their `--dry-run=server`-equivalent
   preview needs byte-level wire equivalence to Helm's own mechanism
   rather than reusing `kube-rs`'s own existing dry-run parameter (which
   M9B-B.7's own contract already prefers, per the same M9-reuse
   discipline M9B-A established). **Resolved by**: a direct wire-level
   check only if a concrete discrepancy surfaces during M9B-B.7/B.8
   implementation; otherwise not required.
2. **Exact location of Helm's `+`/`_` OCI-tag-normalization logic within
   `pkg/registry`** (research §40 item 2) — confirmed it happens inside
   `Pull`/`Push`, not confirmed which specific file/function owns it.
   **Blocks** M9B-B.9/B.10 only from citing an exact upstream counterpart
   in code comments; does not block behavioral implementation, since
   M9B-B.9's own contract already commits to differential-testing this
   logic against real Helm's *observed* behavior rather than a source
   citation. **Resolved by**: either locating the exact source line
   during implementation (a cheap, opportunistic closure) or accepting
   behavioral-only evidence as sufficient, decided explicitly at M9B-B.9,
   not left ambiguous.
3. **`oci-client`'s real-world production adoption beyond WASM-runtime
   (krustlet) contexts** (research §40 item 3, flagged **UNVERIFIED**).
   **Blocks** M9B-B.9/B.10 from treating `oci-client` as a permanently
   settled dependency choice without residual risk acknowledged —
   restated as this document's own OCI-VIA-CRATE-ONLY invariant's own
   explicit escape hatch ("unless later evidence invalidates this
   choice"). **Does not block** starting M9B-B.9 — the research's own
   §37 Tier 2 verdict is to use it regardless, with this risk
   *acknowledged*, not *resolved*, before implementation. **Resolved
   by**: direct implementation experience during M9B-B.9/B.10 (does it
   work against real-world registries — Docker Hub, GHCR, ACR/ECR — as
   well as it works in krustlet's own context), recorded in this
   document's Journal either way.
4. **`helm2oci` crate's scope/maturity** (research §40 item 4, flagged
   **UNVERIFIED**). **Blocks** nothing directly — it is a candidate
   convenience, not a required dependency (M9B-B.9/B.10's own scope
   already includes the tarball↔config-blob derivation logic natively).
   **Resolved by**: an explicit follow-up look during M9B-B.9/B.10 only
   if `helm2oci` might simplify that work; skipped without further
   research otherwise, per this document's own M9B-B.9 stop-and-ask
   decision (2).
5. **The exact default `Info.Description` string Helm writes for a
   rollback record** (research §40 item 5) — **this is M9B-A's own item,
   restated here only because `docs/M9B_A_ACCEPTANCE.md`'s own
   Architectural-questions item 9 already owns and resolves its blocking
   condition** (does not block anything load-bearing, since M9B-A.9
   already recommends matching Helm's flat history list without a
   derived description column). **M9B-B does not reopen this item** —
   listed here only for cross-reference completeness, per the task's own
   instruction to state explicitly which unknowns overlap with M9B-A's
   document versus which are new.
6. **No new live experiments were run in the research's own extension
   pass** (research §40 item 6) — restated here as a standing reminder
   that this document's own claims about OCI/`--dry-run=server` mechanics
   are [SOURCE]/[DOCS]-derived, not independently live-verified prior to
   this document's own authoring; M9B-B.9/B.11's own live-test
   requirements are exactly the mechanism that closes this gap during
   implementation, not before.
7. **Sprig function coverage gaps — the long tail** (research §25.5's own
   flagged crypto-PRNG/regex-flavor/locale-casing costs; **new to
   M9B-B, no M9B-A overlap**, since M9B-A never renders anything).
   **Blocks** M9B-B.4's own completion claim from being unconditional —
   restated here as the document-level tracking pointer for what M9B-B.4's
   own per-function matrix rows must individually resolve, function by
   function, not as a single blanket unknown. **Resolved by**: the
   compatibility matrix itself, populated at M9B-B.4, function by
   function — this item closes incrementally, not in one step.
8. **Map-iteration-order determinism strategy for `range`-over-map
   constructs** (this document's own architectural note at M9B-B.3,
   **new to M9B-B**, no research-document citation — an implementation-
   level unknown this document surfaces itself, not inherited). **Blocks**
   M9B-B.3 from claiming RENDER DETERMINISM holds until resolved.
   **Resolved by**: an explicit, recorded ordering-strategy decision
   (e.g. sorted-keys iteration) made and tested at M9B-B.3, before any
   fixture depending on map-iteration order is written.
9. **CSA/SSA managedFields regime detection's applicability to install's
   own resource-creation path** (overlap note, cross-referenced against
   `docs/M9B_A_ACCEPTANCE.md`'s own item 4, the `csaupgrade.
   UpgradeManagedFields` unknown) — M9B-A.8 already owns closing this for
   rollback/uninstall; **M9B-B.7/B.8 reuse that closed primitive
   verbatim** and do not reopen the underlying unknown, but this
   document notes explicitly that install/upgrade exercise the
   CSA/SSA-detection code path against a *larger* variety of resource
   shapes (freshly-created resources, not just previously-rolled-back
   ones) — if M9B-B.7/B.8's own differential fixtures surface a case
   M9B-A.8's own fixture #22 did not cover, that is a **new finding
   against an M9B-A primitive**, to be fixed inside M9B-A's own module
   under M9B-A's own acceptance discipline, per this document's own
   "Relationship between M9B-A and M9B-B" section — never patched around
   locally inside M9B-B.

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
  converting `docs/NATIVE_HELM_ENGINE_RESEARCH.md`'s chart-execution
  extension findings (§22-40, MH10-MH18) into an M9B-B slice ledger per
  explicit request, mirroring `docs/M9B_A_ACCEPTANCE.md`'s own structure
  and rigor and stating explicitly that M9B-B depends on M9B-A's accepted
  primitives while M9B-A must never depend on M9B-B. Status:
  **PLANNED — NOT STARTED**. No code touched. No cluster touched. No
  commit made as part of authoring this document beyond what the
  requesting instruction explicitly permitted. M10 is not blocked by this
  document's existence and may proceed independently, and neither M9B
  milestone is authorized to begin implementation by this document's
  existence alone.

## Final milestone acceptance checklist

To be completed only when M9B-B implementation actually finishes — listed
here now as the target bar, not as a claim of current status:

- [ ] `docs/M9B_A_ACCEPTANCE.md` confirmed ACCEPTED (all M9B-A.0-A.11
  slices) before M9B-B implementation begins — restated here as the
  literal enforcement point of "M9B-B depends on M9B-A's accepted
  primitives," not merely asserted in prose.
- [ ] M9B-B.0-B.12 all ACCEPTED (or, for M9B-B.10, explicitly and
  deliberately declined per its own conditional scope), each with its
  own recorded evidence in this document's Journal (unit/fake-HTTP/live/
  differential counts, mirroring every prior milestone's own journal-
  entry style).
- [ ] The renderer-compatibility-gate threshold decided at M9B-B.0 is met
  or exceeded by M9B-B.5's own corpus results, recorded explicitly, not
  summarized as "mostly works" — and this result was checked *before*
  M9B-B.6 onward began, not retroactively.
- [ ] `docs/M9B_B_RENDERER_MATRIX.md` fully populated (every Sprig
  function, every Go-template control construct, every Helm-added
  function, every object-model field marked implemented-tested /
  implemented-with-documented-divergence / explicitly-refused).
- [ ] The full extended differential fixture matrix (research §14's
  original fixtures via M9B-A, plus this document's own new fixtures)
  passes against both a pinned `helm` v3.x and v4.x oracle, with any
  deliberately-out-of-scope fixture refused explicitly, never silently
  skipped.
- [ ] The "subsequent real `helm` CLI compatibility" critical test passes
  for install and upgrade, not just rollback/uninstall.
- [ ] Every documented divergence from Helm upstream (inherited from
  M9B-A, plus any new one this milestone's own implementation proposed
  and recorded under "Deliberate divergences" above) is proven by a
  dedicated test, not merely asserted in prose.
- [ ] Full M1-M9B-A (or current) regression green, run via every existing
  `accept-*.py` script unmodified.
- [ ] Combined acceptance run twice, clean.
- [ ] A soak completed with the same "observed stability only" honesty
  discipline every prior soak used.
- [ ] `cargo fmt --check` / `cargo clippy --all-targets -D warnings`
  clean across the whole workspace, including every module this
  milestone added to `crates/helm-engine`.
- [ ] `HANDBOOK.md`, `docs/RUNBOOK.md`, `README.md`, this document,
  `docs/M9B_B_RENDERER_MATRIX.md` reconciled.
- [ ] Working tree clean.
- [ ] Local annotated tag `m9b-b-accepted` created, never pushed without
  explicit authorization.
- [ ] M9B-B.10's disposition (implemented, or explicitly declined) is
  recorded unambiguously — this checklist item exists so the milestone
  cannot be marked complete with that slice silently unresolved.
