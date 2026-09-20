# M9 — GitOps/Package Manager Integration (Flux, Argo CD, Helm): planning ledger (IN PROGRESS)

Status: **IN PROGRESS** — M9.0 ACCEPTED (2026-09-20), M9.1-M9.7 PLANNED.
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
| M9.3 | Argo CD read-only views (Application, opportunistic ApplicationSet) | PLANNED |
| M9.4 | Argo CD guarded actions (sync, refresh, rollback if a safe API path exists) | PLANNED |
| M9.5 | Helm read-only inspection (releases, history, secret-safe values/manifest view) | PLANNED |
| M9.6 | Helm guarded actions (rollback, uninstall; upgrade only if a safe bounded input model is found) | PLANNED |
| M9.7 | Combined acceptance, full M1-M8B regression, soak | PLANNED |

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

### M9.6 — Helm guarded actions

Candidates: `rollback`, `uninstall`; `upgrade` only if a safe, bounded
input model is found (strong default expectation: **not** in M9's first
pass — arbitrary upgrade needs a values/chart input surface this app has
no safe precedent for; start with rollback + uninstall and revisit
upgrade only if a genuinely bounded shape emerges, e.g. "roll forward to
the next already-recorded revision" rather than free-form input).

Execution must go through a real Helm lifecycle path — a Rust Helm
library/SDK abstraction, or the equivalent documented Helm API contract
— never direct manipulation of the `Secret`/`ConfigMap` storage records
as an implementation shortcut (that would bypass Helm's own release
lifecycle semantics — e.g. its own locking/history bookkeeping — and is
explicitly disallowed by the kickoff prompt). If no safe native execution
path is practical within M9's scope, this document is updated to record
mutation-side Helm actions as explicitly deferred, keeping read-only
support (M9.5) as the shipped deliverable instead of faking parity.

Rollback: exact release, exact target revision, preview current revision
-> target revision, strong confirmation, journal, verification that the
release's revision/status actually changed as expected — never a claim
that workloads are healthy solely because Helm itself reports success.

Uninstall: destructive/high-risk, strongest confirmation tier available
(mirroring M8B.6 Force delete's own precedent for "highest risk in the
milestone"), preview of the targeted release plus a bounded resource-
ownership summary, no hidden force semantics, journal, verification of
release absence/uninstalled state — never a claim that all owned
Kubernetes resources are gone unless actually observed gone.

### M9.7 — Combined acceptance, full M1-M8B regression, soak

Mirrors M8B.7's own structure. Required coverage: full M1-M8B regression
(every existing `accept-*.py` script, unmodified); Flux read + guarded
actions; Argo CD read + guarded actions; Helm read + guarded actions;
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

## Final acceptance conditions (mirroring M8B's own structure)

M9 is not ACCEPTED until, for every slice M9.0-M9.7:

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
