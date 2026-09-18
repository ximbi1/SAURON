# Future mutation families (catalog, not a roadmap commitment)

This document is **not a roadmap commitment**. It is a catalog of mutation
families that are intentionally deferred — not merely "not built yet", but
deliberately kept out of M8/M8B's scope because each one changes the
safety story in a way that deserves its own dedicated design pass, not an
incremental extension of the existing Scale/Restart/Delete/Label/Annotate/
Cordon/Drain/etc. shell. Nothing here is scheduled, sized, or approved for
implementation. Anything eventually attempted from this list should get
its own milestone document (an "M#_ACCEPTANCE.md"-shaped ledger, following
the same structure M7/M8/M8B already established), written when that work
actually starts — not derived wholesale from this catalog entry.

Candidates are considered only after M12, or as an explicitly separate,
deliberately-scoped future milestone — never folded casually into M8B or
any milestone whose contract does not already name them.

## Arbitrary apply / edit

**What it would be**: a general "apply this YAML/JSON" or "edit this
object's spec/metadata freely" capability — the actual `kubectl apply -f`/
`kubectl edit` equivalent, as opposed to every named, single-purpose
operation M7/M8/M8B define.

**Why intentionally deferred**:
- It has a **broad schema surface** — literally any field of any resource
  kind, including ones SAURON has never specifically modeled. Every other
  operation in this codebase is deliberately narrow (one field, one kind
  family, one well-understood semantic change) specifically so its policy/
  preview/verification story can be exact. Arbitrary apply/edit has no
  natural boundary to be exact about.
- **Server-side apply ownership/`managedFields` implications**: a real
  `kubectl apply`-equivalent needs to reckon with field ownership conflicts
  (another controller or user owning a field the applied document also
  sets), which requires either adopting server-side apply semantics
  end-to-end (a substantial new transport/negotiation concern) or accepting
  client-side-apply's cruder, more dangerous "last write wins on the whole
  object" behavior — neither of which any current SAURON mutation needs to
  think about, because none of them touch more than one or two named
  fields.
- **Diff/preview requirements** are much harder here: every other
  operation's preview is a one-line "OLD -> NEW" because the change is
  narrow by construction. A general apply/edit needs a real structural
  diff of arbitrary depth, correctly handling added/removed/reordered
  list elements, nested maps, etc. — a materially bigger UI/preview
  engineering problem than anything M7/M8/M8B require.
- **It is easy to bypass semantic safety accidentally**: the whole point
  of M7's policy engine is that it can reason about "this is a namespace-
  scoped ConfigMap Modify" or "this is a cluster-critical Node Modify"
  because the *kind* and *effect* are already known before policy runs.
  An arbitrary-edit surface makes the "what is this change, really"
  question itself part of user input, which is a fundamentally different
  (and harder) trust boundary — a crafted or careless edit could touch a
  Secret's data, a RBAC binding, or a Namespace's finalizers through what
  looks like an innocuous "just editing a Deployment" flow, unless the
  tool inspects the diff deeply enough to re-derive exactly the kind of
  semantic classification the named operations get for free.

**What extra policy/UX/verification would be required**: a real
structural diff engine; a policy layer that inspects the actual changed
paths (not just the object's kind) and can escalate risk per-path (e.g.
"this edit touches `data`/`stringData` on a Secret" should escalate
regardless of what else is in the diff); a server-side-apply-aware
executor (field manager identity, conflict resolution, force-conflicts
opt-in as its own extremely-guarded sub-decision); verification that
re-derives "did the whole diff apply as shown" rather than one field's
equality.

**Why it should not be folded casually into M8B**: every M8B operation
still fits the "one semantic action, one exact intent" model M7 was built
around. Arbitrary apply/edit does not — it is a different kind of tool
(a structural editor) sitting on top of the same execution gateway, not
another named workflow beside Scale/Cordon/Drain.

