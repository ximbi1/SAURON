# M8B — Advanced Cluster Operations: planning ledger (PLANNED, NOT STARTED)

Status: **PLANNED / NOT STARTED**. No slice below is ACCEPTED, no code
exists yet, no fixtures exist yet, no tests exist yet. This document is
scope definition and contract design only, written while M8.6's soak was
still running, specifically so implementation does not start until this
plan is reviewed. Nothing in this file authorizes touching code, cluster,
or CI.

## Purpose

M8 shipped the first guarded, user-facing single-object mutations (Scale,
Restart, Delete, Label, Annotate) on top of M7's policy/confirmation/
execution/journal infrastructure. M8B extends that same set of *semantic,
first-class* operations to cover the next tier of legitimate cluster
operator actions that still fit the "one target, one clear semantic
change, exactly one MutationIntent" model M7/M8 established. M8B is not a
kubectl-replacement layer and not a generic patch console — every
operation below is a named, constrained, reviewed workflow, exactly like
Scale/Restart/Delete/Label/Annotate were.

## Relationship to M7/M8

M8B reuses, unmodified in spirit, everything M7/M8 already built:

- `mutation::{MutationIntent, MutationTarget, MutationEffect, MutationRisk,
  PolicyReason, PolicyDecision, PolicyEvaluation, ConfirmationRequirement,
  Confirmation, MutationOutcome, Verification}` — the pure model. M8B adds
  new `source_action` values and, where genuinely needed, new `PolicyReason`
  variants (e.g. a Node-cordon-specific reason) — it does not fork the
  model.
- `mutation::policy::evaluate` — the single policy engine. M8B operations
  are new *inputs* to this engine (new kinds, new risk classifications),
  never a parallel decision path.
- `mutation::workflow::{Built, Workflow}` — the same shared shell
  (`intent`, `payload`, `change`, `evaluation`, `dry_run`, `armed`,
  `commit`, `verification`). Every M8B builder returns a `Built` exactly
  like `scale`/`restart`/`delete`/`label`/`annotate` do.
- `kube::mutation::{preflight, commit, verify}` — the single execution
  gateway and the single verification entry point. M8B must not add a
  second gateway, a second TOCTOU mechanism, or a second verification
  dispatcher; `commit`/`verify` gain new `MutationEffect`/payload shapes to
  dispatch on where an operation genuinely does not fit the existing
  Modify/Delete split (see Design test, below).
- `mutation::journal::{Journal, Phase, Record}` — the same append-only,
  redacted, bounded journal. M8B may add new `Phase` variants (e.g. a
  drain's per-step progress) but never a second journal file or format.
- `Document.workflow`, the `"mutation"` keymap mode, and
  `mutation::view::workflow_report` — the same shared UI shell. Every M8B
  action renders through the same TARGET/ACTION/CHANGE/POLICY/PREFLIGHT/
  CONFIRMATION/COMMIT RESULT/VERIFICATION structure `workflow_report`
  already produces, extended only where an operation has more than one
  meaningful step (Drain; see M8B.5).
- `Command::{...}` + `parse()` grammar + `command_names()` — the same
  single command registry. Every M8B command is discoverable the same way
  `:scale`/`:label` already are; no second palette, no hidden command.
- The `ConnectOptions.mutation_test_cluster_verified` /
  `--mutation-test-cluster-verified` distinction (never `!readonly`, never
  a context-name heuristic) — unchanged, reused as-is for all M8B live
  acceptance.
- `scripts/test-cluster.sh`'s guarded Docker/API identity check, and the
  `m8-fixtures`/`m8-reset`/`m8-test` convention — M8B adds its own
  `m8b-fixtures`/`m8b-reset`/`m8b-test` cases following the exact same
  shape, in a still-to-be-created dedicated `sauron-m8b` (or similarly
  named) fixture namespace, never reusing `sauron-m8`'s objects for
  destructive/node-level operations.

### Design test (carried over from M8's own ledger, unchanged)

At the end of M8B, adding a further guarded mutation should still look
like: 1) parse the semantic user action, 2) validate its specific
arguments, 3) build a `MutationIntent` (or, for Drain, an explicit ordered
sequence of intents), 4) provide a semantic preview renderer, 5) hand it to
the existing M7 infrastructure. It should NOT require a new confirmation
subsystem, a new policy subsystem, a new journal, a new transport safety
model, a new UID model, or new TOCTOU logic. If any M8B action needs one
of those, stop and reconsider before implementing — the design is wrong.

## Safety boundary (unchanged from M8, restated for this milestone)

Production remains READ-ONLY. Every one of M8B's operations — cordon,
uncordon, drain, set image, CronJob trigger, force delete, evict — is
strictly forbidden against production, exactly like M8's five operations
were. All M8B live writes will use only the isolated `kind-sauron-test`
cluster, via `scripts/test-cluster.sh`'s verified Docker/API identity
guard, gated by the same explicit, non-heuristic
`mutation_test_cluster_verified` flag. Node-level operations (cordon/
uncordon/drain) are especially sensitive even on the isolated test
cluster, since the test cluster may be single-node (`kind` default): drain
scenarios must account for — and the acceptance plan must explicitly
document — what a single-node drain does and does not prove, rather than
silently skipping multi-node drain coverage without saying so.

## Explicit non-goals for M8B

