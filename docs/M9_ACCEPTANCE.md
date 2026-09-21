# M9 — GitOps/Package Manager Integration (Flux, Argo CD, Helm): acceptance ledger

Status: **ACCEPTED** (2026-09-20) — M9.0-M9.5 and M9.7 ACCEPTED; M9.6
(Helm rollback/uninstall) explicitly DEFERRED, an evidence-backed
outcome of its own required investigation, not a blocker. Local
annotated tag `m9-accepted`, never pushed without explicit
authorization.
Checkpoint clarification (2026-09-21): `m9-accepted` points to `f2a18d0`.
Subsequent codebase HEAD `75a3c63` changes only `scripts/soak-m9.py` to
represent an unparsed metrics counter explicitly. The tag has not been moved.
This document started as scope definition and contract design written
before implementation, exactly like `docs/M8B_ACCEPTANCE.md` was for
M8B, and is updated slice by slice as each is implemented. Nothing in
this file authorizes touching code, cluster, or CI beyond the slices
explicitly marked ACCEPTED in the Journal.

## Purpose

M1-M8B built a Kubernetes/operator console: observation (watch/list) ->
evidence (M5 health, M6 relationships) -> guarded action (M7/M8/M8B
mutation model) -> journal -> verification. M9 extends that same
architecture to the three most common cluster-operations layers built on
top of raw Kubernetes objects: Flux (GitOps reconciliation), Argo CD
(GitOps sync), and Helm (package/release management). M9 is explicitly
**not** "add flux/argocd/helm as shell commands" — every read view reuses
the same Kubernetes-API-first observation model this app already has for
every other resource kind, and every guarded action goes through the
existing M7/M8 mutation gateway unmodified. If any candidate integration
cannot be expressed that way, M9 defers or narrows scope rather than
bypassing the model (see Design rule, below).

## Relationship to M7/M8/M8B

M9 reuses, unmodified in spirit, everything M7/M8/M8B already built:

- `mutation::{MutationIntent, MutationTarget, MutationEffect, MutationRisk,
  PolicyReason, PolicyDecision, PolicyEvaluation, ConfirmationRequirement,
  Confirmation, MutationOutcome, Verification}` — the pure model. M9 adds
  new `source_action` values (see the journal-correlation convention in
  M9.0 below) and, where genuinely needed, new `PolicyReason`/
  `MutationOutcome`/`Verification` variants — it does not fork the model.
- `mutation::policy::evaluate` — the single policy engine. Flux/Argo/Helm
  actions are new *inputs* (new kinds, new risk classifications), never a
  parallel decision path.
- `mutation::workflow::{Built, Workflow}` — the same shared shell every
  M8/M8B builder returns. Every M9 action builder returns a `Built`
  exactly like `scale`/`cordon`/`evict` do, unless an action is genuinely
  composite (mirroring Drain's own documented exception in M8B.5), in
  which case it follows that same documented-exception pattern rather
  than inventing a new one.
- `kube::mutation::{preflight, commit, verify}` — the single execution
  gateway and the single verification entry point. M9 must not add a
  second gateway, a second TOCTOU mechanism, or a second verification
  dispatcher. Where an M9 action's target is not a plain Kubernetes PATCH/
  DELETE (e.g. an annotation-based reconcile request, or an Argo CD
  operation that may require calling the Argo API service rather than a
  CRD PATCH), the *executor* gains a new dispatch branch under the same
  gateway — never a bolted-on side path that skips policy/journal/
  verification (see M9.4's explicit "provider-action execution adapter"
  requirement).
- `mutation::journal::{Journal, Phase, Record}` — the same append-only,
  redacted, bounded journal. No second journal file or format.
- `Document.workflow`/`Document.drain`-style dedicated fields, the
  `"mutation"`/`"drain"` keymap modes, and `mutation::view::workflow_report`
  — the same shared UI shell, extended only where an M9 action has more
  than one meaningful step or a genuinely different verification shape
  than "does a fresh read match the requested change".
- `Command::{...}` + `parse()` grammar + `command_names()` — the same
  single command registry. No second palette, no hidden command.
- `evidence::Unknown`, `resources::health::{Severity, Health}` — reused
  directly wherever their existing semantics genuinely apply (CRD/API
  absence is `Unknown::Unsupported`, a partial discovery is
  `Unknown::Partial`, a resource's own Kubernetes-level health is still
  M5's `derive()`); M9 does not invent a parallel "GitOps health" engine
  a Flux/Argo condition is rendered as evidence text alongside — never
  instead of — Kubernetes-level health.
- `Adjacent`/`Xray` (M6) — Flux/Argo/Helm objects join these views through
  the exact same explicit-reference/provenance model every other
  relationship already uses (owner references, label selectors, explicit
  `spec` source fields) — never a heuristic name-based guess.
- The `ConnectOptions.mutation_test_cluster_verified` /
  `--mutation-test-cluster-verified` distinction (never `!readonly`, never
  a context-name heuristic) — unchanged, reused as-is for all M9 live
  acceptance.
- `scripts/test-cluster.sh`'s guarded Docker/API identity check — M9 adds
  its own `m9-*`-prefixed cases following the exact same shape (see
  Live-test strategy, below, for the open question of what cluster
  those cases actually target).

### Design test (carried over from M8/M8B's own ledger, unchanged)

At the end of M9, adding a further guarded Flux/Argo/Helm action should
still look like: 1) parse the semantic user action, 2) validate its
specific arguments, 3) build a `MutationIntent` (or an explicit composite
sequence, if the action is genuinely composite), 4) provide a semantic
preview renderer, 5) hand it to the existing M7 infrastructure. It should
NOT require a new confirmation subsystem, a new policy subsystem, a new
journal, a new transport safety model, a new UID model, or new TOCTOU
logic, and it should NOT require shelling out to `flux`/`argocd`/`helm`.
If any M9 action needs one of those, stop and reconsider before
implementing — the design is wrong.

## Explicit design rule (restated verbatim from the M9 kickoff prompt)

M9 must not turn SAUR-ON into "a TUI that shells out to flux/argocd/helm".
It must remain "a Kubernetes/operator console with first-class,
evidence-backed integrations and guarded semantic actions." If any
requested integration would require bypassing policy, confirmation,
identity, journal, verification, or no-retry semantics, this document
records that mismatch explicitly instead of working around it (see Open
questions).

## Safety boundary (unchanged from M8/M8B, restated for this milestone)

Production remains READ-ONLY. Every M9 guarded action — Flux
reconcile/suspend/resume, Argo CD sync/refresh/(rollback if implemented),
Helm rollback/uninstall — is strictly forbidden against production,
exactly like every M8/M8B operation was. All M9 live writes use only an
isolated test cluster via `scripts/test-cluster.sh`'s verified Docker/API
identity guard, gated by the same explicit, non-heuristic
`mutation_test_cluster_verified` flag. Never infer authorization from
context name. Never push/publish anything without explicit authorization.

## Explicit non-goals for M9

- **Git repository writes.** M9 never writes to Git (no commits, no PRs,
  no branch operations) — Flux/Argo CD reconciliation and sync act on
  already-committed Git/OCI/Helm-repo state; M9 only requests/observes
  that reconciliation, it never originates the change being reconciled.
- **Direct chart/repo authoring.** No chart scaffolding, no values-file
  editing, no repository-add/remove UI. Helm read-only inspection covers
  already-installed releases only.