**Possible future milestone shape**: its own milestone, gated explicitly
behind a real server-side-apply story and a real diff/preview UI, almost
certainly with its own stricter default posture (e.g. read-only preview of
the computed diff mandatory before any confirmation step is even offered,
and probably restricted to a smaller kind allowlist at first, mirroring
how M8.3's Delete started narrow rather than "all kinds").

## Secret mutations

**What it would be**: creating, patching, or deleting Secret objects'
`data`/`stringData` — as opposed to the current state, where Secrets are
explicitly excluded from Label/Annotate (M8.4) and every other current
operation.

**Why intentionally deferred**:
- **Sensitive data handling** is the whole reason this is its own
  category, not an incremental extension of Label/Annotate. Every other
  M7/M8/M8B mutation's payload is safe to log/journal/preview in full
  (a replica count, a timestamp, a label key/value, a container image
  reference) — Secret values are categorically not.
- **Redaction** requirements are stricter than anything the current
  `safety::redact`/journal bounding already does for *display* of existing
  Secrets (read paths) — a *mutation* UI additionally has an INPUT problem:
  where does the new Secret value come from, and how is it kept off the
  terminal scrollback, off the command palette's own text (which SAURON
  currently renders as typed, e.g. in the `:label KEY=VALUE` palette line),
  and off any preview text, while still letting the operator confirm what
  they intended to set?
- **Journal must never capture secret values** — trivially stated, but
  the existing `journal::Record`'s `summary`/`detail` fields are free-text
  strings that every other operation trusts to be safe to log in full;
  a Secret-mutation-aware journal path would need either a hard-coded
  "never log this field for Secret kinds" rule enforced at the type level
  (not just "please don't"), or a structurally different Record shape for
  Secret operations specifically.
- **Special UX required**: likely a masked-input concept for values,
  explicit "you are about to write to a Secret" framing distinct from
  ordinary confirmation, and probably a policy tier stricter than any
  current `PrivilegeSensitive` classification (Secrets are already
  `privilege_sensitive_kinds` today for the *existing*, very limited
  read-adjacent surfaces — a write capability deserves its own dedicated
  review, not inheriting the existing tier by default).

**Why it should not be folded casually into M8B**: none of M8B's
operations touch Secret *content* at all (Cordon/Drain/Set Image/CronJob
trigger/Evict/Force delete all operate on Node/Deployment/StatefulSet/
DaemonSet/CronJob/Pod, never Secret data) — there is no natural extension
point in M8B's scope that would organically grow into this; it would have
to be added deliberately, which is exactly why it belongs in its own
milestone instead.

**Possible future milestone shape**: its own milestone, almost certainly
gated behind explicit, additional user consent/configuration beyond the
existing `mutation_test_cluster_verified`/readonly gates (e.g. a distinct
opt-in flag specifically for Secret-write capability, so it is not
available merely because ordinary mutations are), with UX review as a
first-class deliverable before any executor work starts.

## RBAC mutations

**What it would be**: creating/modifying Role, RoleBinding, ClusterRole,
ClusterRoleBinding, or ServiceAccount objects.

**Why intentionally deferred**:
- **Privilege escalation risk**: an RBAC write can grant capabilities
  (including, transitively, further RBAC-write capability) well beyond
  whatever the operator's own current session is scoped to think about in
  the moment — the blast radius of a wrong RBAC change is not "this one
  object misbehaves", it is "some identity now has access it should not
  have", which is a fundamentally different risk shape than anything
  Scale/Restart/Delete/Cordon/Drain create.
- **Self-lockout risk**: an operator could, through SAURON, remove their
  own effective permissions (or the permissions SAURON's own service
  account needs to keep functioning against that cluster), a failure mode
  none of M7/M8/M8B's operations can cause (none of them touch the
  identity/permission graph at all).
- **Graph/impact analysis desirable**: before allowing an RBAC write, a
  responsible tool would ideally show "here is what this Role/Binding
  currently grants, and to whom" and "here is what it would grant after
  this change" — a real analysis feature, not a one-line preview, and one
  that arguably deserves to exist as a *read-only* Explain/Adjacent-style
  feature on its own, well before any RBAC *write* capability is
  considered.
- **Stronger policy tier needed**: RBAC objects would need a policy
  classification above today's `PrivilegeSensitive`/`ClusterCritical`
  ceiling — today those tiers exist to route to
  `RequireStrongerConfirmation`, the same UX every other Destructive/
  cluster-critical operation gets; RBAC arguably needs a qualitatively
  different gate (e.g. mandatory dry-run-and-review, or an explicit
  "explain what this grants" step folded into confirmation itself, not
  just a stronger version of the same double-press).

**Why it should not be folded casually into M8B**: RBAC objects are
already explicitly excluded from every M8/M8B operation (Secret/
ServiceAccount/Role/RoleBinding/ClusterRole/ClusterRoleBinding are in the
existing `privilege_sensitive_kinds`/`cluster_critical_kinds` denial-
adjacent sets, and M8.3's Delete allowlist explicitly does not include
them) — there is no partial version of "RBAC mutation" that fits inside
M8B's existing per-operation contracts.