- **Bulk operations** (multi-select delete/scale/cordon/drain, "drain all
  nodes", "delete all matching") belong to a later bulk-operations
  milestone (per the roadmap, M10) — never folded into M8B. Every M8B
  operation still targets exactly one object per intent (Drain targets one
  Node, orchestrating many single-Pod eviction intents under it — see
  M8B.5 — but the *user-facing* target is still one Node).
- **Helm/Flux/Argo/GitOps actions** (install/upgrade/uninstall,
  reconcile/suspend/resume, sync/rollback, chart or repo writes) belong to
  M9 — not touched here, not even referenced as a future extension point.
- **Plugin/headless/scripted mutation execution** belongs to M12 — M8B
  ships no non-interactive mutation entry point beyond what M8 already
  exposes (interactive TUI + the same live acceptance harness pattern).
- **kubectl passthrough / arbitrary JSON Patch or Merge Patch console /
  arbitrary YAML edit-apply / generic PodSpec editor** — still explicitly
  out of scope, exactly as M8's own non-goals stated. Set Image is a named,
  single-container, single-field operation, not a generic spec editor.
- Operations catalogued in `docs/FUTURE_MUTATIONS.md` (arbitrary apply/
  edit, Secret mutation, RBAC mutation, Namespace delete, PVC/PV
  destructive operations, a generic patch console) are explicitly NOT part
  of M8B and are not being pulled forward by this document.

## Proposed slice ledger

| Slice | Scope | Verdict |
| --- | --- | --- |
| M8B.0 | Shared advanced-operation extensions to the M7/M8 model (new `PolicyReason`s, node-target support in `MutationTarget`/`Scope`, `Job`-creation support in the executor, any new `Verification`/`Phase` variants needed across multiple operations below) | IN PROGRESS -- node-target support confirmed to need zero new code; `kube::mutation::verify`'s boolean-comparison logic extended (see M8B.1's real bug); no new `PolicyReason`/`Phase` variant added yet, none needed so far |
| M8B.1 | Cordon / Uncordon (Node) | ACCEPTED (unit/fake-HTTP/live); interactive deferred to M8B.7, matching M8.2's own precedent -- `workflow::cordon`/`workflow::uncordon` wired to `:cordon`/`:uncordon`; live-verified twice against the real (single) node of `kind-sauron-test`, briefly cordoned then immediately uncordoned in one sequential test, per explicit user sign-off |
| M8B.2 | Set image (Deployment/StatefulSet/DaemonSet, single container) | ACCEPTED (unit/fake-HTTP/live); interactive deferred to M8B.7 -- `workflow::set_image` reconstructs the full `containers` array (option (b) from this document's own open question, resolved); dedicated `verify_set_image` name-keyed comparison added; live-verified against a real two-container Deployment, unrelated sidecar proven untouched |
| M8B.3 | CronJob trigger (create a Job from a CronJob template) | ACCEPTED (unit/fake-HTTP/live); interactive deferred to M8B.7 -- first real `MutationEffect::Create` in the executor; `MutationIntent.create_resource` added; `policy::evaluate`'s Create-bypasses-confirmation rule removed (a real bug in an untested speculative rule, found while implementing this); traceability via label/annotation, not a real `ownerReference` (matches `kubectl create job --from=cronjob` semantics) |
| M8B.4 | Evict Pod (eviction subresource, PDB-aware) | PLANNED |
| M8B.5 | Drain (orchestrated cordon + bounded eviction sequence) | PLANNED |
| M8B.6 | Force delete (highest risk; explicit, narrow, no default grace=0 leakage) | PLANNED |
| M8B.7 | Combined adversarial acceptance, full M1-M8 regression, soak | PLANNED |

Ordering rationale: M8B.1-M8B.3 are the lowest-complexity, single-request
operations (closest in shape to M8's own Scale/Restart) and come first to
re-validate the shared shell against genuinely new kinds (Node) and a new
effect (Create, for CronJob trigger) before tackling anything
orchestrated. M8B.4 (Evict) is still a single request but introduces a
new subresource and a new denial class (429/PDB) that M8B.5 (Drain)
depends on internally, so Evict must land first. M8B.6 (Force delete) is
placed last among the individual operations because it is the highest-risk
single operation and benefits from every other new policy/UI pattern
(especially the "explain consequences" preview text and stronger-than-
strong confirmation question, if one is decided) already having been
proven on lower-risk operations first. This ordering can be revisited; it
is not a hard dependency chain except where explicitly noted (M8B.4 before
M8B.5).

## Per-operation contracts

Each subsection below covers: resource kinds, policy/risk classification,
preview content, confirmation requirement, TOCTOU/UID/resourceVersion
semantics, dry-run/preflight applicability, journal semantics, post-commit
verification, and required unit/fake-HTTP/live evidence. All of it is a
proposal to be reviewed, not a commitment.

### M8B.1 — Cordon / Uncordon

**Resource kinds**: Node only. No namespaced equivalent.

**Semantic treatment**: modeled as a first-class `cordon`/`uncordon`
action with its own `source_action`, not as "patch
`spec.unschedulable`" exposed through a generic mechanism. The one-field
merge patch (`{"spec":{"unschedulable":true|false}}`) is an
*implementation detail* of the builder, exactly as Scale's
`{"spec":{"replicas":N}}` is — never user-authored, never exposed as a
raw-patch entry point.

**Policy/risk**: Node is already a `cluster_critical_kind` in
`PolicyContext::default()`, so `policy::evaluate` already routes any Node
Modify through `RequireStrongerConfirmation` today, without needing a new
`PolicyReason`. Open question: should cordon/uncordon get a *distinct*
`PolicyReason` (e.g. `NodeSchedulingChange`) so the rendered POLICY block
says something more specific than the generic
`ClusterCriticalResource`/`ClusterScopedSensitiveResource` pair it would
show today? Leaning yes, for operator clarity, but not required for
correctness — the hard-deny/strong-confirmation behavior is already
correct without it.

**Preview**: TARGET (Node name, UID — Nodes have no namespace); ACTION:
`cordon` or `uncordon`; CHANGE: `spec.unschedulable: false -> true` (or
the reverse) plus, ideally, a *count* of Pods currently scheduled on the
node (informational only, matching Delete's own "currently related
objects, not a blast-radius claim" precedent from M8.3 — never phrased as
"cordoning this will evict/break N Pods", since cordon itself evicts
nothing).

**Confirmation**: Strong (Node is cluster-critical). Cordon and uncordon
both get the same strength — uncordon re-enabling scheduling on a node an
operator may not fully trust is not obviously "safer" than cordon, so no
asymmetry is proposed without a concrete reason to add one later.

**TOCTOU/UID**: identical to every other M8 Modify — `revalidate`'s
existing fresh-GET UID check applies unchanged; Nodes already flow through
the same `MutationTarget`/`Scope` shape (cluster-scoped, so
`resource.namespaced == false`, already supported by
`target.resource.namespaced.then_some(...)` throughout `kube::mutation`).

**Dry-run**: supported, identical mechanism to Scale/Restart/Label
(`dryRun=All` PATCH).

**Journal**: `summary: "spec.unschedulable"`, effect `Modify` — no new
`Phase` needed.

**Post-commit verification**: `kube::mutation::verify`'s existing
`leaf_path_and_value`-driven `Modify` path already handles this with zero
new code — `/spec/unschedulable` is exactly the same shape as `/spec/
replicas`. This is a strong signal the shared verification design
generalizes correctly.

**Evidence required**: unit (unsupported-kind rejection for anything but
Node, exact payload for both directions, hash stability); fake HTTP
(commit + verify observing `unschedulable` flip, TOCTOU replacement
rejection reusing the existing Node-as-cluster-critical policy test
pattern); live (cordon then uncordon the kind control-plane node itself —
**explicit open question**: is cordoning the *only* node in a single-node
kind cluster an acceptable live test, given it would make the node
unschedulable for new Pods for the duration of the test? Proposed
resolution: yes, briefly, immediately uncordoned in the same test,
documented as a deliberately narrow live check — but this needs explicit
sign-off before M8B.1 implementation, not assumed here).

### M8B.2 — Set image

**Resource kinds**: Deployment, StatefulSet, DaemonSet (same three-ish
family M8's Restart already covers, DaemonSet added relative to Scale's
own list since DaemonSet has no meaningful "replica count" but does have a
meaningful image).

**Semantic treatment**: `:set_image CONTAINER=IMAGE` (exact grammar TBD;
`set_image` avoids colliding with a bare `:image` that might read as "the
container runtime/registry image picker" in a future context). Requires
an exact container name and exact image reference — never "the first
container", never a fuzzy/partial match.

**Contract specifics**:
- The builder must read the CURRENT container list from the
  already-synced watch object (same "never guess, UNKNOWN != a default"
  discipline Scale's replica count already established) and reject the
  command if the named container does not exist on that object — a
  distinct, explicit error, never a silent no-op or a guess at "the
  closest name".
- If the object has more than one container and the user does not name
  one, reject with an explicit "ambiguous, name the container" error —
  never default to index 0.
- Payload is a precise, single-container merge patch:
  `{"spec":{"template":{"spec":{"containers":[{"name":"X","image":"Y"}]}}}}`
  — note this is a **merge patch on a list**, which Kubernetes handles via
  strategic merge semantics keyed on `name` for `containers` specifically
  (server-side strategic merge, not a naive JSON merge patch replacing the
  whole array). **Open question requiring resolution before
  implementation**: `kube::mutation::patch_request` currently always uses
  `Patch::Merge` (RFC 7396 JSON merge patch), which does NOT understand
  the `containers` list's `name`-keyed strategic-merge semantics — a naive
  JSON merge patch would REPLACE the entire `containers` array with a
  single-element array, silently deleting every other container. Set Image
  therefore needs either (a) `Patch::Strategic` for this one operation
  specifically (introducing the executor's first use of a different patch
  type — needs its own careful review of what that means for TOCTOU/
  dry-run/journaling), or (b) a builder-side merge that reads the full
  current container list and reconstructs the complete array with only the
  named container's image changed, keeping `Patch::Merge`. Leaning toward
  (b): it keeps the executor's patch mechanism uniform and keeps the
  "exactly what changed" story simple for the journal/preview (the
  `leaf_path_and_value` single-leaf-payload assumption `kube::mutation::
  verify` currently relies on would otherwise break for a multi-container
  array payload) — this needs to be resolved and written up before M8B.0
  is implemented, since it may require a **change to `verify`'s
  single-leaf-payload assumption** or a **deliberately different
  verification path for this one operation** (compare current image at a
  specific array index/name-match, not a bare JSON-pointer equality).

**Policy/risk**: `Routine` effect Modify, same tier as Scale/Restart —
`RequireConfirmation`, not Strong, unless the target sits in a protected
namespace or is otherwise cluster-critical/privilege-sensitive by the
existing generic rules.

**Preview**: CHANGE shows `containers["NAME"].image: OLD -> NEW` (exact
old value read fresh, never assumed).

**TOCTOU/UID**: unchanged mechanism; the *value* being TOCTOU-checked
against is more nuanced than a scalar (see verification note above).

**Dry-run**: supported.

**Journal**: `summary: "spec.template.spec.containers[\"NAME\"].image"`.

**Post-commit verification**: must read back the named container's image
specifically (by name-match within the array, not a fixed index — a
Deployment's container ordering is not a stable API guarantee) and compare
to the requested value. This is the first M8B operation that needs
`kube::mutation::verify` to grow a genuinely new comparison shape beyond
`leaf_path_and_value`'s flat-pointer equality.

**Evidence required**: unit (unknown container name rejected, ambiguous
multi-container-no-name-given rejected, exact preview old->new, unrelated
containers preserved in the constructed payload); fake HTTP (commit +
verify with a multi-container fixture proving the OTHER container's image
is untouched); live (multi-container Deployment fixture required — not
currently present in `sauron-m8`, needs a new fixture object).

### M8B.3 — CronJob trigger

**Resource kinds**: CronJob (the *source*); the operation *creates* a
Job (a new kind of target entirely).

**Semantic treatment**: `:trigger` (or `:run_now`) on a selected CronJob
creates one Job from that CronJob's `spec.jobTemplate`, mirroring what
`kubectl create job --from=cronjob/X` does, but through SAURON's own
executor — never a shell-out to `kubectl`.

**This is the first M8B operation using `MutationEffect::Create`**, which
M7's `preflight`/`commit` currently mark `MutationOutcome::Unsupported`
for, by explicit, documented design ("Create needs a full object body and
different identity semantics; out of M7's bounded scope"). M8B.3 is
therefore the slice that finally implements Create in the executor — a
real, scoped extension of `kube::mutation`, not a new parallel path:

- `preflight`/`commit` gain a real `MutationEffect::Create` branch that
  POSTs a full object body (the CronJob's `jobTemplate.spec` wrapped in a
  freshly-constructed Job manifest with a generated name, matching
  `kubectl`'s own `<cronjob-name>-<random-suffix>` convention or an
  explicit timestamp-based suffix — generated once, like Restart's
  timestamp, and reused verbatim across preview/dry-run/commit).
- **Open question**: Create has no existing object to TOCTOU-check a UID
  against (there is nothing to revalidate — the target doesn't exist yet).
  What *is* still worth revalidating before commit: that the SOURCE
  CronJob itself still exists with the same UID (so a deleted/replaced
  CronJob can't have a Job created "from" it after the fact) and that its
  `jobTemplate` hasn't changed in a way the preview didn't show. Proposed:
  `MutationTarget` for a Create carries the CronJob's UID as the
  TOCTOU-checked identity, and the created Job's identity is a *new*,
  not-yet-known fact reported only in the outcome/verification, not part
  of the pre-commit target.
- Confirmation binds to the intent exactly as before — the intent's
  "target" is the CronJob (source), not the not-yet-created Job.

**Policy/risk**: creating a Job is not inherently destructive, but running
arbitrary workload code on a schedule the operator didn't originate is not
nothing either — proposed `MutationRisk::Routine`, `RequireConfirmation`,
same tier as Scale, unless the CronJob lives in a protected namespace.

**Preview**: TARGET is the CronJob; CHANGE describes "create Job from
`jobTemplate`" plus the generated Job name that will be used (shown before
commit, since it's deterministic once generated, not created).

**Journal**: must correlate the source CronJob (its scope/UID, as the
`target`) AND the created Job's identity (name/UID, once known) — this is
the first M8B case where the journal's `Record` shape may need an
additional field (or a documented convention of putting the created
identity in `detail`, redacted-and-bounded exactly like every other
`detail` use) to hold "what was created", distinct from "what was
targeted". Needs explicit resolution: is a new `Record` field acceptable
(schema version bump) or does `detail: "created Job: sauron-m8b/nightly-abc123"`
suffice? Leaning toward the latter for now (matches the existing "detail
is a bounded, redacted string" pattern), but flagged for review.

**Post-commit verification**: fetch the newly-created Job by its
deterministic generated name and confirm it exists with an `ownerReference`
(or at least a label) pointing back at the source CronJob — a *new*
verification shape (`Verification` needs a variant like `Created` alongside
`Verified`, or `Verified` is reused with the "expected value" being "this
object exists with this owner", which doesn't fit `leaf_path_and_value`'s
model at all). This is flagged as an open design question for M8B.0.

**Evidence required**: unit (deterministic name generation, template
extraction correctness, reused-verbatim-across-preview/commit like
Restart's timestamp); fake HTTP (POST body shape, source-CronJob TOCTOU
check, created-Job verification GET); live (a real CronJob fixture,
triggered, Job observed running to completion or at least Pod-scheduled).

### M8B.4 — Evict Pod

**Resource kinds**: Pod only.

**Semantic treatment**: uses the Kubernetes **eviction subresource**
(`POST /api/v1/namespaces/NS/pods/NAME/eviction`, a `policy/v1 Eviction`
body), never a plain DELETE. This is a materially different request shape
from `kube::mutation::delete_request`, and is the mechanism Drain (M8B.5)
is required to build on rather than reimplementing.

**Contract specifics**:
- Must respect PodDisruptionBudget semantics — the API server itself
  enforces this and returns `429 Too Many Requests` when eviction would
  violate a PDB. This must classify to an explicit, distinct
  `MutationOutcome`/reporting state (proposed: reuse `Conflict` conceptually
  but the actual HTTP status is 429, not 409 — `kube::mutation::classify`
  needs a new arm; calling it `MutationOutcome::Conflict` would blur a real
  distinction operators care about, so a new variant, tentatively
  `DisruptionBudgetDenied` or a generically-named
  `Throttled`/`RequestDenied`, is proposed — exact naming deferred to
  implementation review).
- **No fallback to a plain delete if eviction fails or is denied** — ever.
  This must be tested explicitly (a fake-HTTP test asserting that a 429
  eviction response never triggers a subsequent DELETE call).
- Target UID is pinned via the same TOCTOU `revalidate` GET already used
  everywhere else; the Eviction API itself does not carry a UID
  precondition field the way `DeleteParams.preconditions` does for a plain
  delete, so client-side TOCTOU is the *only* protection here — flagged
  explicitly as a documented, narrower defense-in-depth than Delete's
  (M8.3) belt-and-suspenders (client TOCTOU + server precondition).

**Policy/risk**: `Destructive`-adjacent but not identical to Delete — an
evicted Pod is typically recreated by its controller (Deployment/
StatefulSet/DaemonSet), unlike a standalone Pod delete. Proposed:
`RequireStrongerConfirmation` regardless (matching Delete's own
"destructive effect always strong" rule), since from the user's
perspective it is still "make this specific Pod instance go away right
now", but the preview text should be honest that a controller will very
likely recreate it, unlike Delete's own Pod-kind coverage where recreation
is not guaranteed.

**Preview**: TARGET the Pod; ACTION: `evict`; CHANGE: a short description,
e.g. "evict (subject to PodDisruptionBudget)"; POLICY as usual.

**Dry-run**: the Eviction API supports `dryRun` in its request body
(`Eviction.deleteOptions` or a top-level `dryRun` query param depending on
API version — needs a quick verification against the exact kube-rs
`policy::v1::Eviction` type surface before implementation, not assumed
here).

**Journal**: effect... **open question**: is eviction `Delete` or does it
warrant its own `MutationEffect` variant? The object genuinely gets
deleted (from the scheduler's perspective) but via a fundamentally
different, PDB-checked request. Proposed: keep `MutationEffect::Delete`
for policy-classification purposes (risk/confirmation logic is the same
question either way), but the *executor* dispatches to a distinct
eviction-specific request function based on `source_action`, not effect
alone — mirroring how Restart and Label both use `MutationEffect::Modify`
today despite being semantically distinct actions.

**Post-commit verification**: fresh GET for the Pod; `NotFound` or a new
`deletionTimestamp` means the eviction is proceeding/complete
(`ObservedGone`/`DeletionInProgress`, reusing Delete's existing
`Verification` variants directly — no new variant needed here, unlike
Set Image/CronJob trigger above).

**Evidence required**: unit (PDB-denial classification, no-fallback-to-
delete proof at the unit/type level where feasible); fake HTTP (429 →
explicit denial, zero DELETE ever issued, 200 → Committed +
DeletionInProgress/Pending verification); live (a Pod fixture behind a
tight PDB — `maxUnavailable: 0` or equivalent — proving a real 429 denial,
plus a second Pod fixture with no PDB proving a real successful eviction).

### M8B.5 — Drain

**This is flagged, per the master prompt's own framing, as likely the
highest-complexity slice in M8B.** It is explicitly modeled as an
**orchestrated operation**, not a single `MutationIntent` — this is a
deliberate, documented exception to "one user action, one intent", and the
exception's shape needs to be reviewed and agreed before implementation
starts, not discovered mid-implementation.

**Resource kinds**: Node only (the target); orchestrates eviction of every
evictable Pod scheduled on that Node (M8B.4's Evict, reused, not
reimplemented).

**Proposed shape**:
1. Cordon the Node first (reusing M8B.1's cordon builder/executor path
   exactly, not a copy).
2. Enumerate Pods currently scheduled on the Node (a read, via the
   existing watch/store or a fresh bounded list — not a new collector).
3. Filter out **DaemonSet-owned Pods** (never evicted by drain — they are
   expected to keep running or be deliberately excluded, matching
   `kubectl drain`'s own default behavior; draining a DaemonSet Pod is
   explicitly out of scope for this operation, not silently attempted and
   failed).
4. Filter/flag Pods using **local storage** (`emptyDir`, hostPath) —
   `kubectl drain` requires an explicit `--delete-emptydir-data` opt-in
   because evicting such a Pod loses that data; SAURON's Drain must
   surface this as an explicit, named consideration in the preview
   (proposed: such Pods are excluded by default, with a distinct reason
   shown, rather than silently evicted and data lost — exact UX for an
   opt-in override, if any, is an open question, not resolved here).
5. Evict the remaining Pods, bounded and sequential-or-bounded-concurrent
   (open question: sequential is simpler to reason about and journal, but
   slow for a Node with many Pods — proposed default: bounded concurrency,
   e.g. up to N in flight, N to be decided, each still going through
   M8B.4's own PDB-aware eviction path independently).
6. Report per-Pod outcomes distinctly — never one aggregate "Drain
   succeeded" boolean. A Node can be "cordoned, N of M Pods evicted, K
   denied by PDB, J still pending" simultaneously, and the UI must show
   that composite truthfully.

**Cancellation semantics**: if the user cancels mid-drain, PODS ALREADY
EVICTED STAY EVICTED — cancellation stops issuing *further* eviction
requests, it does not and cannot undo requests already sent. This must be
stated explicitly in the UI at the moment of cancellation (reusing the
`OutcomeUnknown`/partial-progress vocabulary, never a blanket "cancelled"
that implies nothing happened).

**Partial-progress / resumability semantics — explicitly flagged as
needing careful design, not resolved here**: if a drain is interrupted
(cancelled, connection lost, SAURON restarted) partway through, is there a
"resume drain on this Node" concept, or does the operator simply see the
Node's current cordon/eviction state via normal read-only navigation and
manually decide whether to invoke Drain again? Proposed leaning: **no
built-in resume/retry state machine** — Drain is a single foreground
operation per invocation; a re-invocation on an already-partially-drained
Node just re-enumerates remaining Pods and continues, which is naturally
idempotent (already-evicted Pods are gone, so a fresh Pod-list read
naturally reflects only what's left) — but this needs explicit sign-off,
since "no resumability" is itself a real product decision, not a default
to slide past silently.

**"Never claim rollback if some Pods were already evicted"**: Drain has
NO ROLLBACK, ever. If the operation is stopped after evicting 3 of 10
Pods, the result state is "3 evicted, 7 not attempted (or in some other
terminal state)" — never "drain reverted" or any language implying the 3
evictions were undone. This must be a tested assertion (fake-HTTP or
integration-level), not just a documentation promise.

**Confirmation**: Strong, and — open question flagged for review — should
Drain require a DIFFERENT/stronger confirmation UX than the existing
double-press (e.g., because it is orchestrated and irreversible-in-parts,
maybe it deserves the "type the exact Node name" pattern the M8.0 ledger
explicitly considered and rejected for ordinary Delete)? Proposed: still
use the same double-press mechanism for consistency, but the PREVIEW text
before the first press must enumerate exactly what will happen (cordon +
N Pods to evict + M excluded with reasons) in enough detail that the
double-press is meaningfully informed, not just a rubber stamp — this
detail level is itself new territory for `workflow_report` and may need a
dedicated multi-line preview format beyond the current single-CHANGE-line
model other M8/M8B operations use.

**Journal**: needs a per-step model — cordon, then N per-Pod eviction
attempts, each independently journaled (reusing M8B.4's own eviction
journal entries) plus a drain-level "started"/"finished" bracket. Proposed
new `Phase` variants: `DrainStarted`, `DrainStep` (one per Pod, correlated
by the drain's own request_id AND the individual eviction's own
request_id), `DrainFinished` — exact shape deferred to implementation, but
the principle (every sub-action is independently auditable, not folded
into one opaque "drain" log line) is fixed here.

**Post-commit verification**: no single verification fact — the composite
per-Pod-outcome table described above IS the verification, continuously
updated as each eviction resolves, not a single post-hoc check.

**Evidence required**: unit (DaemonSet-Pod exclusion, local-storage-Pod
exclusion/flagging, idempotent re-invocation logic); fake HTTP (a
multi-Pod scenario with a mix of successful evictions, one PDB denial, one
DaemonSet exclusion, proving the composite report is accurate and that a
denied eviction never retries automatically); live (**explicitly flagged
open question**: kind-sauron-test is very likely single-node — draining
the only node is a materially different, more disruptive live test than
any other M8/M8B live check performed so far, since it would attempt to
evict every fixture Pod across every namespace on the cluster
simultaneously. This needs an explicit decision before M8B.5 is
implemented: (a) accept a genuinely disruptive single-node live drain test
that then requires re-running every other milestone's `-fixtures` step
afterward, (b) scope the live test to a purpose-built throwaway namespace
with 1-2 disposable Pods and somehow constrain Drain's live test run to
"a drain that only touches Pods in this namespace" — which the real
`kubectl drain`-equivalent semantics do NOT support (drain targets a Node,
not a namespace, so this would require a test-only restriction not present
in the real feature), or (c) treat Drain's live acceptance as
fake-HTTP-only with NO live Node-level test, explicitly documented as a
bounded limitation, matching M7's own precedent of not deriving every
scenario live when a lower layer already proves it. Leaning toward (c) for
the initial acceptance pass, with (a) as a possible one-time exception if
a dedicated multi-node kind cluster is later stood up — but this is a real
open question, not resolved by this document).

**Depends on**: M8B.4 (Evict) and M8B.1 (Cordon), both must be ACCEPTED
before M8B.5 implementation starts.

### M8B.6 — Force delete

**This is flagged as the highest-risk operation in M8B.**

**Resource kinds**: proposed to start deliberately narrow — Pod only,
possibly extended to Job/CronJob-created Pods specifically (the case where
force delete is most legitimately needed: a Pod stuck in `Terminating`
indefinitely because its node is unreachable). Explicitly NOT extended to
Deployment/StatefulSet/DaemonSet/ConfigMap etc. at first — those already
have a working, non-force Delete path (M8.3) and force semantics for them
introduce risks (orphaned dependents, stuck finalizers on higher-level
objects) disproportionate to any real operator need this milestone is
trying to serve.

**Semantic treatment**: `grace_period_seconds: 0` + (where truly
necessary) `propagation_policy` explicitly set — but **critically, this
must be its own distinct command/intent (`:force_delete`, not a flag on
`:delete`)**, so that `:delete`'s own `DeleteParams` construction in
`kube::mutation::delete_request` is never accidentally affected. The M8
prompt's own instruction from the original milestone ("no force/grace=0 as
an ordinary action") is restated and made binding here: **normal
`:delete`'s `DeleteParams` must remain hard-coded to default grace
behavior forever; force delete's grace-zero behavior must live in a
genuinely separate code path**, not a parameter that both share, to make
it structurally impossible for a future refactor to leak force semantics
into ordinary delete by accident.

**Policy/risk**: `PrivilegeSensitive` or a new dedicated
`MutationRisk`/`PolicyReason` tier above ordinary `Destructive` — proposed:
introduce `PolicyReason::ForceSemantics` (or similar) so the rendered
POLICY block is unambiguous that this is not ordinary delete, and so a
future policy audit can grep for exactly this reason. Always
`RequireStrongerConfirmation`, no exceptions, regardless of namespace.

**Preview must explain consequences** — not just "delete NAME", but
something like: "This skips the normal graceful termination sequence.
Containers will be forcibly stopped without their configured
`preStop`/termination grace period. If the Pod's node is unreachable, its
containers may continue running until the node recovers, even though
Kubernetes will consider the Pod gone (a known Kubernetes force-delete
caveat, not a SAURON limitation — this exact caveat should be stated in
the preview text, not left implicit)."

**TOCTOU/UID**: the server-side UID precondition (M8.3's
`DeleteParams.preconditions`) is proposed to remain, "where possible" per
the requirement — but note grace_period_seconds=0 combined with a
precondition is valid Kubernetes API usage (they're independent
DeleteParams fields), so this is not expected to be a real implementation
conflict, just an explicit confirmation to write into the eventual PR
description.

**Dry-run**: supported in principle (server dry-run for a DELETE with
grace=0), though a dry-run's value here is lower than for Modify
operations (there is very little to "get wrong" about a delete's target
identity that dry-run would catch beyond what TOCTOU revalidation already
catches) — included for consistency with every other M8/M8B operation
rather than because it is expected to catch much.

**No automatic retry**: restated explicitly as a hard requirement — a
`Conflict`/`TargetReplaced`/`OutcomeUnknown` outcome from a force delete
must never be silently retried by SAURON itself, exactly like every other
M7/M8/M8B operation. Ambiguous transport outcomes stay `OutcomeUnknown`,
never re-interpreted as success or failure.

**Absolutely not a generic bypass**: force delete must remain scoped to
its named, narrow kind list and its own explicit command — it must never
become "the" way to delete something Delete (M8.3) refuses, and the UI/
docs must be explicit that reaching for force delete because ordinary
delete "isn't working" is very likely the wrong move (ordinary delete
failing usually means a real TOCTOU/policy/PDB reason that force delete
does not actually address, except for the one legitimate case — a
genuinely stuck Terminating Pod on an unreachable node).

**Journal**: `Phase::CommitResult` as usual, with `detail` explicitly
noting `grace_period_seconds=0` was used (bounded, redacted, matching
every other `detail` convention) — an auditor reading the journal must be
able to tell a force delete apart from an ordinary one without cross-
referencing anything else.

**Post-commit verification**: identical to M8.3's existing Delete
verification (`DeletionInProgress`/`ObservedGone`/`Pending`) — no new
`Verification` variant needed.

**Evidence required**: unit (kind allowlist rejection, distinct command
parsing from `:delete`, preview consequence text present); fake HTTP
(grace_period_seconds=0 actually sent, ordinary `:delete`'s DeleteParams
provably unaffected by force delete's code path existing at all — a
regression test on M8.3's own delete path, not just a new test); live (a
Pod fixture, force-deleted, observed gone — a genuinely stuck/unreachable-
node scenario is NOT proposed to be reproduced live, since that would
require deliberately breaking a kind node, which is out of proportion to
what this milestone needs to prove; the live test proves the mechanism
works, not the specific stuck-node scenario it exists for).

### M8B.7 — Combined adversarial acceptance, regressions, soak

Mirrors M8.6's own structure exactly: `scripts/accept-m8b.py` (scenario
count TBD, but expected to need Cordon/Uncordon, Set Image, CronJob
trigger, Evict, Drain, Force Delete each represented, plus the same
regression touchpoints — readonly zero-write, replacement rejection,
conflict/no-retry, context/namespace churn, cancellation, M4 forward held,
M5/M6/M7/M8 regression, 32x9, terminal restoration), run twice; full
M1-M8 regression (all `accept-*.py` scripts through M8, plus M8's own
`accept-m8.py`); a soak (duration TBD, 75 minutes proposed as the
established convention) rotating through every M8B operation with the
same self-restoring/no-drift discipline M8.6's soak established (and, per
that soak's own lesson, with independent per-section error handling from
the start, not discovered as a bug mid-run). Node-level operations
(cordon/drain) in the soak rotation need their own explicit resolution of
the single-node-kind-cluster question raised under M8B.5 before the soak
script is written — this is not re-litigated here, just flagged as a
shared dependency.

## Bugs / limitations (placeholder)

None yet — implementation of M8B.0/M8B.1 has started (see Journal below)
but no bug has been found. This section exists so future implementation
work has a designated place to record what M8.6's own ledger called "bug
discipline": reproduce, classify (app/harness/fixture/environment), root
cause, regression-proof, full locked recheck, replay, only then continue.
Nothing here is retroactively filled in from M8; M8_ACCEPTANCE.md remains
the authoritative record of M8's own bugs.

## Journal

- 2026-09-18: M8B.0/M8B.1 implementation started. Added
  `mutation::workflow::{CORDON_KINDS, cordon, uncordon}` (Node only, via
  a shared private `set_unschedulable` builder so both directions stay a
  single tested code path with distinct `source_action`s); `Command::
  {Cordon,Uncordon}` + `:cordon`/`:uncordon` grammar (zero arguments,
  operate on the currently selected row, exactly like `:restart`/
  `:delete`); wired into `Runtime::command()` via the existing
  `mutation_scope`/`open_workflow_document` helpers -- no new app-layer
  machinery needed. `MutationRisk::ClusterCritical` used for both (Node
  is already in `PolicyContext::default()`'s `cluster_critical_kinds`, so
  this only makes the intent's own risk label accurate; it does not change
  `policy::evaluate`'s behavior, which was already correctly
  `RequireStrongerConfirmation` for any Node Modify regardless of
  `intent.risk`).
  Decision on the doc's own "open question" about a distinct
  `PolicyReason::NodeSchedulingChange`: deferred, not added. The POLICY
  block today shows the existing generic `ClusterScopedSensitiveResource`/
  `ClusterCriticalResource` reasons for a cordon/uncordon intent, which is
  correct but not maximally specific. Revisit if operator feedback says
  the generic wording is actually confusing in practice -- not assumed to
  be worth the change preemptively.
  `kube::mutation::verify`'s existing `Modify`/`leaf_path_and_value` path
  was confirmed to handle `/spec/unschedulable` (cordon direction, true)
  with zero new code, exactly as this document predicted -- proven
  directly by
  `verify_cordon_confirms_observed_unschedulable_flip_with_zero_new_code`
  in `tests/watch_transport.rs`. The uncordon direction (false) DID need
  one small fix -- see the next entry below, a real bug this document's
  own prediction did not anticipate.
  Evidence so far: 4 unit tests (`mutation::workflow::tests`: unsupported-
  kind rejection, exact payload + `ClusterCritical` risk for cordon,
  distinct source_action + payload for uncordon, distinct payload hashes
  between the two directions), 2 fake-HTTP tests (verify generalization;
  commit proving `policy::evaluate` returns `RequireStrongerConfirmation`
  for a real cordon intent and that the exact PATCH body is sent), 1
  command-grammar test, 1 app-level zero-write-under-readonly test. Full
  locked suite: 198 unit + 50 fake HTTP, fmt/check/clippy (`-D warnings`)
  clean.
  NOT done yet at this point: live evidence, interactive evidence,
  `m8b-fixtures`/`m8b-reset`/`m8b-test` cases in `scripts/test-cluster.sh`.
  Blocked on an explicit decision (this document's own open question,
  restated): is cordoning/uncordoning the live `kind-sauron-test`
  cluster's node(s) an acceptable live test, given the cluster is very
  likely single-node? -- explicit user sign-off obtained: yes, a brief
  cordon+uncordon within one sequential test is acceptable.
- 2026-09-18: M8B.1 live evidence added and a real bug found and fixed.
  Added `m8b-fixtures` (ensures every node starts schedulable; no
  namespaced fixture object needed for this slice, since Cordon/Uncordon
  targets the Node directly), `m8b-reset` (uncordons every node,
  idempotent), `m8b-test` cases to `scripts/test-cluster.sh`, and
  `tests/mutation_m8b_live.rs`
  (`live_cordon_then_uncordon_the_single_node`) -- one sequential test
  (matching every other `*_live.rs` file's own precedent against racing
  under parallel test execution) that asserts the cluster is genuinely
  single-node, cordons it, verifies, uncordons it, verifies again, and
  asserts the node is left schedulable no matter what.
  First run failed with a real, live-cluster-discovered bug (not a
  harness bug): after uncordon, `kube::mutation::verify` reported
  `ObservedDifferent("expected /spec/unschedulable=false, observed
  /spec/unschedulable=<absent>")` even though the node WAS correctly
  schedulable. Root cause: Kubernetes' own Go struct tags mark
  `spec.unschedulable` `omitempty`, so the API server's JSON response
  omits the field entirely when its value is the boolean zero value
  (`false`) -- absent and explicit-`false` are the identical observed
  fact for this field, but `verify_modify`'s equality check (`observed ==
  Some(other)`) treated them as different. Fixed by extending the existing
  `Value::Null` special case (already present for label/annotation
  removal) with a parallel `Value::Bool(false)` case: `observed.is_none()
  || observed == Some(&Value::Bool(false))` both count as matching. Added
  a fake-HTTP regression test
  (`verify_uncordon_treats_an_omitted_false_field_as_verified_not_different`)
  proving this before re-running the live test, which then passed clean,
  twice, leaving the node schedulable both times (confirmed via `kubectl
  get nodes` showing `Ready`, never `Ready,SchedulingDisabled`, after
  either run). Full locked suite re-confirmed clean: 198 unit + 51 fake
  HTTP, fmt/check/clippy (`-D warnings`).
  This is exactly the kind of finding M8B's own "design test" and bug
  discipline exist to catch: a genuine gap in how far the *shared*
  verification logic generalizes, caught by actually exercising a new
  kind against the real API server rather than only against fake-HTTP
  fixtures (which had no reason to omit the field, since the test author
  writing a fake-HTTP fixture naturally writes explicit values). This
  finding also means M8B.0's "does `verify` generalize with zero new
  code" question from the original design must be revisited when
  implementing M8B.2 (Set Image) and beyond: any future boolean (or other
  Go-zero-value, `omitempty`-tagged) field will need the same
  absent-equals-zero-value treatment, not just this one.
  M8B.1 ACCEPTED for unit/fake-HTTP/live; interactive TUI coverage
  deferred to M8B.7's combined script, matching M8.2 (Restart)'s own
  precedent of not requiring every individual slice to carry its own
  dedicated interactive scenario.
- 2026-09-18: M8B.2 (Set Image) implemented and ACCEPTED (unit/fake-HTTP/
  live; interactive deferred to M8B.7). This document's own open question
  ("Patch::Strategic vs. builder-side full-array reconstruction") was
  resolved in favor of option (b): `workflow::set_image` takes the
  object's own full, live `spec.template.spec.containers` array, rejects
  an unknown container name or an ambiguous (multi-container, unnamed)
  request explicitly, and reconstructs the complete array with only the
  named container's `image` field changed -- every other field of every
  container (including the target's own `resources`/`env`/etc.) is
  preserved verbatim via `Value::clone()`, never re-derived or
  simplified. This keeps `kube::mutation::patch_request` on the single
  `Patch::Merge` mechanism every other M8/M8B operation already uses --
  no `Patch::Strategic` introduced, no second patch code path.
  `leaf_path_and_value`'s single-scalar-leaf assumption does NOT apply to
  an array-valued payload, exactly as predicted -- added a dedicated
  `verify_set_image` (dispatched in `kube::mutation::verify` by checking
  `intent.source_action == "set_image"` before falling through to the
  generic path) that compares every container named in the payload
  against the freshly-observed object by name-match, not array index or
  whole-array equality -- this incidentally also re-verifies every
  *unrelated* container matches too, catching an accidental cross-
  container corruption from the builder's own reconstruction, not just
  the one container that was supposed to change.
  Live evidence: `tests/fixtures/m8b-set-image.yaml` (a dedicated
  `sauron-m8b` namespace, two-container Deployment `m8b-multi`);
  `m8b-fixtures`/`m8b-reset` extended to apply/roll it out;
  `live_set_image_changes_only_the_named_container` in
  `tests/mutation_m8b_live.rs` changes the `web` container's image and
  asserts, via a fresh `kubectl`-equivalent GET, that `sidecar`'s image
  and every other field survived byte-for-byte. Run twice via
  `scripts/test-cluster.sh m8b-test`, both clean, fixtures reset to
  pristine between runs.
  6 unit tests (`mutation::workflow::tests`: unsupported-kind rejection,
  unknown-container rejection, ambiguous-multi-container rejection,
  single-container default-without-naming, unrelated-container/field
  preservation, distinct payload hashes for different target containers),
  3 fake-HTTP tests (verify success, verify `ObservedDifferent` when an
  unrelated container unexpectedly changed, commit sending the full
  reconstructed array with the unrelated container's exact image string
  still present in the request body), 1 grammar test. Full locked suite:
  205 unit + 54 fake HTTP, fmt/check/clippy (`-D warnings`) clean.
  No SAURON app bugs found in this slice (M8B.1's `omitempty`-false fix
  from the previous entry already generalized correctly here: Set Image's
  own values are non-empty strings, never Kubernetes' boolean zero value,
  so the fix's scope was correctly limited to booleans and did not need
  broadening for this slice).
- 2026-09-18: M8B.3 (CronJob trigger) implemented and ACCEPTED (unit/
  fake-HTTP/live; interactive deferred to M8B.7) -- the first real
  `MutationEffect::Create` in `kube::mutation`, resolving this document's
  own open questions:
  - `MutationIntent` gained one new field, `create_resource:
    Option<Resource>` -- the resource actually being *created* (Job),
    distinct from `target.resource` (the CronJob, whose UID is
    TOCTOU-revalidated exactly like every other operation, via the
    already-generic `revalidate()`, unchanged). Every existing
    `MutationIntent` literal (six call sites, mostly single shared test
    helpers) got `create_resource: None`; zero behavior change for any
    existing effect.
  - `kube::mutation::create_request` (new) POSTs the full object manifest
    via `PostParams`/`Request::create`; wired into both `preflight`
    (dry-run) and `commit` (real), matching `patch_request`/
    `delete_request`'s own shape.
  - Traceability back to the source CronJob uses a label
    (`sauron.io/triggered-from`) + annotation
    (`sauron.io/triggered-from-uid`), deliberately NOT a real
    `ownerReference` -- a real owner reference would make the triggered
    Job subject to garbage collection if the CronJob is later deleted,
    which `kubectl create job --from=cronjob/X` itself also avoids for
    the same reason.
  - `Verification::Created(String)` added (a fresh GET confirms the new
    object exists under its own deterministic, previously-generated
    name -- `verify_create`, a fourth verification shape alongside
    `verify_modify`/`verify_set_image`/`verify_delete`); `Pending` on a
    404 (creation accepted, not yet visible), never treated as failure.
  - **Real bug found and fixed, not in M8B.3's own new code but in M7's
    original `policy::evaluate`**: the rule `confirmation_needed = strong
    || destructive || (effect != Create)` meant a `Routine`-risk Create
    silently bypassed confirmation entirely (`PolicyDecision::Allow`) --
    untested in practice, since M7 shipped no real Create path (always
    `Unsupported`). Now that Create means something real (running
    workload code via a triggered Job), silently allowing it without
    confirmation would be wrong. Removed the `effect != Create` escape
    hatch; every current effect now requires at least standard
    confirmation, matching Scale/Restart/Label/Annotate/Set Image's own
    tier. Updated the one existing test that depended on the old
    behavior (`create_with_routine_risk_is_allowed_without_confirmation`
    -> `create_with_routine_risk_still_requires_confirmation`). Verified
    this does not regress any shipped M8/M8B behavior: Modify/Delete were
    never affected by the removed clause (their own `effect != Create`
    was always `true` already); only Create's previously-untested,
    never-shipped path changes. `PolicyDecision::Allow` remains a
    legitimate model value, just unreachable by anything this codebase
    currently produces.
  - A second, genuinely live-cluster-only bug was found and fixed in the
    live test itself (not the app): the first live run asserted the
    created Job's label via `created.data.pointer("/metadata/labels/...")`
    -- `DynamicObject.data` holds only non-metadata fields; `metadata` is
    a separate typed `ObjectMeta` field, so the pointer always returned
    `None` regardless of the (correct) real label being present, as
    confirmed independently via `kubectl get -o jsonpath`. Fixed by
    reading `created.metadata.labels` directly. Classified as a harness/
    test bug, not an app bug -- the same class of `DynamicObject.data`
    vs. typed-field confusion already hit once in this same file for the
    Node cordon test's earlier `.spec` mistake.
  Live evidence: `tests/fixtures/m8b-cronjob.yaml` (CronJob `m8b-nightly`,
  schedule `0 0 1 1 *` so it never fires on its own during a test run,
  keeping any observed Job attributable only to a live trigger);
  `m8b-fixtures` applies it, `m8b-reset` also deletes every
  label-matching triggered Job so repeated runs stay deterministic;
  `live_trigger_creates_a_job_traceable_to_the_source_cronjob` in
  `tests/mutation_m8b_live.rs` triggers a real Job, commits, verifies
  `Created`, and confirms the label back to the source. Run twice via
  `scripts/test-cluster.sh m8b-test` (alongside the M8B.1/M8B.2 live
  tests in the same file), both clean, fixtures reset to pristine
  between runs.
  5 unit tests (`mutation::workflow::tests`: unsupported-kind rejection,
  invalid-generated-name rejection, `Create` effect + `create_resource`
  set correctly, exact payload name/template, distinct hashes for
  distinct generated names), 4 fake-HTTP tests (commit posts the exact
  manifest and requires confirmation, dry-run never commits, verify
  reports `Created`, verify reports `Pending` on 404), 1 grammar test, 1
  app-level zero-write-under-readonly test. Full locked suite: 212 unit +
  58 fake HTTP, fmt/check/clippy (`-D warnings`) clean.

## Final acceptance conditions (proposed, mirroring M8's own structure)

M8B is not ACCEPTED until, for every slice M8B.0-M8B.7:

- Every supported operation goes through the M7 gateway
  (`preflight`/`commit`/`verify`) — no direct UI mutation bypass, exactly
  as M8 required.
- Readonly means zero writes for every M8B operation, exactly as M8
  required, verified per-operation, not just once generically.
- TOCTOU/UID replacement rejection is proven per operation that has an
  existing target to replace (all except CronJob trigger's created Job,
  and Drain's own composite semantics, which need their own explicitly
  designed equivalent check).
- Conflicts are explicit; no automatic ambiguous retry, anywhere,
  including inside Drain's per-Pod eviction loop.
- `OutcomeUnknown` (and Drain's own partial-progress reporting) is
  preserved and never silently upgraded to success or downgraded to
  failure.
- The journal records every real commit attempt/result for every
  operation, including Drain's per-step records and CronJob trigger's
  created-Job correlation.
- Post-commit verification is truthful and distinct from commit outcome
  for every operation, exactly as M8.5 established, including the new
  verification shapes Set Image and CronJob trigger require.
- Health/readiness is never conflated with mutation success (Drain
  especially: "cordoned and evicted" is not the same claim as "workloads
  rescheduled and healthy elsewhere", which stays M5's Explain/health
  domain).
- Timeline contains only observed changes; Adjacent/Xray remain
  observation-derived — restated because Drain's Node-Pod relationship
  reasoning must not tempt a manually-constructed graph shortcut.
- 32x9 works, including Drain's necessarily denser multi-line preview.
- Terminal restoration works.
- Full M1-M8 regression passes.
- The soak (M8B.7) is acceptably stable, with the same "observed
  stability only, no leak-freedom claims" honesty M8.6 already
  established.
- Production sees zero writes and zero mutating dry-runs across every
  M8B operation.
- Docs (this file, `HANDBOOK.md`, `docs/RUNBOOK.md`, `docs/
  SOFKA_PARITY.md`, `README.md`) are reconciled with actual evidence.
- Worktree is clean.

Only then: create a local annotated tag (name TBD, e.g. `m8b-accepted`,
mirroring the `m7-accepted`/`m8-accepted` convention) — never pushed
without explicit authorization, exactly like every prior milestone tag.

## Open architectural questions requiring resolution before implementation

Collected here from the per-operation sections above, so they are not lost
in prose:

1. Set Image's payload/verification shape breaks `verify`'s current
   single-leaf-JSON-pointer assumption (`leaf_path_and_value`) — needs a
   named second comparison shape (array-element-by-key), decided before
   M8B.0/M8B.2 implementation.
2. CronJob trigger is the first real use of `MutationEffect::Create` in
   the executor — what TOCTOU identity applies when there is no existing
   target (proposed: the source CronJob's UID), and what `Verification`
   variant models "this new object was created and looks right" (proposed:
   a new `Created` variant, or a reinterpretation of `Verified`) both need
   explicit decisions.
3. Evict's PDB-denial (`429`) classification needs a new, precisely-named
   `MutationOutcome` variant (or a deliberate, justified reuse of
   `Conflict`) — decided before M8B.4.
4. Drain's orchestration model (one Node-level user action producing many
   independent eviction intents) is a structural exception to "one action,
   one intent" and needs its own reviewed design note (journal `Phase`
   shape, cancellation semantics, no-resumability decision, concurrency
   bound) before any code is written, not discovered mid-implementation.
5. Drain's live acceptance strategy on a very likely single-node
   `kind-sauron-test` cluster is unresolved — leaning toward fake-HTTP-only
   live coverage with an explicitly documented limitation, but this is a
   product decision, not assumed here.
6. Force delete's confirmation strength and preview-consequence wording
   need actual copy drafted and reviewed (this document proposes the
   *shape*, not final text) before implementation, since the caveat about
   unreachable nodes is easy to get subtly wrong or alarmist.
7. Cordon/Uncordon: whether a distinct `PolicyReason` (e.g.
   `NodeSchedulingChange`) is worth adding versus relying on the existing
   generic cluster-critical reasons — a small decision, but affects the
   POLICY block's exact wording, so flagged rather than assumed.
8. Whether cordoning/draining the single node of `kind-sauron-test` is an
   acceptable live test at all (M8B.1 flags a narrow cordon/uncordon
   version of this same question; M8B.5 flags the much larger drain
   version) — one shared decision point, listed twice above for locality,
   listed once here as the actual open question.