- **Generic arbitrary Helm install/upgrade with free-form values input.**
  Only in scope if M9.6's design proves a safe, bounded input model exists
  (see M9.6's own contract); otherwise explicitly deferred, not faked.
- **ApplicationSet as a first-class managed object** (only Application is
  required; ApplicationSet read support is opportunistic, not required).
- **A generic "provider framework"** for arbitrary future integrations —
  that architecture-generalization step belongs closer to M12, per the
  kickoff prompt's own instruction. M9.0 builds only what concretely
  reduces duplication across exactly the three integrations in scope.
- **Bulk operations** (multi-select reconcile/sync/rollback across many
  objects) belong to the bulk-operations milestone (M10), never folded
  into M9.
- **Plugin/headless/scripted execution** belongs to M12.
- **kubectl/flux/argocd/helm CLI passthrough** of any kind — every read
  and every action goes through the Kubernetes API (or, where genuinely
  unavoidable for Argo CD's own operation semantics, an explicit adapter
  under the same safety gateway — see M9.4) — never a spawned CLI
  subprocess.

## Proposed slice ledger

| Slice | Scope | Verdict |
| --- | --- | --- |
| M9.0 | Shared integration identity/capability-discovery model | ACCEPTED |
| M9.1 | Flux read-only views (Kustomization, HelmRelease, GitRepository, OCIRepository, HelmRepository, Bucket, Image* if available) | ACCEPTED |
| M9.2 | Flux guarded actions (reconcile, suspend, resume) | ACCEPTED |
| M9.3 | Argo CD read-only views (Application, opportunistic ApplicationSet) | ACCEPTED |
| M9.4 | Argo CD guarded actions (sync, refresh, rollback if a safe API path exists) | ACCEPTED |
| M9.5 | Helm read-only inspection (releases, history, secret-safe values/manifest view) | ACCEPTED |
| M9.6 | Helm guarded actions (rollback, uninstall; upgrade only if a safe bounded input model is found) | DEFERRED (no safe native execution path found; see M9.6's own write-up) |
| M9.7 | Combined acceptance, full M1-M8B regression, soak | ACCEPTED |

## Per-slice contracts

### M9.0 — Shared integration model

**Scope, deliberately small** — this is a capability-discovery/identity
layer, not a provider framework:

- **Integration identity**: `Integration::{Flux, ArgoCd, Helm}` — an
  explicit, closed enum, never a free-form string.
- **Capability discovery**: for each integration, an explicit list of
  expected CRD kinds (group + kind), checked against the already-fetched
  `kube::discovery::Catalog` (no new network call — discovery already
  happened at connect time, exactly like every other resource kind's own
  presence). A new `Catalog::group_kind(group, kind)` exact-identity
  lookup is added (small, integration-agnostic, useful independent of
  M9) since `Catalog::resolve` is fuzzy/human-name-based and wrong for
  "does this exact GVK exist".
- **Unsupported/unknown state**: reuses `evidence::Unknown` directly
  (`Unsupported` = zero expected kinds found, `Partial` = some but not
  all) — no new parallel enum, since the semantics genuinely overlap
  with what `Unknown` already models everywhere else in the app.
  Absence of a CRD must never render as Healthy/Zero; a partial
  installation must never silently degrade to fully-absent or
  fully-available.
- **Bounded read requests**: no new transport code in M9.0 — every future
  read (M9.1/M9.3/M9.5) reuses the existing `kube::relationships::
  read_bounded` convention (byte-capped streaming, 200-item list caps,
  explicit `Unknown` classification per HTTP status) already proven by
  M6. Documented here as the convention every M9 read must follow, not
  reimplemented.
- **Namespace/context awareness**: no new code — every Flux/Argo/Helm
  object is, from the app's perspective, just another catalogued
  resource kind, so it already inherits the existing namespace/context
  switching, history, and picker behavior for free once `M9.1`/`M9.3`/
  `M9.5` register their kinds' presence.
- **Shared rendering conventions**: `integrations::view::discovery_report`
  — a small, pure text renderer (TARGET-style: integration name, overall
  state, per-kind present/absent list) that every later slice's own
  `:flux`/`:argocd`/`:helm`-style capability view can reuse verbatim,
  mirroring `mutation::view::policy_report`'s own role.
- **Shared journal correlation convention** (documentation only, zero new
  code — the existing journal is already generic enough): every M9
  guarded action's `source_action` is prefixed by its integration
  (`flux_reconcile`, `flux_suspend`, `flux_resume`, `argocd_sync`,
  `argocd_refresh`, `argocd_rollback`, `helm_rollback`, `helm_uninstall`),
  and the journal's existing `resource`/`namespace`/`name` fields already
  disambiguate which integration produced a record via the object's own
  GVK — no new journal field is needed.
- **Common verification vocabulary**: reused only where semantics
  genuinely overlap today (`Verification::{Pending, Verified,
  ObservedDifferent, Unknown, TargetReplaced}` already fit "did the
  suspend field flip"/"did a fresh read match"). The **new** distinction
  M9.2/M9.4 need — "request accepted" vs "controller convergence
  observed" vs "workload health" as three separate, never-collapsed
  facts — is *not* added in M9.0; it is explicitly deferred to M9.2 as an
  open question (see below), since inventing that vocabulary before a
  concrete action needs it risks guessing wrong.

**Non-goals for M9.0**: no UI wiring (no `:flux`/`:argocd`/`:helm`
commands yet — that is M9.1/M9.3/M9.5's job, once real objects exist to
show), no network code beyond the zero-new-network discovery model above,
no generic provider trait/registry.

### M9.1 — Flux read-only

List/inspect via the Kubernetes API directly, no `flux` CLI shell-out,
ever. Kinds (checked via M9.0's discovery, each independently optional):
`Kustomization`, `HelmRelease`, `GitRepository`, `OCIRepository`,
`HelmRepository`, `Bucket`, and `ImageRepository`/`ImagePolicy`/
`ImageUpdateAutomation` if the installed API supports them.

Surface: `generation`/`observedGeneration`, Ready/Reconciling/Stalled-
style conditions (rendered as evidence text, not fed into a second health
engine), last applied/attempted revision where available, suspend state,
dependencies, source references, inventory/resources where the CRD
provides them.

Connect into Adjacent/Xray via explicit references/provenance only
(`spec.sourceRef`, `spec.dependsOn`, inventory entries) — never a
name-based guess. Explicit handling required for: CRD absent, API version
mismatch, partial/malformed status, stale `observedGeneration` (i.e.
`status.observedGeneration != metadata.generation`, a real "conditions
are stale" fact, not silently treated as current), missing referenced
source, and same-name/new-UID replacement (reused UID model, not
reinvented).

No Git writes, no direct Git content editing, no `flux` CLI shell-out.

### M9.2 — Flux guarded actions

Candidates: `reconcile`, `suspend`, `resume`.

- `suspend`/`resume`: a `MutationEffect::Modify` on `spec.suspend`
  (boolean), following the exact same shape M8B.1's Cordon/Uncordon
  already established (`set_unschedulable`'s own pattern, generalized to
  "set a named boolean field"). Verification checks the exact field
  value post-commit, reusing `kube::mutation::verify`'s existing
  boolean-`omitempty` handling (the exact fix M8B.1 already made for
  Node's `spec.unschedulable` — Flux's `spec.suspend` is the same shape
  of Go-zero-value-omitted field, so this is expected to need zero new
  verification code, restated here as a design test for M9.2's own
  implementation to confirm or refute).
- `reconcile`: Flux's own documented control surface is the
  `reconcile.fluxcd.io/requestedAt` annotation (set to the current
  timestamp) — a `MutationEffect::Modify` on an annotation, structurally
  identical to M8's own Restart (`kubectl.kubernetes.io/restartedAt`).

**Critical semantic distinction, restated from the kickoff prompt**:
"reconcile requested" != "reconciliation completed successfully".
Verification must report three separate facts, never collapsed into one
"Success":
1. mutation/request accepted (`MutationOutcome`, exactly as today),
2. fresh Flux object state (`Verification`, exactly as today — did the
   annotation/field actually change),
3. eventual controller convergence/health (Flux's own Ready condition,
   rendered as a distinct, separately-labeled fact — never claimed by
   (1) or (2) alone, and never polled-for/awaited: SAURON reports what a
   single fresh read shows, exactly like M8.5's verification model
   already does for everything else — no new polling loop).

No automatic retries, matching every other M7/M8/M8B action.

### M9.3 — Argo CD read-only

`Application` first; `ApplicationSet` opportunistically, never forced.
Surface where available: source(s), destination, sync status, health
status, `targetRevision`, currently synced revision, operation state,
conditions, managed-resource summaries, automated sync policy, suspension
equivalent if the installed CRD/version supports one, source/destination
relationships. Version-tolerant parsing (Argo CD's CRD has changed shape
across versions) with explicit `Unsupported` when expected fields/API are
genuinely unavailable — never a guessed default.

Integrate into Adjacent/Xray through explicit references only
(`spec.source(s)`, `spec.destination`, `status.resources`). No causal
claims from topology alone. No `argocd` CLI shell-out.

### M9.4 — Argo CD guarded actions

Candidates: `sync`, `refresh`, `rollback`.

**RESOLVED 2026-09-20 (Open questions 2/3): no provider-action execution
adapter is needed.** All three actions are plain CRD-level operations,
confirmed live against a real Argo CD `v3.5.3` install:
- **sync**: a `MutationEffect::Modify` PATCH setting `Application.
  operation.sync` (top-level field, sibling to `spec`/`status`) — the
  application controller performs the sync and reports progress/result
  in `status.operationState`. No Argo API server call.
- **refresh**: a `MutationEffect::Modify` PATCH setting the documented
  `argocd.argoproj.io/refresh: normal|hard` annotation — structurally
  identical to Flux's own reconcile annotation (M9.2); the controller
  consumes and clears it itself.
- **rollback**: the exact same `operation.sync` field, with an explicit
  `revision` naming an already-recorded entry from `status.history`
  (never a hand-reconstructed prior spec, never a free-form/unverified
  revision string) — this is Argo CD's own underlying rollback
  primitive, not a SAURON-invented shortcut. **Rollback is not
  deferred.**

Every action still goes through the same policy/confirmation/identity/
journal/verification gateway, exactly like every M7/M8/M8B/M9.2 action —
no new dispatch branch in `kube::mutation`'s executor was needed either,
by the same reasoning M9.2 already established for Flux (confirm or
refute this explicitly once implemented, per this document's own design
test discipline).

**Critical semantic distinctions, restated from the kickoff prompt**:
"sync requested" != "Application synced"; "refresh requested" != "desired/
live converged"; "rollback requested" != "rollback completed
successfully". Verification reports request acknowledgement separately
from observed Application status, mirroring M9.2's own three-fact model
(commit/verify facts stay in `MutationOutcome`/`Verification`; Argo CD's
own `status.sync`/`status.health`/`status.operationState.phase`, read
fresh via M9.3's own status renderer, are the separate later fact).

### M9.5 — Helm read-only

No `helm` CLI shell-out unless a concrete investigation proves there is
no reasonable native path — and if that investigation happens, its
finding is written here *before* reaching for a shell-out, not after.
Current expectation: Helm release state lives in `Secret`/`ConfigMap`
objects (`sh.helm.release.v1.<name>.v<revision>`) in the release
namespace, readable via the same Kubernetes API path every other resource
already uses — no CLI needed for inspection.

Surface: release name, namespace, revision, status, chart, app version
where available, history (prior revisions), values summary, manifest
metadata/owned resources where safely derivable, notes if safe,
relationship to Kubernetes resources.

**Security — the load-bearing requirement of this slice**: Helm release
data is gzip+base64-encoded inside the storage object and commonly
contains sensitive values. SAUR-ON must never leak a decoded secret value
just because it decoded a Helm release record to read metadata.
Requirements: secret-safe redaction by default; the mutation/evidence
journal never contains decoded secret payloads; no raw Secret dumps; no
accidental terminal exposure of credentials/tokens sourced from values or
rendered manifests; bounded decoding (size caps, matching `read_bounded`'s
own byte-cap discipline); explicit failure/partial state if release data
cannot be decoded safely (never silently show nothing and never silently
show everything). Values view defaults to redacted; sensitive-looking
keys (`password`, `token`, `secret`, `key`, `credential`, similar
heuristics) are masked by default, with decoding never assumed safe just
because it succeeded. Manifest view may show resource identity/ownership
metadata; it must never show a `Secret`'s own data/stringData body.

Integrate Helm-owned resources into Adjacent/Xray only via evidence/
provenance (the `app.kubernetes.io/managed-by: Helm` + release name/
namespace labels Helm itself sets, or owner references where Helm sets
them) — never a heuristic name-prefix guess.

#### M9.5 security design decision: a narrow, explicit exception, not a relaxation

Implementation surfaced a genuine architectural conflict: `resources::
Object::new()` unconditionally calls `safety::redact()`, which blanks
every Secret's `data`/`stringData` to `<redacted>` at construction —
meaning the generic watch/Store/Object pipeline can *never* carry a Helm
release Secret's real body (a Helm release lives entirely inside one
such Secret's `data.release` field). This is deliberate, pre-existing,
load-bearing behavior (reinforced independently by
`kube::relationships::fetch_target`'s own metadata-only Secret fetch) —
not a bug to route around. Per this document's own "stop before an
architectural/safety decision" governance, this was raised and decided
explicitly rather than guessed at.

**Decision (verbatim, as directed): treat this as a narrowly-scoped
security capability, not as a relaxation of the generic Secret
invariant.** The old absolute statement "SAUR-ON never reads a Secret
body" becomes:

> SAUR-ON's generic object model never reads or retains Secret bodies;
> the explicit Helm-release inspection capability may transiently read
> one exact verified Helm release Secret under a bounded, non-
> persistent, sanitizing path.

Restated as the load-bearing contract (also carried verbatim as this
crate's own doc comments on `integrations::helm` and `kube::helm`):

> SAUR-ON's generic object/evidence pipeline never reads, stores,
> propagates, or renders Secret bodies. A single explicit, bounded
> Helm-release reader may transiently fetch one exact Secret body only
> when the user explicitly requests Helm inspection of a previously
> identified Helm release.

What stays unchanged, with zero exception: `Object::new()` continues to
redact Secret `data`/`stringData` unconditionally; `Store`/watch/list/
`fetch_target` gain no Helm-specific carve-out; there is no generic
"fetch a raw Secret" helper any other caller could reach for.

What the one narrow exception looks like, as implemented:

- **Dedicated module/path** — `kube::helm::read_release` is the *only*
  function in the crate that ever calls
  `integrations::helm::decode_release` (kept `pub(crate)`, never `pub`,
  so nothing else can reach it). It never routes the raw Secret through
  `Object::new`, `Store`, `Timeline`, the graph, the mutation journal, or
  any generic evidence structure.
- **Explicit user request only** — invoked only from
  `app::open_helm_view`'s own `:helm` command handler, spawned as its
  own one-shot task exactly like `start_adjacent`'s established
  convention (reusing the existing `Payload::Document`/
  `Payload::DocumentError` events — no new plumbing). Never triggered by
  selection, watch, or any background path.
- **Exact identity / TOCTOU** — `open_helm_view` uses the already-
  selected (already-redacted) object only to know it is a `Secret` and
  to identify *which* one (namespace/name/UID) to fetch — its own
  `type` field is presentational only and is deliberately **not**
  trusted as authorization (a plain `Opaque` Secret is still allowed to
  reach the fetch; proven by
  `helm_view_does_not_trust_the_cached_type_field_the_fresh_fetch_re_
  verifies_it`). `read_release` performs one bounded GET and
  independently re-verifies, against that *fresh* response: UID must
  still equal the expected UID (else `TargetReplaced`, fail closed —
  proven live by `live_stale_uid_against_a_real_secret_fails_closed`
  and via fake-HTTP by
  `helm_read_release_fails_closed_on_uid_mismatch_never_decoding_a_
  replaced_object`), and `type` must still be `helm.sh/release.v1`
  (else `WrongType` — proven via fake-HTTP by
  `helm_read_release_rejects_a_secret_that_is_not_a_helm_release_
  fresh_check_not_cached`). Only then is `data.release` decoded.
- **Bounded raw handling** — the HTTP response itself is bounded by the
  existing `read_bounded` 2MB single-object cap (reused unchanged, zero
  new HTTP code); the base64-decoded bytes and the gunzipped bytes each
  have their own independent bound
  (`integrations::helm::MAX_DECOMPRESSED_BYTES` = 8MB, matching
  `read_bounded`'s own list-cap convention), since gzip can expand a
  small compressed payload far past the HTTP cap — proven by
  `decode_release_refuses_a_decompression_bomb_beyond_the_bounded_cap`.
  No automatic retry. Every distinct failure (missing field, bad
  encoding, oversized/corrupt compression, non-JSON-object content) is
  its own explicit `DecodeError`/`HelmReadError` variant — never
  collapsed into a false "empty release".
- **Raw material lifetime** — the raw fetched Secret JSON, `data.release`,
  the decoded/decompressed bytes, and the unredacted release record are
  all local to `read_release`'s own async block and are dropped when it
  returns. No persistent cache, no `Store` insertion, no journal entry,
  no tracing/debug formatting of raw content anywhere in the path.
  Every `HelmReadError` variant's `Display` is a fixed, enum-tag-derived
  message with no field that could ever carry payload bytes — proven
  structurally (no such field exists) and at runtime by
  `error_display_never_embeds_raw_payload_content`.
- **Sanitization boundary** — the only thing that crosses the module
  boundary outward is `integrations::helm::HelmReleaseView`, produced by
  `sanitize()` immediately after decode. There is no
  `DecodedHelmRelease`-shaped type exposed anywhere else in the crate.
- **Values** — `sanitize()`'s internal `redacted_values` masks
  recursively at any JSON depth via case-insensitive substring matching
  (`password`, `token`, `secret`, `credential`, `private-key`,
  `apikey`, `passphrase`, and the deliberately broad `key`, per the
  explicit instruction to cover "password/token/secret/key/credential
  and reasonable variants") — proven by
  `sanitize_masks_sensitive_keys_at_any_depth_never_the_rest`. This
  masking happens inside the module, before the view is returned; no
  renderer downstream is trusted to redact later.
- **Manifest** — `manifest_resources` extracts only
  `apiVersion`/`kind`/`metadata.{name,namespace}` per rendered document
  (bounded to 200 documents); a `Secret`-kind manifest entry contributes
  only that identity, never its `data`/`stringData` (those fields are
  never even parsed) — proven by
  `sanitize_extracts_manifest_identity_only_never_secret_data`.
- **Notes are deliberately never shown** — `info.notes` is investigated
  and rejected as unsafe to surface: it is freeform chart-author prose
  (NOTES.txt) that real-world charts commonly interpolate generated
  credentials into (e.g. an auto-created database password echoed back
  to the installer), and unlike `values`/`manifest` there is no
  structured key to redact by — there is no reliable, general
  sanitization contract for arbitrary text. `HelmReleaseView` has no
  `notes` field at all (a compile-time guarantee, restated at runtime by
  `sanitize_never_carries_notes_into_the_view` and
  `status_report_never_contains_the_raw_secret_value_or_notes`).

Evidence: 9 unit tests in `integrations::helm` (decode round-trip;
every distinct decode failure mode; decompression-bomb refusal; values
masked at any depth; manifest identity-only extraction; notes never
carried into the view; `status_report` never contains a raw secret
value or notes; explicit "none reported" state when values are absent)
plus 1 in `kube::helm` (error `Display` never embeds raw payload); 3
app-level tests (`:helm` requires selecting a *Secret* first — a
presentational, pre-fetch check only; the cached `type` field is never
trusted as authorization, only the fresh fetch's own re-verified type
is; `:helm` on a plausible release Secret spawns exactly one bounded
fetch task and never attempts to decode the already-redacted cached
copy); 4 fake-HTTP tests in `tests/watch_transport.rs` against a real
HTTP endpoint (real decode+sanitize round trip against a live-shaped
response; UID-mismatch fails closed; wrong-type fails closed even with
a release-shaped name; malformed encoding is an explicit error, never
an empty view); 4 live tests in `tests/mutation_m9_helm_live.rs` against
two real, independently-created Helm releases on `sauron-m9`
(`demo-release`, installed via the real `helm` CLI as pure test-harness
tooling — see this document's own established convention for that
distinction — and `podinfo-helm`, created by Flux's own `HelmRelease`
from `tests/fixtures/m9-flux.yaml`): both real releases decode and
sanitize correctly, a real-but-wrong UID fails closed, and a
nonexistent release name is an explicit error, never a fabricated view.
Full locked suite green (305 unit + 74 fake-HTTP), `cargo fmt --check`
and `cargo clippy --all-targets -D warnings` clean, full M1-M8B
interactive regression (`accept-m8b.py`) reconfirmed green on the
untouched `kind-sauron-test` cluster, and Flux/Argo CD's own live test
suites (9 tests) reconfirmed green on `sauron-m9` — zero drift from
touching `src/app/mod.rs`.

### M9.6 — Helm guarded actions — DEFERRED (2026-09-20, evidence-backed architectural decision)

**Status: explicitly DEFERRED, not implemented, not attempted as a partial
approximation.** This is a decided scope outcome of this milestone's own
required investigation, not an omission.

Candidates were `rollback` and `uninstall` (upgrade was already scoped
out of M9's first pass per this section's original text, for unrelated
reasons — no safe bounded input model for arbitrary chart/values input).

**Investigation (Open question 4, per this document's own explicit
"must-stop" list)**: before writing any mutation code, a dedicated
investigation asked whether a safe native (non-shell-out) Rust execution
path exists for Helm's own rollback/uninstall lifecycle. Findings:

- **No maintained Rust crate implements Helm's client-side action logic**
  (the equivalent of Go's `helm.sh/helm/v3/pkg/action`). The only
  Helm-related crates found (`helm-api`, last published 2019, targets
  the pre-Helm-3 protobuf/Tiller wire format; a `helm-wrapper-rs` that
  itself shells out to the `helm` binary) are either dead or themselves
  violate this project's no-shell-out rule.
- **Helm v3+ has no server/API component to call instead.** Tiller
  (Helm v2's in-cluster gRPC server) was permanently removed in Helm v3
  (2019) and remains removed through Helm 4 — confirmed via Helm's own
  "Changes Since Helm 2" documentation. There is no RPC endpoint to
  target as an alternative to embedding logic or shelling out.
- **Reimplementing "just enough" of rollback/uninstall directly against
  the `kube` crate + the release Secret storage carries real, specific
  hazards to Helm's own bookkeeping**, not just this app's own
  correctness: incorrect revision-number sequencing, status-enum
  transitions, or malformed stored release records can cause the real
  `helm` CLI to misbehave or refuse to operate on that release
  afterward; rollback's actual semantics depend on which apply strategy
  (three-way strategic-merge vs. Server-Side Apply, per HIP-0023) the
  release was previously managed under, and getting this wrong causes
  field-ownership conflicts on the next real `helm upgrade`; hook
  lifecycle (pre/post-rollback, pre/post-delete, weights, per-hook
  delete policies) and `helm.sh/resource-policy: keep` filtering are
  both load-bearing for correctness and easy to get subtly wrong in a
  from-scratch reimplementation; wrong deletion-propagation choice on
  uninstall risks deleting resources (e.g. PVCs) Helm itself would have
  preserved.

**Decision**: none of the three theoretically available paths are
acceptable inside M9's scope:
1. Shelling out to the `helm` CLI — explicitly disallowed by this
   milestone's own kickoff instructions (same rule as Flux/Argo CD).
2. Directly manipulating Helm's release Secret/ConfigMap storage records
   as an implementation shortcut — explicitly disallowed by this
   section's own original text, since it bypasses Helm's own lifecycle/
   bookkeeping guarantees.
3. A partial/"simple releases only" native reimplementation (e.g.
   detecting and only handling releases with no hooks and no
   `resource-policy: keep`) — rejected even as a reduced-scope option:
   it would still write Helm's own bookkeeping records by hand, still
   carries the revision/apply-strategy hazards above, and would let
   this app silently claim a form of Helm lifecycle compatibility that
   has not been demonstrated, which this project's own honesty
   discipline (`UNKNOWN != ZERO`-style precision) does not accept.

**M9.5 (read-only Helm inspection) is therefore the final Helm
deliverable for M9** — full release decode, secret-safe values/manifest
rendering, the bounded/TOCTOU-safe raw-body-read exception, history/
status/chart metadata — exactly as already implemented and ACCEPTED
above. No Helm mutation surface (`:helm_rollback`/`:helm_uninstall`)
exists in M9; none is implied to exist.

**Future work (explicitly out of scope for M9, tracked as backlog, not
started here)**: a possible dedicated future milestone, "Native Helm
Engine," could deliberately implement Helm-compatible release lifecycle
behavior in Rust as its own substantial piece of engineering — release
storage/history semantics, revision numbering, the full
deployed/superseded/failed/pending-* status state machine, rollback and
uninstall lifecycles, hooks and hook ordering, hook delete policies,
`resource-policy: keep`, three-way-merge/SSA-equivalent apply behavior,
deletion propagation, wait/timeout semantics, and failure/partial-
operation behavior. The critical design rule for that future work, if
undertaken: **`LOOKS EQUIVALENT != BEHAVES EQUIVALENT`** — producing the
same apparent end state is not the bar; remaining compatible with
Helm's own expectations (so the real `helm` CLI can keep operating on a
release SAUR-ON has touched) is. The recommended acceptance strategy for
that future milestone is a differential one, using the real `helm` CLI
as a **test oracle only** (never as SAUR-ON's own runtime
implementation): perform the same fixture operation via real `helm` and
via SAUR-ON's native engine, then compare resulting resources, release
revision, release status, release history, hooks, stored release
metadata, failure behavior, and — critically — whether the real `helm`
CLI can still successfully operate on the release afterward. That
milestone does not begin as part of M9, and M9's own acceptance does not
wait for it.

### M9.7 — Combined acceptance, full M1-M8B regression, soak

Mirrors M8B.7's own structure. Required coverage: full M1-M8B regression
(every existing `accept-*.py` script, unmodified); Flux read + guarded
actions; Argo CD read + guarded actions; Helm read-only inspection (M9.6
guarded actions are explicitly DEFERRED per that section's own
evidence-backed write-up — M9's acceptance does not wait for them);
readonly zero-write checks per integration; replacement/TOCTOU rejection;
controller-unavailable/CRD-absent handling; partial/malformed status
handling; cancellation; 32x9; terminal restoration; M4 forward held
throughout if practical; a soak rotating through bounded integration
operations with the same self-restoring/no-drift discipline M8.6/M8B.7
established (per-section independent error handling from the start, not
discovered as a bug mid-run); RSS/fd/thread/metrics observations with the
same "observed stability only, no leak-freedom claims" honesty. Run
combined acceptance twice. Then reconcile `HANDBOOK.md`,
`docs/RUNBOOK.md`, `README.md`, this document; clean worktree; local
annotated tag `m9-accepted`; never pushed without explicit authorization.

## Live-test strategy (RESOLVED 2026-09-19 — option (b), dedicated cluster)

**Decision**: option (b) — a dedicated, disposable second kind cluster
for GitOps controller-backed live evidence. `kind-sauron-test` is never
touched by Flux or Argo CD installation and remains exactly as it is for
M1-M8B regression. Rationale: installing full Flux and/or Argo CD
controllers into the existing cluster risks destabilizing the M1-M8B
regression fixtures/namespaces it already hosts (resource pressure,
CRD/webhook interactions, controller-owned namespaces) — a dedicated
cluster removes that risk entirely rather than accepting or mitigating
it.

### M9 dedicated cluster lifecycle

- **Cluster**: `sauron-m9` (Docker container `sauron-m9-control-plane`,
  same `kindest/node:v1.33.1` image and `tests/fixtures/kind.yaml`
  config as `kind-sauron-test` — no new kind config needed).
- **Context**: `kind-sauron-m9`.
- **Kubeconfig**: `.test-cluster-m9/config` (dedicated, `chmod 600`,
  gitignored — mirrors `.test-cluster/config` exactly, never the
  default `~/.kube/config`).
- **Create**: `scripts/bootstrap-test-cluster-m9.sh` — mirrors
  `scripts/bootstrap-test-cluster.sh` verbatim (refuses a non-local
  Docker endpoint, backs up any previous dedicated kubeconfig under
  `.test-cluster-m9/previous.*` rather than overwriting silently,
  `kind create cluster --name sauron-m9 ... --kubeconfig
  .test-cluster-m9/config`, `chmod 600`). Idempotent: if the container
  already exists, it just re-verifies instead of recreating.
- **Verify (the identity guard)**: `scripts/test-cluster-m9.sh` — the
  exact same three-part guard `test-cluster.sh` uses for
  `kind-sauron-test`, restated for this cluster: (1) the dedicated
  kubeconfig file must exist, (2) `docker inspect`'s
  `io.x-k8s.kind.cluster` label on `sauron-m9-control-plane` must equal
  `sauron-m9` (never inferred from the container *name* alone), (3) the
  kubeconfig's own recorded API server URL must exactly equal
  `https://<the Docker-reported loopback port binding>` for that same
  container. All three must hold before any command in the script runs
  — exactly the non-heuristic, external-proof model `test-cluster.sh`
  already established, applied to a second cluster rather than weakened
  or shortcut for it. **No mutation authorization ever comes from the
  context name string** — the app's own `mutation_test_cluster_verified`
  flag (set only via the explicit `--mutation-test-cluster-verified` CLI
  flag when launching against this cluster) is the actual authorization,
  identical to every M7/M8/M8B live test's own model; this guard script
  is what makes it *safe* to pass that flag, not what grants it.
- **Install Flux**: `scripts/test-cluster-m9.sh flux-install` — fetches
  the pinned `v2.9.5` official install manifest once (cached under
  `.test-cluster-m9/flux-install-v2.9.5.yaml`, gitignored) and applies
  it with plain `kubectl apply -f` — no `flux` CLI involved at all, not
  even for cluster setup (consistent with M9's own "never shell out"
  principle, and with `test-cluster.sh`'s own precedent of installing
  `metrics-server` via a plain kustomize/kubectl path, not a vendor
  CLI). Waits for `source-controller`/`kustomize-controller`/
  `helm-controller`/`notification-controller` rollouts before returning.
- **Fixtures**: `scripts/test-cluster-m9.sh flux-fixtures` — applies
  `tests/fixtures/m9-flux.yaml` (a real `GitRepository` against a small
  public repo, a `Kustomization` reconciling from it, a `HelmRepository`,
  and a `HelmRelease` — real, controller-reconciled objects, not
  hand-crafted status stand-ins, since the whole point of the dedicated
  cluster is real controller-backed evidence).
- **Reset**: `scripts/test-cluster-m9.sh flux-reset` — re-applies the
  same fixture file (Flux's own reconciliation is idempotent and
  self-correcting, so re-apply is sufficient; no delete+recreate dance
  needed, unlike M8B's disposable-Pod fixtures).
- **Live tests**: `scripts/test-cluster-m9.sh flux-test` — runs
  `cargo test --test mutation_m9_live -- --ignored --nocapture` against
  `SAURON_TEST_M9_KUBECONFIG`, mirroring `m8b-test`'s own convention
  exactly (a distinct env var name so a stray unset `SAURON_TEST_
  KUBECONFIG` can never accidentally point M9 live tests at
  `kind-sauron-test` instead).
- **Destroy**: `kind delete cluster --name sauron-m9` (manual, not
  wrapped in a script — destruction is rare enough, and dangerous enough
  as a scriptable one-liner, that it stays an explicit manual command
  exactly like nothing in this repo auto-deletes `kind-sauron-test`
  either).
- `scripts/test-cluster.sh` (the `kind-sauron-test` guard) is
  **untouched** — no new cases, no shared code path, no parameterization
  that could weaken its own guard for the M1-M8B cluster. The two
  scripts are siblings, not a shared abstraction, specifically so a bug
  in the M9 script can never affect the M1-M8B script's own behavior.

### Environment finding recorded during first bootstrap (classified: environment, not app)

The first `flux-install` attempt failed: every Flux controller entered
`CrashLoopBackOff` with **zero log output**, and `kube-apiserver` itself
logged `error creating fsnotify watcher: too many open files` while
CoreDNS could not even reach the API server's ClusterIP. Root cause:
`fs.inotify.max_user_instances` was at the Linux default (128) — too low
to run two kind clusters' control planes simultaneously on this host
(each apiserver/kubelet/controller creates its own inotify watchers for
cert/CA file rotation). This is a well-known kind-on-Linux host
requirement, not a SAURON defect, not a Flux defect, and not a memory
problem (RAM headroom was ample throughout). Fixed by raising
`fs.inotify.max_user_instances` to 512 and `fs.inotify.max_user_watches`
to 524288 (both `sysctl -w` immediately and persisted in
`/etc/sysctl.d/99-sauron-kind.conf`), then destroying and recreating
`sauron-m9` fresh so every control-plane component started clean under
the corrected limit (raising the limit alone did not self-heal the
already-degraded first attempt — the API server's own retry loop had
already left it in a bad state). After recreation, all 7 Flux
controllers reached `Running`/`Ready` with zero restarts. Recorded here
as a **host prerequisite** for anyone reproducing M9's live evidence,
not as a workaround baked into any script (the sysctl change is a
one-time host setup step, correctly outside `bootstrap-test-cluster-m9.sh`
itself, which does not require elevated privileges).

Helm needs no controller at all (`helm` itself is a client-side
operation against Kubernetes storage objects), so Helm's live evidence
(M9.5/M9.6) uses a real, disposable Helm release installed into a
dedicated namespace on the existing `kind-sauron-test` cluster — far
lower risk than a reconciling controller, and no reason to place it on
`sauron-m9` instead.

Argo CD's own cluster placement — **RESOLVED 2026-09-20, user decision**:
alongside Flux, in the same `sauron-m9` cluster. Installed via the
official pinned `v3.5.3` install manifest (`kubectl apply -n argocd
--server-side --force-conflicts` — server-side apply is required because
the `ApplicationSet` CRD's schema exceeds client-side apply's
262144-byte `last-applied-configuration` annotation limit, a real,
documented Argo CD installation quirk unrelated to this repo). Confirmed
live: all 7 Argo CD components (`argocd-server`, `argocd-repo-server`,
`argocd-application-controller`, `argocd-applicationset-controller`,
`argocd-notifications-controller`, `argocd-redis`, `argocd-dex-server`)
reached `Running`/`Ready` with zero restarts, and Flux's own 7
controllers remained untouched and healthy throughout (checked
immediately after install and again after 22h of both running
side-by-side) — no CRD/webhook/resource-pressure interference observed.
If that ever changes, Argo CD moves to its own third dedicated cluster
with a documented reason; nothing here prevents that later.

## Open architectural questions requiring resolution before implementation

1. **Flux/Argo live evidence cluster** — **RESOLVED 2026-09-19**: a
   dedicated `sauron-m9` kind cluster (option (b), see Live-test strategy
   above), never `kind-sauron-test`. Flux is installed there now.
   1a. **Argo CD's own cluster placement** — **RESOLVED 2026-09-20, user
       decision**: alongside Flux in `sauron-m9` (see Live-test strategy
       above for the live confirmation that this caused no interference).
2. **Argo CD sync execution path** — **RESOLVED 2026-09-20**: the CRD
   alone is fully sufficient, no Argo API service call needed. Confirmed
   live: `Application`'s own top-level `operation` field (sibling to
   `spec`/`status`, schema-documented via `kubectl explain applications.
   argoproj.io.operation`) accepts a plain Kubernetes PATCH
   (`{"operation":{"sync":{"revision":"..."}}}`) and the application
   controller picks it up, runs the sync, and reports progress/result in
   `status.operationState` (`phase`: `Running`/`Succeeded`/`Failed`/
   `Error`, plus `syncResult.resources` per managed object) — proven by
   actually triggering a real sync this way against the `guestbook`
   fixture (created real `Service`/`Deployment` objects in `sauron-m9`).
   `refresh` is the same shape: the documented `argocd.argoproj.io/
   refresh: normal|hard` annotation, which the controller consumes and
   clears itself (confirmed live) -- structurally identical to Flux's
   own reconcile annotation. **This means M9.4 needs no provider-action
   execution adapter for sync/refresh** -- both are plain
   `MutationEffect::Modify` through the exact same gateway.
3. **Argo CD rollback feasibility** — **RESOLVED 2026-09-20**: the same
   `operation.sync` field accepts an explicit `revision` distinct from
   `spec.source.targetRevision`, which is exactly Argo CD's own
   underlying rollback primitive (`argocd app rollback` is a thin
   convenience wrapper over this same field, picking a revision from
   `status.history`) — no prior-spec reconstruction needed, no Argo API
   service call needed. M9.4 implements rollback as "sync to a specific,
   already-recorded prior revision from `status.history`", the same CRD
   path as sync, never a hand-reconstructed spec. Rollback is **not**
   deferred.
4. **Helm execution library** — a Rust Helm SDK crate, vs. a smaller
   hand-rolled client against Helm's own documented storage-record
   format that still respects Helm's lifecycle semantics (locking,
   history revision numbering) rather than bypassing them. Resolve at
   the start of M9.6 with a concrete crate/approach evaluation recorded
   here.
5. **New `Verification`/`MutationOutcome` vocabulary for "request
   accepted vs. controller converged vs. workload healthy"** —
   **RESOLVED 2026-09-19 by M9.2: the existing model already suffices,
   zero new variants needed.** `MutationOutcome`/`Verification` continue
   to describe only "was the request accepted" and "does a fresh read
   match the exact requested change" (unchanged meaning). The third fact
   -- controller convergence -- is presented by reusing M9.1's own
   `integrations::flux::status_report` on a fresh read of the same
   object, shown as separate, clearly-labeled context alongside the
   commit/verification result, never folded into either enum and never
   awaited/polled for. Confirmed live: a real run of
   `live_flux_suspend_resume_and_reconcile_round_trip_on_a_real_
   kustomization` caught the controller genuinely still reconciling
   (`Reconciling=True`, `Ready=Unknown`, a real `STALE`
   observedGeneration) immediately after a reconcile commit -- exactly
   the honest, un-awaited fact this design exists to surface, not a bug
   the test had to work around.
6. **Flux `ImageRepository`/`ImagePolicy`/`ImageUpdateAutomation` scope**
   — include only if the installed Flux version's API actually exposes
   them (image-automation controllers are a separate, optional Flux
   component); M9.1 checks via M9.0's discovery model rather than
   assuming presence.

## Bugs / limitations (placeholder)

None yet — implementation has not started. This section exists so future
implementation work has a designated place to record what M8B's own
ledger called "bug discipline": reproduce, classify (app/harness/fixture/
environment), root cause, regression-proof, full locked recheck, replay,
only then continue.

## Journal

- 2026-09-20: M9.0 (Shared integration model) implemented and ACCEPTED.
  Deliberately small, per this document's own contract:
  - `Catalog::group_kind(group, kind) -> Option<&Resource>` added to
    `kube::discovery` -- an exact-identity lookup, never fuzzy/alias/
    shortname matching like `resolve()`, since capability discovery needs
    "does this exact GVK exist", not "what does this human-typed name
    mean". Generic and integration-agnostic, useful independent of M9.
  - New `integrations` module: `Integration::{Flux, ArgoCd, Helm}` (a
    closed enum), `ExpectedKind` (label + group + kind), `Discovery`
    (per-expected-kind `Option<Resource>` plus a `state()` method), and
    `discover(catalog, integration, expected) -> Discovery` -- pure, zero
    network calls, since discovery already happened at connect time
    exactly like every other resource kind's own presence.
  - `Discovery::state()` reuses `evidence::Unknown` directly
    (`Unsupported` = zero expected kinds found, `Partial` = some but not
    all, `None` = fully available) -- no new parallel enum, per this
    document's own instruction to reuse shared vocabulary only where
    semantics genuinely overlap.
  - `integrations::view::discovery_report` -- a small, pure text renderer
    (integration name, overall state, per-kind present/absent list with
    each absence's own group/kind spelled out) for later slices'
    `:flux`/`:argocd`/`:helm`-style capability views to reuse verbatim.
  - Bounded reads, namespace/context awareness, and journal correlation
    needed **zero new code** in this slice, exactly as predicted: bounded
    reads reuse `kube::relationships::read_bounded` once M9.1/M9.3/M9.5
    need one; namespace/context awareness is inherited for free once a
    Flux/Argo/Helm kind is just another catalogued resource; journal
    correlation is a naming convention (`source_action` prefixed by
    integration) recorded in this document, not a code change, since
    `Record`'s existing `resource`/`namespace`/`name` fields already
    disambiguate by the object's own GVK.
  - The "request accepted vs. controller converged vs. workload healthy"
    verification vocabulary was explicitly **not** added here, per this
    document's own Open question 5 -- deferred to M9.2, the first slice
    that actually needs it.
  - No UI wiring in this slice (no `:flux`/`:argocd`/`:helm` command) --
    there is nothing real yet to show; that starts at M9.1.
  Evidence: 6 unit tests (`integrations::tests`: absent integration is
  `Unsupported` never `Healthy`/zero; fully-available integration is
  `None` state; partial installation is `Partial` and never silently
  collapsed either direction; an unrelated CRD sharing a Kind name but a
  different group never counts as present -- proving discovery is by
  `(group, kind)`, never `kind` alone; the capability report never claims
  presence without a real discovered `Resource`; integration labels are
  explicit and distinct); 1 unit test on `Catalog::group_kind` (exact
  match, never fuzzy, and a same-kind-different-group CRD never matches).
  Full locked suite: 252 unit + 66 fake HTTP, fmt/check/clippy (`-D
  warnings`) clean.
  Next: M9.1 (Flux read-only views), starting with resolving the
  Live-test-strategy open question for Flux specifically.
- 2026-09-19: Live-test strategy open question RESOLVED (option (b)) and
  the M9 dedicated cluster stood up, per explicit user instruction before
  M9.1 began. `scripts/bootstrap-test-cluster-m9.sh` and
  `scripts/test-cluster-m9.sh` added (siblings of the existing
  `kind-sauron-test` scripts, zero shared code path, zero changes to
  `scripts/test-cluster.sh` itself); `.test-cluster-m9/` gitignored. The
  guard script enforces the exact same three-part non-heuristic identity
  proof (kubeconfig file presence, Docker label, API-server-URL-matches-
  reported-port) as `test-cluster.sh`, applied to `sauron-m9`/
  `kind-sauron-m9` instead of `sauron-test`/`kind-sauron-test`.
  Real Flux `v2.9.5` installed via a pinned, cached, plain-`kubectl`-
  applied manifest (no `flux` CLI anywhere in the setup path).
  **Environment finding** (classified environment, not app -- see the
  Live-test strategy section above for the full account): the first
  install attempt left every Flux controller in a silent, log-less
  `CrashLoopBackOff` because `fs.inotify.max_user_instances` was at the
  Linux default (128), too low to run two kind control planes on this
  host at once. Fixed by raising it to 512 (`fs.inotify.max_user_watches`
  to 524288) and recreating `sauron-m9` fresh; all 7 controllers then
  reached `Running` with zero restarts. `kind-sauron-test` was verified
  unaffected throughout (`scripts/test-cluster.sh check` stayed green).
  Recorded as a one-time host prerequisite, not a script workaround.
  Next: M9.1 implementation proper (Flux read-only views), now unblocked.
- 2026-09-19: M9.1 (Flux read-only views) implemented and ACCEPTED.
  - **Listing/browsing needed zero new code**, exactly as this document's
    own contract predicted: Flux's CRDs (`Kustomization`, `HelmRelease`,
    `GitRepository`, `OCIRepository`, `HelmRepository`, `Bucket`, and the
    three Image* automation kinds) are just catalogued resources once
    discovered (M9.0's own `Catalog::group_kind`); M3's existing CRD
    `additionalPrinterColumns` support already renders each kind's own
    `STATUS`/`Ready` column in the generic table with no Flux-specific
    code -- confirmed live (`:kustomizations.kustomize.toolkit.fluxcd.io`
    showed a real `STATUS` column with `Ready=True` immediately).
  - **`integrations::flux`** added: `KINDS` (the 9 expected kinds for
    M9.0's discovery), `STATUS_KINDS` (the 6 with reconciliation-shaped
    status: `Kustomization`/`HelmRelease`/`GitRepository`/
    `OCIRepository`/`HelmRepository`/`Bucket`), and `status_report()` --
    a pure renderer showing every condition Flux reported (never
    filtered to just `Ready`; `HelmRelease` reports `Ready` AND
    `Released` as two separate facts, both shown), `observedGeneration`
    staleness (including Flux's own `-1` "never reconciled" sentinel,
    discovered live and shown distinctly from ordinary staleness),
    suspend state, revision fields, `dependsOn`, and the source
    reference -- all rendered verbatim as evidence, never fed into a
    second health engine. The three Image* automation kinds are
    discovered and generically browsable but do not get a dedicated
    status renderer in this pass (their own shape -- image scan
    results/policies -- is different enough to warrant separate design
    later): a bounded, documented limitation, not a silent gap.
  - **Adjacent/Xray integration** (`graph::references.rs`): a new
    `(group, kind)`-matched dispatch branch (group-only, version-
    tolerant, matching M9.0's own discovery precedent and Flux's real
    v1beta2->v1 migration history) extracts `Kustomization.spec.
    sourceRef`/`HelmRelease.spec.chart.spec.sourceRef` and both kinds'
    `spec.dependsOn`. **Real finding while building this against live
    data**: Flux does not persist a resolved `apiVersion` inside
    `sourceRef` at all (confirmed against a real `kubectl get -o json`)
    -- hardcoding one would silently break across Flux's own API version
    migrations. Resolved by reusing `Provenance::StatusReference`'s
    existing "kind is known, exact served version is not" exemption
    (already established by `storage.rs`'s own `claimRef` handling for
    the same underlying reason), rather than inventing a new provenance
    meaning or guessing a version. `dependsOn` has no such ambiguity
    (same kind/version as the referencing object) and uses a plain
    `ExplicitReference`.
  - **`:flux` command** added (palette-only, no bound key yet): shows
    the currently selected object's own Flux status
    (`integrations::flux::is_status_kind` gate) if one of the 6
    reconciliation kinds is selected, otherwise the M9.0 capability
    report for the whole cluster. Read-only, zero network beyond what
    discovery/watch already fetched.
  - **Live evidence**: real Flux `v2.9.5` on `sauron-m9`, real `podinfo`
    `GitRepository`/`Kustomization`/`HelmRepository`/`HelmRelease`
    (`tests/fixtures/m9-flux.yaml`, mirroring the standard Flux
    getting-started tutorial's own fixture shape) plus a deliberately
    suspended, never-reconciled `Kustomization` with a missing source and
    a real `dependsOn` edge, for exactly the "missing referenced source"/
    "never reconciled"/"dependsOn" coverage this document's own M9.1
    contract required. `tests/mutation_m9_live.rs` (4 tests, run twice
    clean via `scripts/test-cluster-m9.sh flux-test`): discovery reports
    every installed kind present; a `Ready` Kustomization's status and
    resolved `GitRepository` source reference (resolved against the
    REAL discovery catalog, not a fake one, proving the empty-
    `api_version`/`StatusReference` path genuinely works live); a
    `HelmRelease` showing both `Ready` and `Released`; the suspended
    Kustomization showing the `-1` sentinel, its real `dependsOn` edge,
    and proof that its missing source's KIND resolves (GitRepository is
    installed) while the named OBJECT genuinely does not exist (a real
    404, never fabricated). Full M1-M8B regression (`accept-m3.py`
    through `accept-m8b.py`) reconfirmed green -- zero drift from
    touching shared `graph::references.rs`/`app/mod.rs`.
  - **Interactive evidence**: a manual tmux smoke check (not a full
    `accept-m9.py` script, which is deferred to M9.7 per M8B's own
    precedent of deferring combined interactive acceptance to its `.7`
    slice) confirmed `:flux`'s status view, the generic table's own CRD
    printer columns, and Adjacent's live reference resolution all work
    end-to-end in the real terminal against `sauron-m9`.
  Evidence: 6 unit tests (`integrations::flux::tests`); 4 unit tests
  (`graph::references::tests`: sourceRef is a `StatusReference` with no
  guessed `api_version`; `dependsOn` targets the same kind/version
  unambiguously and honors an explicit namespace; HelmRelease's nested
  chart sourceRef path; an unrelated CRD sharing the `Kustomization`
  Kind name in a different group never matches); 2 app-level tests
  (`:flux` shows the capability report when Flux is absent, and the
  object status when a Flux kind is selected -- both issue zero network
  tasks); 1 grammar test; 4 live tests (above), run twice. No new fake-
  HTTP test: M9.1 introduces zero new network code (discovery reuses the
  connect-time `Catalog`, status rendering reads already-synced
  watch/store data) -- everything network-shaped is already covered by
  M6's own `read_bounded`/`resolve_target` fake-HTTP tests, reused
  as-is. Full locked suite: 265 unit + 66 fake HTTP, fmt/check/clippy
  (`-D warnings`) clean.
  Next: M9.2 (Flux guarded actions -- reconcile/suspend/resume),
  continuing automatically per this milestone's own instruction (no new
  architectural blocker identified).
- 2026-09-19: M9.2 (Flux guarded actions: reconcile, suspend, resume)
  implemented and ACCEPTED. No architectural blocker -- confirmed the
  design test in full: every action reuses the existing gateway with
  zero new policy/journal/transport/verification code.
  - **`mutation::workflow`**: `flux_suspend`/`flux_resume` are the exact
    same shape as M8B.1's `cordon`/`uncordon` (`set_unschedulable`
    generalized) -- a shared private `flux_set_suspend` builder with an
    inverted boolean, `MutationEffect::Modify`, `MutationRisk::Routine`
    (Flux CRDs hit no cluster-critical/privilege-sensitive hardcoded
    list, so risk tier only affects the displayed `PolicyReason`, not
    confirmation strength -- confirmed by reading `policy::evaluate`'s
    own `strong` rule before choosing this, not guessed). `flux_reconcile`
    is structurally identical to M8's own `restart` (an annotation bump,
    `reconcile.fluxcd.io/requestedAt`, Flux's own documented control
    surface). `FLUX_SUSPENDABLE_KINDS`/`FLUX_RECONCILABLE_KINDS` cover
    the 6 kinds confirmed via a real `kubectl explain <plural>.spec.
    suspend` against the live `sauron-m9` install (not assumed) --
    `Kustomization`/`HelmRelease`/`GitRepository`/`OCIRepository`/
    `HelmRepository`/`Bucket`. The three Image* kinds are excluded: their
    exact reconcile/suspend semantics were not independently confirmed.
  - **Zero new `kube::mutation` dispatch code**: `verify()`'s existing
    generic `Modify`/`leaf_path_and_value` path (including M8B.1's own
    boolean-`omitempty` fix) handles both `spec.suspend` and the
    annotation bump unmodified -- proved by 2 fake-HTTP tests
    (`mutation_flux_suspend_and_reconcile_commit_via_the_generic_modify_
    path`, `verify_flux_suspend_and_reconcile_confirm_via_the_generic_
    modify_path_zero_new_code`) and live.
  - **Open question 5 RESOLVED** (see its own entry above): the existing
    `Verification`/`MutationOutcome` vocabulary already suffices for
    "request accepted" and "fresh read matches"; the third fact
    (controller convergence) is presented by reusing M9.1's own
    `status_report` on a fresh read, shown as separate context, never
    folded into either enum, never awaited/polled for.
  - **`:flux_suspend`/`:flux_resume`/`:flux_reconcile` commands** added
    (palette-only), each following the exact `mutation_scope` ->
    builder -> `open_workflow_document` pattern every M8/M8B action
    uses -- no new app-layer machinery.
  - **Live evidence**: `live_flux_suspend_resume_and_reconcile_round_
    trip_on_a_real_kustomization` against the real `podinfo-kustomize`
    Kustomization on `sauron-m9` -- suspends (commits, verifies
    `spec.suspend: true`), resumes (commits, verifies `spec.suspend:
    false`, explicitly left unsuspended -- self-restoring, no drift),
    then reconciles (commits, verifies the exact annotation timestamp)
    and reads Flux's own fresh status afterward. **Real finding, kept as
    evidence rather than smoothed over**: the first run of this test
    caught the controller genuinely still reconciling immediately after
    the commit (`Reconciling=True`, `Ready=Unknown`, a real `STALE`
    observedGeneration lag) -- proving the three-fact separation is not
    just a paper design, an actual race a naive "commit succeeded =
    healthy" implementation would have hidden. Run twice clean via
    `scripts/test-cluster-m9.sh flux-test` (5 live tests total in the
    file now). Full M1-M8B regression (`accept-m3.py` through
    `accept-m8b.py`) reconfirmed green.
  - **Interactive evidence**: manual tmux smoke check confirmed
    `:flux_suspend`'s readonly denial and `:flux_reconcile`'s preview
    (`POLICY: RequireConfirmation`, `CONFIRMATION: required (press
    confirm once)` -- Standard, not Strong, exactly as designed) render
    correctly against the real cluster. Full `accept-m9.py` interactive
    coverage remains deferred to M9.7, per M8B's own precedent.
  Evidence: 5 unit tests (`mutation::workflow::tests`: unsupported-kind
  rejection for both suspend/resume; exact payload + distinct
  `source_action` for suspend vs. resume; every confirmed kind supported;
  unsupported-kind rejection and exact annotation payload for reconcile);
  2 app-level tests (readonly denies suspend/resume and sends zero
  requests; a Routine-risk Modify requires only Standard confirmation and
  commits on the first press -- proving the risk-tier choice was
  correct, not assumed); 1 grammar test; 2 fake-HTTP tests (commit sends
  the exact PATCH body for both actions; verify confirms via the
  existing generic path with zero new dispatch code); 1 live test (3
  real actions chained, self-restoring), run twice. Full locked suite:
  273 unit + 68 fake HTTP, fmt/check/clippy (`-D warnings`) clean.
  Next: M9.3 (Argo CD read-only views) -- but Open question 1a (Argo
  CD's own cluster placement: alongside Flux in `sauron-m9`, or a third
  dedicated cluster?) is exactly the kind of decision this milestone's
  own instructions require stopping for. Stopping here to ask before
  proceeding into M9.3.
- 2026-09-20: Open question 1a resolved by explicit user decision:
  Argo CD installed alongside Flux in `sauron-m9` (see Live-test
  strategy and Open question 1a's own entries above for the full
  rationale and live confirmation of zero interference). Real Argo CD
  `v3.5.3` installed via the pinned official manifest with `--server-
  side --force-conflicts` (required -- `ApplicationSet`'s CRD schema
  exceeds client-side apply's 262144-byte annotation limit, a real Argo
  CD installation quirk, not specific to this repo). All 7 components
  (`argocd-server`, `argocd-repo-server`, `argocd-application-
  controller`, `argocd-applicationset-controller`, `argocd-
  notifications-controller`, `argocd-redis`, `argocd-dex-server`)
  reached `Running`/`Ready` with zero restarts.
  **Major finding while investigating placement, resolving Open
  questions 2 and 3 ahead of M9.4**: `Application.operation` is a plain
  top-level CRD field. A real sync was triggered purely via `kubectl
  patch --type merge -p '{"operation":{"sync":{"revision":"master"}}}'`
  -- zero Argo API server calls -- and produced real
  `status.operationState` progress/result and real `Service`/
  `Deployment` objects in `sauron-m9`. `refresh` is the documented
  `argocd.argoproj.io/refresh` annotation (self-clearing, exactly like
  Flux's own reconcile annotation). `rollback` is the same
  `operation.sync` field with an explicit prior `revision` from
  `status.history` -- no adapter needed for any of the three M9.4
  actions.
- 2026-09-20: M9.3 (Argo CD read-only views) implemented and ACCEPTED.
  - **Listing/browsing needed zero new code**, exactly like M9.1's own
    Flux precedent: `Application`/`ApplicationSet`/`AppProject` are just
    catalogued resources once discovered; confirmed live -- the generic
    table already showed real `Sync Status`/`Health Status` printer
    columns for `applications.argoproj.io` with no new code.
  - **`integrations::argocd`** added: `KINDS` (`Application`,
    `ApplicationSet`, `AppProject`), `STATUS_KINDS` (`Application` only
    -- `ApplicationSet`'s own shape, a template generating many
    Applications, and `AppProject`'s own shape, an RBAC/source-
    restriction policy, are different enough to warrant their own
    dedicated renderers later; a bounded, documented limitation
    matching M9.1's own treatment of Flux's Image* kinds), and
    `status_report()` -- a pure renderer showing every source (single
    `spec.source` or multi-source `spec.sources`, both real schema
    shapes), destination, sync status + synced revision, health status,
    operation state (or its explicit absence, "no sync attempted yet"),
    every condition verbatim, automated sync policy presence, project,
    and a bounded (20-row) managed-resource summary from
    `status.resources`.
  - **Adjacent/Xray integration** (`graph::references.rs`): a new
    `(group, kind)` == `("argoproj.io", "Application")` dispatch branch
    extracts every `status.resources[]` entry as a managed-resource
    reference and `spec.project` as an `AppProject` reference.
    **Difference from Flux worth recording**: Argo CD's own
    `status.resources[]` entries *do* carry real, explicit `group`/
    `version` fields (unlike Flux's `sourceRef`), so the extracted
    `Target.api_version` here is real reported data, not the empty-
    string `StatusReference` exemption Flux needed -- confirmed against
    real live data before writing the extraction code, not assumed
    symmetric with Flux's own shape. `spec.project` still uses the
    empty-version exemption (`AppProject`'s own version is not reported
    on the referencing object), matching Flux's `sourceRef` precedent
    exactly for that one case.
  - **`:argocd` command** added (palette-only), same shape as `:flux`:
    selected object's own status if it is an `Application`, otherwise
    the M9.0 capability report.
  - **Live evidence**: real Argo CD `v3.5.3` on `sauron-m9`, two real
    `Application` fixtures (`tests/fixtures/m9-argocd.yaml`) --
    `guestbook` (synced against the real Argo CD example-apps repo,
    real `Service`/`Deployment` created in `sauron-m9`) and
    `guestbook-broken` (an intentionally nonexistent `targetRevision`,
    producing a real `ComparisonError` condition and `Sync: Unknown`).
    3 live tests in `tests/mutation_m9_argocd_live.rs`, run twice clean
    via `scripts/test-cluster-m9.sh argocd-test`: discovery reports
    every installed kind present; the synced Application's status,
    managed-`Service`/managed-`Deployment` references (resolved against
    the real discovery catalog, proving the real-group/version path
    genuinely works live, distinct from Flux's empty-version path), and
    `AppProject` reference all check out; the broken Application shows
    `Sync: Unknown` and the real, live-reported `ComparisonError`
    message verbatim. Flux's own 5 live tests reconfirmed passing
    unaffected by Argo CD's presence. Full M1-M8B regression
    reconfirmed green (one unrelated transient container-log-retrieval
    flake on `kind-sauron-test`, confirmed non-reproducing on retry and
    unrelated to any M9 change -- classified environment, not app,
    per this document's own bug-discipline categories).
  - **Interactive evidence**: manual tmux smoke check confirmed the
    generic table's own Sync/Health printer columns and `:argocd`'s
    status view render correctly against the real cluster. Full
    `accept-m9.py` interactive coverage remains deferred to M9.7.
  Evidence: 5 unit tests (`integrations::argocd::tests`: synced/healthy
  Application; broken Application with a comparison-error condition;
  multi-source Application lists every source; automated sync policy
  presence shown distinctly; `is_status_kind` recognizes `Application`
  only); 3 unit tests (`graph::references::tests`: managed resources
  carry real group/version never guessed; the project reference defaults
  to the Application's own namespace; an unrelated CRD sharing the
  `Application` Kind name in a different group never matches); 2
  app-level tests (capability report when absent; object status when an
  Application is selected -- both zero network tasks); 1 grammar test; 3
  live tests (above), run twice. No new fake-HTTP test, same rationale
  as M9.1 (zero new network code -- discovery and status reads reuse
  existing conventions). Full locked suite: 284 unit + 68 fake HTTP,
  fmt/check/clippy (`-D warnings`) clean.
  Next: M9.4 (Argo CD guarded actions), continuing automatically -- the
  provider-action-adapter question is already resolved (none needed, see
  above), so no new architectural blocker is expected, but M9.4's own
  implementation may still surface one (e.g. exact rollback-history
  bookkeeping) and will stop and ask if so.
- 2026-09-20: M9.4 (Argo CD guarded actions: sync, refresh, rollback)
  implemented and ACCEPTED. No new architectural blocker surfaced --
  confirmed the design test: zero new `commit`/`preflight` dispatch code
  (both already PATCH generically for any `Modify` payload regardless of
  shape); exactly one new named `verify()` branch was needed, for the
  same underlying reason `set_image` needed one in M8B.2.
  - **`mutation::workflow`**: `argocd_sync` (`operation.sync: {}`, no
    explicit revision -- Argo CD syncs to `spec.source`'s own
    `targetRevision`), `argocd_refresh` (the documented `argocd.
    argoproj.io/refresh: normal|hard` annotation, mirroring Flux's own
    reconcile annotation), `argocd_rollback` (the SAME `operation.sync`
    field with an explicit `revision`). All three `MutationEffect::
    Modify`, `MutationRisk::Routine` for sync/refresh, `Destructive` for
    rollback (transparency in the POLICY block only -- per this
    document's own resolved Open question 5 precedent, no risk tier
    changes confirmation strength here; all three stay Standard,
    consistent with M9.2's own Flux suspend/resume/reconcile). Distinct
    `source_action` per action despite sync/rollback sharing one field,
    matching M8B.6 Force delete's own "same shape, distinct
    source_action" precedent for journal/policy auditability.
  - **One new `kube::mutation::verify()` branch, `verify_argocd_
    operation`**: sync/rollback's payload leaf is an object (`{}` or
    `{"revision": ...}`), not a scalar, so the generic `leaf_path_and_
    value` path does not apply -- confirmed by checking the actual
    payload shape against `leaf_path_and_value`'s own single-scalar-leaf
    walk before writing new code, not assumed. `Verified` means only
    "the request is reflected in `status.operationState`" (any phase for
    plain sync; the exact requested `revision` echoed back for
    rollback) -- never "sync/rollback completed successfully". A
    mismatched/not-yet-present `operationState` is `Pending`, never a
    false `Verified` and never `ObservedDifferent` (an eventual-
    consistency window, not a confirmed different fact) -- proved by a
    dedicated fake-HTTP test asserting exactly this for a wrong-revision
    echo. Zero new `MutationOutcome`/`Verification`/`MutationEffect`
    variant, per this document's own resolved Open question 5.
  - **`:argocd_sync`/`:argocd_refresh [hard]`/`:argocd_rollback
    REVISION`** commands added (palette-only). `argocd_rollback`'s
    app-layer handler enforces workflow::argocd_rollback's own
    documented contract before ever building an intent: the given
    revision must already appear in the selected Application's own
    `status.history[].revision` -- an unknown revision is rejected with
    a clear error, zero intent built, zero request sent. This is
    deliberately enforced at the UI boundary (closest to free-form user
    input), not left to the workflow builder alone to trust its caller.
  - **Live evidence**: real Argo CD `v3.5.3` on `sauron-m9` -- sync
    (real `status.operationState` recorded), refresh (annotation
    committed and verified), and rollback (to the same revision
    `guestbook` was already synced to -- a mechanism proof, matching
    M8B.6 Force delete's own precedent for "proves the mechanism, not a
    materially-different real-world scenario", since a genuinely
    different-revision rollback would need fabricating a second real
    Git revision, out of proportion here) all committed and verified via
    the full gateway. `tests/mutation_m9_argocd_live.rs` now has 4 live
    tests, run twice clean via `scripts/test-cluster-m9.sh argocd-test`.
    Flux's own 5 live tests reconfirmed passing unaffected. Full M1-M8B
    regression reconfirmed green.
  - **Interactive evidence**: manual tmux smoke check confirmed
    `:argocd_refresh`'s preview (`POLICY: RequireConfirmation`,
    `CONFIRMATION: required (press confirm once)` -- Standard, exactly
    as designed) renders correctly against the real cluster. Full
    `accept-m9.py` interactive coverage remains deferred to M9.7.
  Evidence: 4 unit tests (`mutation::workflow::tests`: unsupported-kind
  rejection for all three actions; sync's exact empty-operation payload;
  refresh's normal-vs-hard annotation distinction; rollback's exact
  revision payload and distinct `source_action`); 2 app-level tests
  (readonly denies sync/refresh and sends zero requests; rollback
  rejects an unknown revision before building an intent, and accepts a
  known one); 1 grammar test; 2 fake-HTTP tests (commit sends the exact
  `operation` field for both sync and rollback; verify's dedicated
  comparison covers all four cases -- plain sync verified once any
  operationState appears, sync pending when none has appeared yet,
  rollback verified only on an exact revision echo, rollback pending on
  a mismatched echo, never a false positive); 4 live tests (above), run
  twice. Full locked suite: 292 unit + 70 fake HTTP, fmt/check/clippy
  (`-D warnings`) clean.
  **M9.0-M9.4 are now ACCEPTED. Flux support (read + guarded actions) is
  complete.** Next: M9.5 (Helm read-only inspection) -- a materially
  different domain (client-side release records in Secrets/ConfigMaps,
  not a reconciling controller) with its own load-bearing requirement
  (secret-safe redaction) that deserves its own careful design pass
  before writing code, per this document's own M9.5 contract.
- 2026-09-20: M9.5 (Helm read-only inspection) -- implementation hit a
  genuine architectural conflict immediately: `resources::Object::new()`
  unconditionally redacts every Secret's `data`/`stringData`, so the
  generic object pipeline can never carry a Helm release's real body (a
  release lives entirely inside one Secret's `data.release` field).
  Stopped before implementing past this per this document's own "stop
  at safety-defining decisions" governance, presented the finding with
  options and a recommendation, and received back a precise, fully
  itemized decision: implement a single, narrowly-scoped, explicit,
  bounded, TOCTOU-safe, sanitizing Helm-release reader as the one
  permitted exception to the generic "never read a Secret body"
  invariant -- documented in full, verbatim, under this section's own
  "M9.5 security design decision" write-up above. Implemented exactly
  as specified: `integrations::helm` (pure decode/sanitize logic --
  `decode_release` now `pub(crate)`, `HelmReleaseView`/`sanitize()` as
  the only public boundary crossing outward, notes permanently excluded)
  and the new `kube::helm::read_release` (the one function in the crate
  allowed to fetch a release Secret's body -- fresh bounded GET,
  independent UID+type re-verification against that fresh response,
  fail-closed on either mismatch). `app::open_helm_view` rebuilt as an
  async spawn reusing the existing `Payload::Document`/
  `Payload::DocumentError` plumbing (`start_adjacent`'s own convention)
  instead of the original broken synchronous design that tried to
  decode the already-redacted cached object. Evidence: 9 unit tests in
  `integrations::helm`, 1 in `kube::helm`, 3 app-level tests, 4 fake-HTTP
  tests (`tests/watch_transport.rs`, against a real HTTP endpoint -- the
  UID/type re-verification is a real security property, not just pure
  logic, so it is tested against a real fetch, not just
  decode/sanitize in isolation), and 4 live tests
  (`tests/mutation_m9_helm_live.rs`) against two independently-created
  real Helm releases on `sauron-m9` (`demo-release` via the real `helm`
  CLI as test-harness tooling; `podinfo-helm` via Flux's own
  `HelmRelease`), run via the new `scripts/test-cluster-m9.sh helm-test`
  case. Full locked suite green (305 unit + 74 fake-HTTP), fmt/clippy
  (`-D warnings`) clean, `accept-m8b.py` full interactive regression
  reconfirmed green on the untouched `kind-sauron-test` cluster, and
  Flux's (5) + Argo CD's (4) own live test suites reconfirmed green on
  `sauron-m9` -- zero drift from touching the shared `src/app/mod.rs`.
  **M9.0-M9.5 are now ACCEPTED.** Next: M9.6 (Helm guarded actions --
  rollback, uninstall) -- expected to itself raise a "must-stop"
  decision (Open question 4: whether a safe native Helm execution
  library/path exists, per this document's own explicit example of a
  question that must not be guessed at), so it is approached with the
  same care as the Helm read design above rather than assumed to follow
  the same shape.
- 2026-09-20: M9.6 (Helm guarded actions) -- ran the required Open
  question 4 investigation before writing any mutation code. Findings:
  no maintained Rust crate implements Helm's client-side action logic
  (`helm-api` is dead since 2019 and targets the pre-Helm-3 Tiller wire
  format; no equivalent of Go's `helm.sh/helm/v3/pkg/action` exists in
  the Rust ecosystem); Helm v3+ has no server/API component at all
  (Tiller was permanently removed in Helm v3, confirmed still true
  through Helm 4) so there is no RPC path to target either; and a
  from-scratch reimplementation of rollback/uninstall directly against
  the `kube` crate and the release Secret storage carries concrete,
  specific hazards to Helm's *own* bookkeeping -- revision-number
  sequencing, the deployed/superseded/failed status state machine, the
  three-way-merge-vs-Server-Side-Apply strategy a release was previously
  managed under (HIP-0023), hook lifecycle/ordering/delete-policies, and
  `helm.sh/resource-policy: keep` filtering are all load-bearing and
  easy to get subtly wrong, with the failure mode being that the real
  `helm` CLI later misbehaves or refuses to operate on a release this
  app touched. Presented these findings with three options (defer;
  reimplement a reduced "simple releases only" subset; reimplement the
  full lifecycle) and a recommendation to defer, per this document's
  own "stop before a safety/architecture decision" governance.
  **Decision: M9.6 is explicitly DEFERRED**, not attempted as a partial
  approximation -- none of shelling out to `helm`, directly manipulating
  Helm's storage records, or a reduced-scope native reimplementation
  (still writes Helm's own bookkeeping by hand, still carries the same
  hazards, would silently claim a form of Helm compatibility that was
  never demonstrated) were judged acceptable inside M9's scope. M9.5
  (read-only Helm inspection) stands as the final, complete Helm
  deliverable for M9. A future "Native Helm Engine" milestone is noted
  as backlog (not started, not scoped into M9) for anyone who later
  wants to deliberately take on Helm-compatible lifecycle
  reimplementation as its own substantial project, with the explicit
  design rule `LOOKS EQUIVALENT != BEHAVES EQUIVALENT` and a
  differential-testing acceptance strategy (real `helm` CLI as test
  oracle only, never as SAUR-ON's runtime implementation) recorded in
  M9.6's own write-up above for whenever that work is picked up.
  Full write-up, including the rejected options and their specific
  reasons, is in the M9.6 section above; the ledger and final acceptance
  conditions have been updated to reflect M9.6 as a deferred, not
  blocking, slice. Next: M9.7 (combined acceptance, full regression,
  soak) -- proceeding automatically, since the deferral itself is the
  accepted, evidence-backed outcome of this slice's own investigation,
  not an open blocker.
- 2026-09-20: M9.7 (combined acceptance, full M1-M8B regression, soak) --
  built `scripts/accept-m9.py`, mirroring `accept-m8b.py`'s own
  interactive tmux-driven style, against the dedicated `sauron-m9`
  cluster (`kind-sauron-test` left untouched for the M1-M8B regression
  scripts themselves). Coverage: readonly denies `flux_suspend`/
  `argocd_sync` with zero writes, then `:reload` picks up a per-context
  override without relaunching; Flux `suspend`/`resume`/`reconcile`
  round-trips against a real Kustomization, left un-suspended; Argo CD
  `sync`/`refresh`/`rollback` round-trips against a real Application
  (rollback to its own already-synced revision, a mechanism proof
  matching M8B.6 Force delete's own established precedent); `:helm`
  decodes and renders a real release, secret-safe; a never-reconciled,
  missing-source Kustomization (`podinfo-dependent`) renders the `-1`
  sentinel and an unresolvable `sourceRef` explicitly, never `Healthy`
  or empty (this milestone's own required partial/malformed-status
  coverage); Escape-before-commit sends zero writes; 32x9 with a Flux
  document open does not panic; M5 Explain/M7 mutation journal views
  are unaffected on `sauron-m9`; quit while the Helm view is open exits
  cleanly with exact `stty` restore; an M4 port-forward against a real
  podinfo Pod is held alive across three checkpoints spanning the Flux
  and Argo CD guarded-action sequences; and, run separately against
  `kind-sauron-test` (which has neither Flux nor Argo CD installed),
  `:flux`/`:argocd` both report `STATE: Unsupported`, never `Healthy` or
  empty -- proving CRD/API-absence handling on a cluster that never had
  M9's own CRDs. Replacement/TOCTOU rejection is deliberately not
  re-proven a third time interactively here: it is already proven at
  the fake-HTTP layer for every M9 kind (`tests/watch_transport.rs`'s
  `mutation_uid_mismatch_before_commit_sends_zero_mutation_request` and
  the Flux/Argo CD/Helm-specific verify/read tests) and live for Helm
  specifically (`live_stale_uid_against_a_real_secret_fails_closed`) --
  the underlying mechanism (`kube::mutation::commit`'s UID
  revalidation, `kube::helm::read_release`'s own re-verification) is
  identical across every kind M9 touches.

  Full M1-M8B regression (`accept-m3.py` through `accept-m8b.py`,
  unmodified) re-run and green. Run twice: the first run passed clean
  end to end; the second run hit one isolated failure in `accept-m3.py`
  (`expect('ambiguous')` timed out resolving the `po` alias against a
  cluster catalog that also has fixture CRDs registered) -- reproduced
  as a standalone clean pass on an immediate retry with zero code
  changes, confirming a pre-existing, unmodified M3 harness/environment
  timing flake unrelated to any M9 change (matching this document's own
  established precedent for classifying a similar M4 flake during
  M9.4). A subsequent full second run then passed clean end to end,
  satisfying the "run twice clean" requirement.

  New `scripts/soak-m9.py`: a bounded soak scoped down proportionally
  from `soak-m8b.py`'s own 75-minute/488-cycle run, since M8B.7's soak
  already proved the shared mutation gateway (preflight/commit/verify/
  journal/policy) stable under sustained load -- this soak's own job is
  narrower, proving M9's specific additions (Flux/Argo CD guarded
  actions, Helm's dedicated Secret-body reader) don't regress that
  stability, not re-deriving it a second time. Rotates through Flux
  status checks and a self-restoring suspend/resume round-trip every
  cycle (reconcile throttled to every 10th cycle -- a repeated
  reconcile-annotation bump every cycle would be indistinguishable from
  one write, not a meaningful signal, mirroring `soak-m7.py`'s own
  established reasoning for its own throttled mutation), Argo CD status
  checks and a self-restoring sync/refresh round-trip every cycle
  (rollback likewise throttled to every 10th cycle), and Helm read-only
  inspection every cycle (no throttling concern, since it is read-only),
  with an M4 forward against a real podinfo Pod held and checked every
  cycle. Ran twice (once as part of each full `accept-m9.py` run above):
  240s/242s, 53-54 cycles each, **zero reconnects, zero transient
  errors**, RSS/fd/thread counts flat both times (32720-32752 KiB RSS,
  14 fds, 4 threads throughout), metrics-requests-started counter
  unchanged (`1->1`, as expected for a soak with no metrics-emitting
  resource kind selected) -- "observed stability only, no leak-freedom
  claims", the same honesty `soak-m7.py`/`soak-m8b.py` already
  established.

  No new Rust source was needed for M9.7 -- it is entirely test-harness
  and documentation work, and it found zero SAURON application bugs
  (the one failure encountered, in `accept-m3.py`, was confirmed
  harness/environment timing, not app or M9 logic). Full locked suite
  unchanged from M9.5 (305 unit + 74 fake HTTP, plus 9 Flux/Argo CD live
  tests and 4 Helm live tests), `cargo fmt --check` and
  `cargo clippy --all-targets -- -D warnings` clean (no Rust files
  touched this slice). `HANDBOOK.md`, `docs/RUNBOOK.md`, and
  `README.md` reconciled with M9's full actual scope and status
  (M8B's own completion, previously only recorded in
  `docs/M8B_ACCEPTANCE.md` and its own tag, was also backfilled into
  these three narrative docs while reconciling M9, since they had never
  been updated for it).

  **M9 is now ACCEPTED: M9.0-M9.5 and M9.7. M9.6 (Helm rollback/
  uninstall) is explicitly DEFERRED**, per its own evidence-backed
  investigation and decision recorded above -- not a blocker to this
  acceptance. Worktree confirmed clean; local annotated tag
  `m9-accepted` created, not pushed, per this project's own standing
  "never push without explicit authorization" rule.

## Final acceptance conditions (mirroring M8B's own structure)

M9 is not ACCEPTED until, for every slice M9.0-M9.5 and M9.7 (M9.6 is
explicitly DEFERRED, an accepted outcome of its own required
investigation, not a blocker — see M9.6's own write-up):

- Every supported action goes through the M7 gateway
  (`preflight`/`commit`/`verify`) — no direct UI mutation bypass, no CLI
  shell-out mutation path, exactly as M8/M8B required.
- Readonly means zero writes for every M9 action, verified per-operation.
- TOCTOU/UID replacement rejection is proven per operation that has an
  existing target to replace.
- Conflicts are explicit; no automatic ambiguous retry, anywhere.
- `OutcomeUnknown` is preserved and never silently upgraded to success or
  downgraded to failure.
- The journal records every real action attempt/result, correlated by
  integration via `source_action` naming (M9.0's convention) and the
  object's own GVK — never a second journal.
- Post-commit verification is truthful and distinct from commit outcome
  for every action, and — new for M9 — distinct from controller
  convergence/workload health where those are separate facts (M9.2/M9.4).
- Health/readiness is never conflated with action acknowledgement; Flux/
  Argo conditions render as evidence, not a second health engine.
- Timeline remains observation-derived only; Adjacent/Xray remain
  evidence-derived only — Flux/Argo/Helm objects join those views via
  explicit references/provenance only.
- CRD/API absence renders `Unsupported`, never `Healthy`/empty; partial
  discovery renders `Partial`, never silently degraded either direction.
- Helm values/manifest views are secret-safe by construction (M9.5's own
  contract) — verified by a dedicated redaction test suite, not just
  informal review.
- 32x9 works. Terminal restoration works. Full M1-M8B regression passes.
- The soak (M9.7) is acceptably stable, with the same "observed stability
  only, no leak-freedom claims" honesty M8.6/M8B.7 already established.
- Production sees zero writes and zero mutating dry-runs across every M9
  action.
- Docs (this file, `HANDBOOK.md`, `docs/RUNBOOK.md`, `README.md`) are
  reconciled with actual evidence.
- Worktree is clean.

Only then: create a local annotated tag `m9-accepted` — never pushed
without explicit authorization, exactly like every prior milestone tag.