**Possible future milestone shape**: its own milestone, likely requiring
the read-only "what does this grant, to whom" analysis feature to exist
and be trusted FIRST, as a prerequisite deliverable, before any RBAC write
path is designed at all.

## Namespace delete

**What it would be**: deleting a Namespace object (and, by Kubernetes'
own cascading-deletion semantics, everything inside it).

**Why intentionally deferred**:
- **Extremely destructive fanout**: a single Namespace delete can remove
  an unbounded number of objects of every kind that namespace contains —
  categorically different from every current Delete target (M8.3's
  allowlist is Pod/Deployment/StatefulSet/DaemonSet/ReplicaSet/Job/
  CronJob/ConfigMap, each a single object with a bounded, well-understood
  footprint).
- **Broad blast radius**: unlike Delete's existing "informational, not
  causal" relationship summary (M8.3's "currently related objects" framing,
  never "deleting this will break X"), a Namespace delete's true impact is
  genuinely close to "everything in here is going away" — a preview that
  tried to enumerate it fully could be enormous, and a preview that
  didn't would be misleadingly thin for the actual risk.
- **Finalizers**: Namespace deletion routinely gets stuck in a
  `Terminating` state indefinitely when some contained resource (or a
  cluster-level admission/finalizer controller) blocks completion — a
  real operational headache even for experienced operators using
  `kubectl` directly, and not a problem any current SAURON operation has
  to reason about (Pod/ConfigMap/etc. deletes normally complete quickly).
- **Long-running deletion**: unlike every current Delete (M8.3) or Evict
  (M8B.4) target, which resolve to `ObservedGone` within a bounded,
  short poll window, a Namespace's full teardown can take an
  unpredictably long time — the existing "bounded verification, explicit
  timeout, never block indefinitely" discipline would need genuine
  rethinking for what "verification" even means here (is `DeletionInProgress`
  sufficient forever, or does an operator need some notion of "how much
  is left"?).
- **Strong verification/confirmation required**: almost certainly needing
  its own confirmation tier beyond the existing double-press strong
  confirmation, given the scale of consequence — possibly the
  "type the exact namespace name" pattern M8.0's own design contract
  explicitly considered and rejected for ordinary mutations, revisited
  here specifically because the stakes are different in kind, not just
  degree.

**Why it should not be folded casually into M8B**: Namespace is already
in `cluster_critical_kinds` and explicitly absent from M8.3's Delete
allowlist; M8B's own Force delete (M8B.6) is explicitly scoped to Pod
(and possibly Job/CronJob-created Pods) specifically to avoid this exact
category of risk — there is no natural, narrow M8B extension that becomes
"delete a namespace" without abandoning every scoping discipline M8B
itself establishes.

**Possible future milestone shape**: its own milestone, likely requiring
a bespoke long-running-operation UI pattern (progress over an open-ended
duration, not a single commit+verify round-trip) that does not exist
anywhere in SAURON today.

## PVC/PV destructive operations

**What it would be**: deleting PersistentVolumeClaims or
PersistentVolumes, or otherwise mutating them in ways that affect the
underlying storage (e.g. forcing a `Released` PV back to `Available`,
editing `reclaimPolicy`).

**Why intentionally deferred**:
- **Data-loss risk**: unlike every other object this codebase mutates,
  a PVC/PV represents a claim on actual persisted data. Deleting the
  Kubernetes object is not "the workload goes away and comes back" (as
  Restart/Evict/Drain assume for compute) — depending on `reclaimPolicy`,
  it can mean the underlying storage volume itself is destroyed,
  irreversibly.
- **`reclaimPolicy` semantics**: `Delete` vs `Retain` vs (deprecated)
  `Recycle` fundamentally change what a PV deletion *does* to the backing
  storage — a tool offering this operation must understand and surface
  this distinction correctly per-object, not treat PV deletion as a
  uniform action the way M8.3's Delete treats its seven allowlisted kinds
  uniformly.
- **Storage-class/provider differences**: dynamic provisioners (the local-
  path-provisioner already used in this repo's own M6 fixtures, plus every
  real cloud provider's CSI driver) each have their own behavior around
  what happens to backing storage on PVC/PV deletion, detach timing, and
  finalizer removal — there is no single "this is what deleting a PVC
  does" story the way there is for, say, deleting a ConfigMap.
- **Detach/finalizer/state transition considerations**: PVs go through
  `Bound`→`Released`→(`Available`|deleted) transitions gated by CSI
  detach operations and finalizers that can themselves get stuck — another
  instance of the "long-running, not a bounded verify" problem Namespace
  delete raises, specific to storage's own lifecycle here.
- **Irreversible outcomes**: unlike every current mutation (even Delete,
  where the object can at least be recreated with fresh data/config even
  if the exact prior state is gone), destroying backing storage with
  `Delete` reclaim policy is often genuinely unrecoverable data loss, not
  just "recreate the Kubernetes object and move on."

**Why it should not be folded casually into M8B**: PersistentVolume and
PersistentVolumeClaim are already in `privilege_sensitive_kinds`; none of
M8B's seven operations touch storage objects at all, and Force delete
(M8B.6) is explicitly scoped away from anything but Pod-family objects
specifically to avoid this exact risk category.

**Possible future milestone shape**: its own milestone, almost certainly
requiring `reclaimPolicy`-aware, storage-class-aware preview text (e.g.
explicitly stating "this PV's reclaimPolicy is Delete; deleting it will
destroy the underlying volume" vs. "...is Retain; the underlying volume
will survive but become unmanaged") before any confirmation step, plus its
own dedicated live-acceptance strategy given how provider-dependent the
actual data-loss behavior is (the local-path-provisioner fixture already
in this repo may or may not be representative of real cloud-provider CSI
behavior — worth flagging now, resolving later).

## Generic patch console

**What it would be**: a direct "enter a JSON Patch or Merge Patch document
and apply it to the selected object" capability — narrower than full
arbitrary apply/edit (no full-object YAML, no server-side-apply ownership
questions), but still fundamentally different from every named M7/M8/M8B
operation, since the *content* of the patch is entirely user-authored
free text rather than a semantically-validated, narrow builder output.

**Why intentionally deferred**:
- **Potential bypass around semantic workflows**: this is the single
  clearest way an operator (or a future contributor, or muscle-memory from
  `kubectl patch`) could route around every named operation's careful
  validation (Set Image's container-name checking, Label's key/value
  syntax validation, Cordon's Node-only restriction) by just... patching
  the field directly. Every safety property this codebase has built is
  about knowing *in advance* what kind of change is being made; a generic
  patch console erases that knowledge at the one point where it matters
  most (before policy evaluation).
- **Easy to invalidate policy guarantees**: `mutation::policy::evaluate`'s
  risk classification today depends partly on `intent.effect`
  (Modify/Delete/Create) and `intent.risk`, both of which are currently
  ALWAYS set correctly because they come from a trusted builder
  (`workflow::scale`/`workflow::label`/etc.), never from user input. A
  generic patch console would need the *user's freely-typed patch content*
  to somehow still produce a trustworthy `MutationRisk`/effect
  classification, which is close to a contradiction — either the console
  restricts what kinds of patches are even expressible (at which point it
  is not meaningfully more general than the named operations already
  cover), or policy classification degrades to "any generic patch on a
  sensitive kind is always maximally strong", which makes the feature far
  less useful than its name implies while still carrying real risk.
- **If ever supported, must still pass through the M7 executor and
  explicit policy** — restated as non-negotiable even in this deferred-
  catalog entry: there is no version of this feature, however far in the
  future, that would be allowed to call `patch_request`/`delete_request`
  (or their future equivalents) directly, bypassing `commit`'s policy-
  evaluation-then-confirmation-then-revalidation sequence. The entire
  point of flagging this feature as high-risk is that the temptation to
  treat it as "just a thin wrapper around the same PATCH call" is exactly
  the failure mode to avoid.
- **Likely expert-only / heavily restricted**: if built at all, almost
  certainly gated behind its own explicit, separate opt-in (beyond
  ordinary mutation-capability gating), probably with a mandatory diff
  preview of the patch's *computed effect* (not just an echo of the typed
  patch document) before any confirmation, and likely restricted to a
  narrow kind allowlist rather than "any object SAURON can see."

**Why it should not be folded casually into M8B**: it is the closest
thing on this entire list to actively undermining M8B's own stated design
principle ("treat these as semantic operations of first-class, not as
kubectl passthrough or generic patch") — including it in M8B would
contradict M8B's own charter, not just extend it.

**Possible future milestone shape**: if ever pursued, its own tightly-
scoped milestone with the diff-preview-of-computed-effect requirement
above as a hard prerequisite, not an incremental "add a patch box"
feature.
