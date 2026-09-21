# Native Helm Engine — Feasibility Research

Status: **RESEARCH ONLY.** No implementation started. No file other than this
one was created or modified for this investigation. This document does not
authorize writing code; it exists to let that decision be made with evidence.

Scope: a follow-on investigation to `docs/M9_ACCEPTANCE.md`'s M9.6 (Helm
`rollback`/`uninstall` guarded actions, DEFERRED 2026-09-20). It asks the
question M9.6 explicitly left open: could SAUR-ON someday implement Helm's
own release-lifecycle behavior natively in Rust, well enough that the real
`helm` CLI can keep operating on a release SAUR-ON touched?

Sourcing discipline used throughout: every behavioral claim is tagged
**[SOURCE]** (upstream Go source file/function, fetched from
`github.com/helm/helm` at a specific tag), **[DOCS]** (official
`helm.sh/docs`), or **[EXPERIMENT]** (a reproducible command run in this
session against the disposable `helm-research-scratch` namespace on the
`kind-sauron-m9` cluster, cleaned up afterward — never against `demo-release`,
`podinfo-helm`, `sauron-m9`, or `argocd`). Nothing here is asserted from
memory alone; §20 lists what could not be verified and why.

Helm versions inspected: **v3.22.0** (current stable v3 line, primary
reference — this is what the overwhelming majority of real-world clusters
run) and **v4.3.0** (current v4 line; the `helm` binary actually present on
this host reports `v4.1+unreleased`, confirming v4 is now the locally
available CLI and a live behavioral reference, not just source). Where v3
and v4 diverge, both are called out explicitly — the divergence itself is
one of this document's central findings (§8).

---

## 1. Executive summary

Helm is not a server. A "release" is entirely client-side state: a Go struct
serialized to JSON, gzipped, base64-encoded twice, and stored as the `data`
field of a `Secret` (or `ConfigMap`) named
`sh.helm.release.v1.<release>.v<revision>`
**[SOURCE: `pkg/storage/driver/secrets.go:newSecretsObject`, `pkg/storage/storage.go:makeKey`]**.
Every "Helm" operation is: read some Secrets, mutate a small Go object graph
in memory, apply/delete plain Kubernetes resources with `client-go`, and
write more Secrets. There is no RPC server (Tiller was removed in Helm v3,
2019, confirmed by Helm's own "Changes since Helm 2" doc) **[DOCS]**, and no
maintained Rust crate reimplements this logic — this repo's own prior M9.6
investigation already established that, and this research did not find a
newer alternative.

That "it's just Secrets and API calls" framing is exactly the trap. The
actual behavior is not simple:

- **Rollback is itself a targeted `Update`, not a restore.** It builds the
  *previous* revision's rendered manifest, diffs it against the *current*
  revision's rendered manifest via a **three-way patch** (old desired /
  new desired / live cluster state) in Helm v3, or via **Server-Side
  Apply** by default in Helm v4 — two genuinely different wire protocols
  with different conflict semantics
  **[SOURCE: `pkg/kube/client.go:createPatch` (v3.22.0) vs `pkg/kube/client.go:808 Update` defaulting `serverSideApply: true` (v4.3.0)]**,
  independently reproduced live in this session (§9, §20).
- **Uninstall reorders and filters** the release's own recorded manifest
  (reverse of install-kind order, `helm.sh/resource-policy: keep` skipped
  per-document) before issuing per-resource deletes with an explicit
  propagation policy **[SOURCE: `pkg/action/uninstall.go:deleteRelease`,
  `pkg/releaseutil/kind_sorter.go`]**.
- **Hooks are a full sub-lifecycle** (weight-ordered creation, watch-until-
  ready, delete-policy-driven cleanup, log capture on failure) that runs
  *inside* both rollback and uninstall, not beside them
  **[SOURCE: `pkg/action/hooks.go:execHook`]**.
- **There is no locking.** Helm's own source comment says outright:
  *"If executed concurrently, Helm's database gets corrupted and multiple
  releases are DEPLOYED"* **[SOURCE: `pkg/storage/storage.go:Deployed`]**.
  Compatibility here means matching an intentionally-unlocked system, not
  improving on it silently.

**Verdict (expanded in §21): FEASIBLE, but only as a dedicated, source-
verified, differentially-tested engine — never as a "simple releases only"
shortcut, and never as a reimplementation of hooks/SSA/three-way-merge from
guesswork.** The prior M9.6 rejection of a partial/simple-case
reimplementation was correct and remains correct; this document's job is to
show what a *complete-enough* one would require, and to be honest that some
of it (Server-Side Apply field-manager compatibility across a v3→v4-managed
release, hook log capture, CRD three-way-merge edge cases) is genuinely hard
and must be proven, not assumed, before shipping.

## 2. Feasibility verdict

**Medium-high**, conditioned on scope discipline:

- Rollback and uninstall against releases with **no hooks**, **no
  `resource-policy: keep`**, **client-side-apply (v3-style) history**, and
  **no CRDs/cluster-scoped resources** are *tractable* to build and
  differentially prove compatible within a few milestones.
- Extending that to hooks, resource-policy, CRDs, and mixed CSA/SSA
  history is *substantially* more work but is bounded and traceable —
  every behavior above has a concrete upstream source location, not an
  unknown.
- Extending it to full Server-Side-Apply-manager compatibility with a
  release a real Helm v4 CLI will *later* operate on is the one area
  where "compatible" cannot be fully proven without also tracking Helm's
  own field-manager string and upgrade-path behavior (`upgradeClientSideFieldManager`,
  §8) — this is real risk, not merely unfamiliarity.

This is why §17 recommends a narrow, honestly-labeled *first* scope (v3-style
CSA releases, no hooks, no keep-policy) with everything else explicit,
tested-later scope — never silently unsupported.

## 3. Helm upstream call graph

### Rollback (`pkg/action/rollback.go`, v3.22.0)

```
Rollback.Run(name)
├─ cfg.KubeClient.IsReachable()                         [fail fast if cluster unreachable]
├─ cfg.Releases.MaxHistory = r.MaxHistory
├─ prepareRollback(name)
│   ├─ chartutil.ValidateReleaseName(name)
│   ├─ cfg.Releases.Last(name)                           -> currentRelease
│   ├─ resolve previousVersion (explicit r.Version, or currentRelease.Version-1)
│   ├─ cfg.Releases.History(name)                        -> historyReleases
│   ├─ linear scan: does previousVersion exist in history? else error
│   ├─ cfg.Releases.Get(name, previousVersion)            -> previousRelease
│   └─ build targetRelease{
│         Name, Namespace: currentRelease.Namespace,
│         Chart/Config/Manifest/Hooks/Labels: <copied from previousRelease>,
│         Info.FirstDeployed: <copied from currentRelease>,
│         Info.LastDeployed: now,
│         Info.Status: StatusPendingRollback,
│         Version: currentRelease.Version + 1,   <-- NEW monotonic revision, not a revert
│       }
├─ [if !DryRun] cfg.Releases.Create(targetRelease)        [writes v(N+1) Secret as pending-rollback]
├─ performRollback(currentRelease, targetRelease)
│   ├─ KubeClient.Build(currentRelease.Manifest)  -> current (typed Info list)
│   ├─ KubeClient.Build(targetRelease.Manifest)   -> target
│   ├─ [if !DisableHooks] execHook(targetRelease, HookPreRollback, Timeout)
│   ├─ target.Visit(setMetadataVisitor(name, ns, force=true))   [stamps Helm release/name annotations]
│   ├─ KubeClient.Update(current, target, Force)   -> per-resource create/patch/delete (§8)
│   │    on error: currentRelease.Status=Superseded, targetRelease.Status=Failed,
│   │              recordRelease(current); recordRelease(target);
│   │              [if CleanupOnFail] Delete(results.Created)
│   │              -> return error (release left in Failed/Superseded pair, see §10)
│   ├─ [if Recreate] recreate(cfg, results.Updated)   [best-effort pod restart, logged only on error]
│   ├─ [if Wait] KubeClient.Wait/WaitWithJobs(target, Timeout)
│   │    on error: targetRelease.Status=Failed; recordRelease both; return error
│   ├─ [if !DisableHooks] execHook(targetRelease, HookPostRollback, Timeout)
│   ├─ Releases.DeployedAll(currentRelease.Name) -> deployed[]
│   ├─ for each deployed: rel.Status = Superseded; recordRelease(rel)   [supersede ALL prior deployed, not just current — issue #2941]
│   └─ targetRelease.Status = Deployed
└─ [if !DryRun] cfg.Releases.Update(targetRelease)        [final persisted state]
```

`recordRelease` = `cfg.Releases.Update(rel)`, logged-only on failure (never
retried, never surfaced as a hard error to the caller) — this is itself a
notable failure-mode fact, see §10.

### Uninstall (`pkg/action/uninstall.go`, v3.22.0)

```
Uninstall.Run(name)
├─ KubeClient.IsReachable()
├─ [if DryRun] releaseContent(name, 0) -> return without mutating anything
├─ chartutil.ValidateReleaseName(name)
├─ cfg.Releases.History(name) -> rels[]                  [errors.Wrapf unless IgnoreNotFound]
├─ releaseutil.SortByRevision(rels); rel = rels[last]      [operate on the LATEST revision only]
├─ [if rel.Status == Uninstalled]
│     [if !KeepHistory] purgeReleases(rels...) -> return   [idempotent re-uninstall of an already-uninstalled release just purges]
│     [else] -> hard error "already deleted"
├─ rel.Status = Uninstalling; rel.Info.Deleted = now;
│  rel.Info.Description = "Deletion in progress (or silently failed)"   [note: this string is written BEFORE success is known]
├─ [if !DisableHooks] execHook(rel, HookPreDelete, Timeout)
├─ cfg.Releases.Update(rel)     [persist Uninstalling state — logged-only on failure, not retried]
├─ deleteRelease(rel)
│   ├─ releaseutil.SplitManifests(rel.Manifest)
│   ├─ releaseutil.SortManifests(..., UninstallOrder)      [reverse-ish of install kind order, §7]
│   ├─ filterManifestsToKeep(files) -> (filesToKeep, filesToDelete)   [helm.sh/resource-policy: keep, read from the STORED manifest, not the live object]
│   ├─ KubeClient.Build(filesToDelete)
│   └─ KubeClient.DeleteWithPropagationPolicy(resources, cascade)     [per §11]
├─ [if Wait] KubeClient.WaitForDelete(deletedResources, Timeout)      [errors collected, not fatal]
├─ [if !DisableHooks] execHook(rel, HookPostDelete, Timeout)          [errors collected, not fatal]
├─ rel.Status = Uninstalled; rel.Description = <custom or default>
├─ [if !KeepHistory] purgeReleases(rels...) -> return (with any collected errs joined)
├─ [else] cfg.Releases.Update(rel)          [persist Uninstalled state]
├─ Releases.DeployedAll(name) -> supersede any still-Deployed prior revision (issue #12556/#2941 pattern, same as rollback)
└─ return (errs joined if any occurred; partial success still reports the joined errors)
```

### Shared substrate both actions depend on

```
Configuration (cfg)
├─ Releases: *storage.Storage        (wraps a driver.Driver — Secrets/ConfigMap/SQL/Memory)
├─ KubeClient: kube.Interface        (client-go wrapper: Build/Create/Update/Delete/Wait/WatchUntilReady)
├─ execHook / deleteHookByPolicy / outputLogsByPolicy   (pkg/action/hooks.go)
└─ recordRelease = Releases.Update   (best-effort persistence, never itself retried)
```

## 4. Rollback behavior map

See §3's call graph for the exact sequence. Key facts a native engine MUST
reproduce, each already source-cited above:

1. Target revision is validated against `History`, not merely
   "does a Secret with this name exist" — a malformed/missing intermediate
   revision fails `prepareRollback` before any cluster write happens.
2. Rollback **creates a new revision** (`currentRelease.Version + 1`); it
   never rewrites/reactivates the old revision's own Secret. `helm rollback`
   to "v1" from "v3" produces a **v4** record whose content mirrors v1.
3. The new revision is persisted **twice**: once as `PendingRollback`
   (before any cluster mutation), once as `Deployed`/`Failed` (after). A
   process crash between these two writes leaves a `pending-rollback`
   release permanently in that state (§10).
4. `Update(current, target, force)` is the actual apply step — not a
   generic PATCH. It diffs the resource set (create/patch/delete based on
   set membership) and, per resource, chooses replace-vs-patch and
   patch-type based on `force` and object structuredness (§8).
5. ALL previously-`Deployed` revisions for the release name are superseded
   after a successful rollback, not just the one being replaced (defensive
   code against `helm`'s own lack of locking, issue #2941).
6. Hooks run twice (`pre-rollback`, `post-rollback`) around the apply step,
   using the **target** (previous) release's own recorded hook manifests.

## 5. Uninstall behavior map

See §3. Key facts:

1. Uninstall only ever acts on the **latest** revision by version number
   (`SortByRevision` then take the last) — an uninstall request is not
   parameterized by revision.
2. Re-uninstalling an already-`Uninstalled` release with `KeepHistory=false`
   is defined as **purge** (delete all history Secrets), not an error — this
   is the one place `Uninstall.Run` treats "already done" as success.
3. `rel.Info.Description = "Deletion in progress (or silently failed)"` is
   written *before* deletion is attempted — Helm is explicitly modeling that
   a crash here leaves an ambiguous, self-documenting "maybe silently
   failed" state (§10) rather than pretending certainty.
4. The manifest used for deletion targeting is the **release's own stored
   manifest**, not a live-cluster relationship query — matching resources
   are identified structurally (same apiVersion/kind/namespace/name as
   recorded at install/upgrade time), never by label selector or owner
   reference at delete time.
5. `resource-policy: keep` is evaluated from that **stored** manifest's
   annotations, not from the live object's current annotations — a resource
   whose live annotation was added later (out of band) is NOT protected;
   one whose recorded annotation was later removed live but still says
   `keep` in the stored manifest IS still protected. This is a subtle,
   source-verified divergence a "read the live object's annotations"
   reimplementation would get wrong.
6. `Wait`/`WaitForDelete` failures and post-delete-hook failures are
   collected into `errs` but do **not** prevent `rel.Info.Status =
   StatusUninstalled` from being set — "uninstalled" in Helm's bookkeeping
   means "the delete calls were issued and the release record updated",
   not "every resource is confirmed gone" (§10).

## 6. Shared lifecycle primitives

Both actions share, and a native engine needs, exactly these primitives —
none is unique to one action:

| Primitive | Upstream source | Native engine equivalent needed |
|---|---|---|
| Release storage CRUD (get/list/query/create/update/delete by key) | `pkg/storage/storage.go`, `pkg/storage/driver/secrets.go` | Yes — read+write, not read-only like M9.5 |
| Release history query/sort/supersede | `pkg/storage/storage.go: History/DeployedAll/Deployed`, `pkg/releaseutil` sort helpers | Yes |
| Manifest split/sort/filter by kind and resource-policy | `pkg/releaseutil/manifest.go`, `kind_sorter.go`, `pkg/action/resource_policy.go` | Yes |
| Typed-vs-unstructured resource "Build" (parse manifest -> apply-ready objects w/ REST mapping) | `pkg/kube/client.go: Build` (client-go `resource.Builder`) | Yes — via `kube` crate discovery + dynamic client |
| Apply/patch/replace/SSA dispatch | `pkg/kube/client.go: updateResource/createPatch` (v3) vs SSA-default `Update` (v4) | Yes — the highest-risk primitive, §8 |
| Delete w/ propagation policy | `pkg/kube/client.go: rdelete/DeleteWithPropagationPolicy` | Yes |
| Hook execution state machine | `pkg/action/hooks.go: execHook` | Only if hooks are in scope (§9 recommends deferring, not skipping forever) |
| Wait/watch-until-ready | `pkg/kube/wait.go`, `ready.go`, `WatchUntilReady` | Only where `Wait`/hooks require it |
| Status/pending-state transition rules | `pkg/release/status.go` | Yes |

This *is* a sensible internal engine boundary — a `storage` layer, a
`manifest`/`resource-policy` layer, an `apply` layer, and a `lifecycle`
(action) layer that composes them, matching the brief's proposed shape
(§15) reasonably well.

## 7. Release storage / history semantics

**[SOURCE + EXPERIMENT]** — confirmed against real objects in
`helm-research-scratch` (kind-sauron-m9), cleaned up after capture.

- **Naming**: `sh.helm.release.v1.<release>.v<revision>`
  (`pkg/storage/storage.go:makeKey`, `HelmStorageType = "sh.helm.release.v1"`).
- **Type**: `helm.sh/release.v1` (matches M9.5's own `RELEASE_SECRET_TYPE`
  constant already in this codebase — no drift there).
- **Labels Helm sets** (`newSecretsObject`, confirmed live):
  `name`, `owner: helm`, `status`, `version`, plus `createdAt` (on Create,
  Unix seconds) or `modifiedAt` (on Update, Unix seconds) — never both on
  the same object.
  Live capture after install/upgrade/rollback:
  ```
  sh.helm.release.v1.rsc.v1  {modifiedAt:1789980633 name:rsc owner:helm status:superseded version:1}
  sh.helm.release.v1.rsc.v2  {modifiedAt:1789980633 name:rsc owner:helm status:superseded version:2}
  sh.helm.release.v1.rsc.v3  {modifiedAt:1789980794 name:rsc owner:helm status:deployed   version:3}
  ```
  (v3 here is the *rollback-to-v1* revision — note it is a new v3 record,
  not a resurrected v1, exactly as §4 predicts.)
- **Revision increment**: strictly `currentRelease.Version + 1` for every
  lifecycle transition (install=1, then +1 per upgrade/rollback). There is
  no "revision reuse" path anywhere in `rollback.go`/`uninstall.go`.
- **Deployed/Superseded**: at most one revision is `Deployed` under correct
  sequential operation; every prior `Deployed` revision is flipped to
  `Superseded` at the end of a successful rollback/upgrade
  (`DeployedAll` + loop, both `rollback.go` and, per its own comment
  referencing issue #12556, `uninstall.go`).
- **Pending-* states**: `pending-install`/`pending-upgrade`/`pending-rollback`
  are written *before* the cluster-affecting work and only resolved to a
  terminal state (`deployed`/`failed`/`superseded`) after — meaning a
  process death mid-operation leaves a **permanently pending** release
  (`Status.IsPending()`, `pkg/release/status.go`). Helm's own `install`/
  `upgrade` actions special-case a pre-existing pending release (not
  inspected in this pass, out of scope for rollback/uninstall but
  important context: Helm itself has known "stuck pending" behavior that
  users work around with `helm rollback`/manual Secret edits).
- **Uninstall + KeepHistory**: confirmed live — with `--keep-history`, the
  final revision's Secret is kept with `status: uninstalled`; `helm status`
  and `helm history` continue to work against it afterward. Without
  `--keep-history`, `purgeReleases` deletes every revision's Secret for
  that name — a subsequent `helm status`/`history` reports "release: not
  found", and a subsequent `helm install` of the same name starts fresh at
  revision 1.
- **MaxHistory / removeLeastRecent**: pruning happens on `Create` (i.e. on
  install/upgrade/rollback, never on uninstall), keeps the currently
  deployed revision unconditionally, and is not atomic with the create it's
  making room for — a crash between prune and create can leave history
  short by one slot without the intended new record existing yet.
- **Atomicity / locking**: **none**. `pkg/storage/storage.go:Deployed`'s own
  comment: *"If executed concurrently, Helm's database gets corrupted and
  multiple releases are DEPLOYED. Take the latest."* Concurrent `helm`
  invocations against the same release name are a documented, accepted
  hazard in upstream Helm itself, not something this project needs to
  solve better than upstream — only avoid making *worse* (§13).

## 8. Resource reconciliation semantics — the highest-risk area

This is the section the brief explicitly demands not be hand-waved. It is
also empirically the most consequential finding of this research.

### Helm v3 (v3.22.0): three-way strategic-merge patch, client-side

`createPatch` (`pkg/kube/client.go`) computes, per resource:
- `oldData` = JSON of the *previous* release's rendered object for this
  resource (the "original" 3-way input),
- `newData` = JSON of the *new* release's rendered object,
- `currentData` = a **fresh GET** of the live object at apply time,
- then either `strategicpatch.CreateThreeWayMergePatch` (typed/registered
  kinds) or, for unstructured/CRD objects, either a generic
  `jsonpatch.CreateMergePatch` (two-way, default) or
  `jsonmergepatch.CreateThreeWayJSONMergePatch` (three-way, only if the
  caller explicitly opts into `UpdateThreeWayMerge`/`--reset-then-reuse-values`-style behavior — `rollback.go` calls plain `Update`, i.e. **two-way** for CRDs).
- The field manager sent with every PATCH/CREATE/DELETE is a fixed string
  from `getManagedFieldsManager()` — the `ManagedFieldsManager` package var
  if explicitly set, else `filepath.Base(os.Args[0])` (in practice
  literally `"helm"` for the real CLI).
- **`--force`** bypasses all patch logic and does `helper.Replace` (a full
  PUT), which is a different failure/conflict surface entirely (replaces
  resourceVersion-guarded, no merge semantics at all).

Consequence for "fields changed manually outside Helm": under v3's
three-way merge, a field present in `oldData` but absent from `newData` is
explicitly removed even if a human changed it live (that's exactly what
three-way merge is for — it distinguishes "chart used to set this, no
longer does" from "chart never touched this"); a field a human added that
was never in either `oldData` or `newData` is left alone (three-way merge
only acts on paths the *patch itself* knows about). This is **not** SSA's
field-manager-based conflict tracking — it is closer to "diff old-desired
vs new-desired, apply that diff on top of live", which can silently
overwrite an operator's live edit if that edit happens to touch a path
Helm's own diff also touches, with no explicit conflict signal at all —
confirmed by reading `createPatch` itself; there is no ownership metadata
consulted anywhere in this code path.

### Helm v4 (v4.3.0): Server-Side Apply is the *default*

`pkg/kube/client.go`'s `Update` in v4.3.0:
```go
// The default is to use server-side apply, equivalent to: `ClientUpdateOptionServerSideApply(true)`
func (c *Client) Update(originals, targets ResourceList, options ...ClientUpdateOption) (*Result, error) {
	updateOptions := clientUpdateOptions{
		serverSideApply: true, // Default to server-side apply
		...
```
with an explicit `ClientUpdateOptionUpgradeClientSideFieldManager` option
whose own doc comment says outright: *"This is required when upgrading a
chart from client-side to server-side apply, otherwise the client-side
field management remains, conflicting with server-side applied updates."*
— i.e. **Helm's own authors treat CSA→SSA migration as a real, non-trivial
compatibility hazard requiring an explicit opt-in upgrade step**, not an
automatic transparent change. **[SOURCE]**

**[EXPERIMENT]**, confirmed live with the `v4.1+unreleased` binary on this
host against the disposable `rsc` release in `helm-research-scratch`: after
`helm install`, the live `ConfigMap`'s `managedFields` showed
`"manager": "helm", "operation": "Apply"` — i.e. this v4 binary is already
using SSA (`operation: Apply`, not the `Update` operation a v3 patch/replace
would produce) for ordinary install/rollback, matching the v4.3.0 source's
documented default.

**Compatibility conclusion**: a native engine choosing "always three-way
patch" or "always SSA" is choosing which *class* of releases it can safely
touch. A release whose history contains only CSA-managed revisions (v3-style)
can be safely rolled back with a v3-equivalent three-way-merge implementation.
A release with SSA-managed live objects (v4-style, or migrated via
`upgradeClientSideFieldManager`) requires implementing actual SSA semantics
(field manager string, conflict/force-conflicts handling) to avoid either
(a) a spurious 409 conflict against the recorded Helm field manager, or
(b) silently stripping that field manager's ownership by patching instead of
applying. **There is no single implementation that is correct for both
without detecting which regime a given release is in** — and Helm itself
does not record "which apply strategy was used" anywhere in the release
object; it is inferred only from the live object's `managedFields`, which a
native engine would have to inspect at operation time, not assume from the
stored release record alone. This is this document's single biggest
"must-prove-before-first-write" item (§21).

### HIP-0023

HIP-0023 is Helm's own proposal that formalized the CSA→SSA migration
strategy summarized above (the `UpgradeManagedFields`/
`k8s.io/client-go/util/csaupgrade` reference embedded directly in the v4
source comment is the concrete implementation of that HIP). No separate
fetch of the HIP text was performed in this pass (§20) — the *behavioral*
consequence (explicit opt-in upgrade step, dual-mode `Update`) was
confirmed directly from the shipped v4.3.0 source, which is stronger
evidence than the proposal document itself.

## 9. Hook semantics

`pkg/action/hooks.go: execHook`, traced fully in §3's shared substrate and
read in full for this research:

- Hooks matching the requested event (`pre-rollback`, `post-rollback`,
  `pre-delete`, `post-delete`, etc.) are collected from `rl.Hooks` and
  **sorted stably by `Weight`, then by `Name`** (`hookByWeight`).
- **Default delete policy is `before-hook-creation`** if a hook specifies
  none (`hooks.go` sets this explicitly, not left as "no cleanup") — i.e.
  a hook Job with the same identity from a prior run is deleted before the
  new one is created, by default, even if the chart author never wrote a
  delete-policy annotation.
- **`CustomResourceDefinition` is *never* deleted by hook-delete-policy**,
  hardcoded (`deleteHookByPolicy`'s own explicit early return) — "to avoid
  cascading garbage collection." A native engine that treats hook delete
  generically (interpreting the annotation literally for every kind) would
  diverge from real Helm here.
- Hook resources are created, then `WatchUntilReady`'d — Job/Pod-specific
  readiness (`pkg/kube/client.go: watchUntilReady`, other kinds are a no-op
  wait). A hook failure sets `HookPhaseFailed`, triggers optional log
  capture (`outputLogsByPolicy`, Job/Pod only, reads container logs via the
  K8s API), triggers `hook-failed`-policy deletion of the failing hook, and
  triggers `hook-succeeded`-policy deletion of every hook that *already
  succeeded earlier in this same execHook call* — then the **whole action
  returns the hook error**, aborting rollback/uninstall entirely at that
  point (rollback: before any resource `Update`; uninstall: before
  `deleteRelease`, since `pre-delete` runs first).
- On success of every hook, hook resources whose policy includes
  `hook-succeeded` are deleted (in reverse execution order) and their logs
  optionally captured.

**Verdict on hooks (brief's explicit A/B/C question)**: **(B) — a later
slice, not (A) mandatory-from-day-one, and emphatically not (C) rejected
forever.** Reasoning: a chart with `pre-delete`/`pre-rollback`/`post-*` hooks
whose delete-policy or ordering a native engine got wrong could leave
orphaned Job/Pod resources, or delete a resource the real `helm` CLI would
have kept, or apply a rollback without running a required pre-rollback
migration hook — all silent, all compatibility-breaking, and all outside
what a differential test would catch unless the fixture matrix specifically
targets it (fixtures #7-9, §14). Shipping *without* hook support is honest
only if the engine explicitly **detects and refuses** to operate on any
release whose target manifest contains hooks for the relevant event,
rather than silently skipping them — silently ignoring hooks would be
exactly the "simple releases only" shortcut M9.6 already rejected. This
detection is cheap (hooks are already present in the decoded release
record M9.5 can read) and should be MH5/MH6's own explicit guard, not an
afterthought.

## 10. Failure semantics

All of the below are traced directly from §3/§4/§5's call graphs; each row
is a genuine upstream behavior, not a guess.

| Case | What Helm v3 does | Resulting release state | Safe to retry? | Observable artifacts |
|---|---|---|---|---|
| `KubeClient.Update` fails mid-rollback | `currentRelease.Status=Superseded`, `targetRelease.Status=Failed`, both `recordRelease`d; `CleanupOnFail` optionally deletes just-created resources | current=Superseded, new revision=Failed (permanent, two records) | Yes — a fresh `helm rollback` creates yet another new revision; the Failed one is inert history | The Failed revision Secret remains forever (or until `MaxHistory` prunes it) |
| Hook fails (`pre-rollback`/`pre-delete`) | Immediate return before any resource apply/delete | Rollback: **no state change at all yet** (targetRelease's `Create` already wrote `pending-rollback` before this point though — see next row) | Yes, cluster state untouched by this action; but see "process crash" row below for the already-written pending record | `pending-rollback`/target's hook's own Job/Pod possibly left per its own delete-policy |
| `recordRelease`/`Releases.Update` fails (storage write fails) after a successful cluster mutation | Logged only (`cfg.Log`), **swallowed**, function proceeds | Cluster state reflects the change; release record may still show a stale prior status | Ambiguous — depends which write failed | This is a **silent partial-success** state Helm itself accepts; a native engine copying this behavior needs the same class of "journal-must-not-silently-fail" discipline SAUR-ON already requires elsewhere (`mutation::journal`), which is a genuine tension (§11) |
| Target rollback revision missing from history | `prepareRollback` returns `"release has no %d version"` before any write | No state change | Yes | None |
| Resource delete fails / already gone (uninstall) | `errs` collected, does **not** abort the loop (`Delete`/`DeleteWithPropagationPolicy` continue across resources); function returns the joined errors *after* still setting `Status=Uninstalled` | `Uninstalled`, but "Info" field lists what was intentionally kept, **not** what failed to delete | Yes, but a retry will attempt every "still-there" resource again (idempotent from K8s's point of view — DELETE-on-missing is a no-op via `apierrors.IsNotFound` handling in `rdelete`) | Actual live resources may still exist despite `Uninstalled` status — this is Helm's own documented tradeoff, not a bug to "fix" silently |
| Malformed release record (undecodable Secret) | `decodeRelease`/`Storage.Get` returns an error; `History` propagates it; the whole action fails before mutating anything | No change | Yes | None — matches this repo's own M9.5 `DecodeError` discipline already |
| Process crash after `Releases.Create(targetRelease)` but before `performRollback` completes | New revision permanently stuck at `pending-rollback` | `pending-rollback` forever until a human/another `helm rollback` intervenes | A subsequent `helm rollback` creates **yet another** new revision (N+2) — it does not resume or clean up the stuck N+1 | The stuck `pending-rollback` Secret remains as permanent history noise |
| Cluster unreachable mid-operation | `IsReachable()` check happens only at the very start of `Run`; a mid-operation network partition surfaces as whatever `client-go` error the in-flight call returns, propagated as-is (no special handling) | Same shape as "resource patch fails" above | Depends on exactly which write completed | Ordinary transport error, no Helm-specific classification |
| Storage update succeeds, resource reconciliation fails | Only possible via `rollback.go`'s own explicit ordering: `Releases.Create(target as pending)` happens *before* `performRollback`'s cluster mutation — so this is the "process crash after some writes" row again, not a separate path | `pending-rollback` (or `Failed`, if the error surfaces before crash) | See above | See above |
| Resource reconciliation succeeds, storage update fails | `recordRelease`/`Releases.Update` swallow-logs the error (see row 3) | Cluster already reflects new state; Secret may still say `pending-rollback` or an earlier status | A human running `helm status` sees a **stale/wrong** status relative to the live cluster — Helm does not reconcile this automatically | This is the scenario SAUR-ON's own `Verification::ObservedDifferent`-style "don't collapse commit vs observed" discipline is *already built for* — directly relevant to §12 |

**Cross-cutting finding**: Helm does not treat its own release-record
consistency as sacred against process death — it accepts "stuck pending"
and "status says X but cluster says Y" as known, user-visible failure
modes recoverable only by another explicit human-initiated operation. A
native engine claiming compatibility must reproduce this same *shape* of
failure (not a "better", silently-self-healing one) or it risks producing
release records the real Helm CLI's own `pending`-handling logic (in
`install`/`upgrade`, not inspected here, out of scope for rollback/uninstall
but real) does not expect.

## 11. Security implications

Builds directly on M9.5's already-accepted model
(`src/integrations/helm.rs`, `src/kube/helm.rs`) rather than replacing it.

**What stays exactly as-is, zero change**: `resources::Object::new`'s
unconditional Secret redaction; no generic raw-Secret helper anywhere;
`decode_release`/`sanitize` staying `pub(crate)`/non-persistent; the
existing bounded-gunzip/base64 discipline (`MAX_DECOMPRESSED_BYTES`, the
distinct decode-error taxonomy) — a write-side engine reuses these
read-side primitives verbatim rather than reinventing decode.

**What a write-capable engine genuinely needs that M9.5 did not**:

1. **Encoding, not just decoding** — the inverse of
   `integrations::helm::decode_release`: JSON-marshal a `Release`-equivalent
   struct, gzip it, double-base64 it, and write it as a *new* Secret's
   `data.release`. This is new surface area, and it is exactly the kind of
   capability M9.5's own doc comment warned must stay "the narrowest
   possible write capability" if ever added. Recommendation: a single
   `kube::helm::write_release` (mirroring `read_release`'s own shape: one
   function, `pub(crate)`, never generalized into a Secret-writing helper
   any other caller could reach for), taking an already-validated,
   in-memory release record and performing exactly one bounded Create/Update
   — never a batch, never exposed to a generic mutation payload path.
2. **The full unredacted release record must exist in memory transiently**
   during a rollback (to reconstruct the target revision's manifest/config
   verbatim) — this is unavoidable and matches what real Helm itself does;
   the constraint is that this unredacted value must never cross into
   `Object`/`Store`/`Timeline`/graph/journal, exactly as M9.5 already
   guarantees for reads. A write-side `HelmReleaseRecord` (raw, internal,
   never `Debug`-derived in a way that could leak into a panic message —
   restated from M9.5's own `error_display_never_embeds_raw_payload_content`
   test discipline, which a native engine's own error types must inherit)
   should be as narrowly scoped as `decode_release`'s current return value.
3. **The mutation journal must never receive the raw release payload.**
   `mutation::journal::Record` already redacts payloads generically
   (per M8); a Helm rollback/uninstall's journal entry should record
   *intent* (`source_action: "helm_rollback"`, target release name/
   namespace/from-revision/to-revision) and *outcome*, never the decoded
   chart values/manifest/notes. This is a new, explicit design decision
   for whichever milestone adds Helm write support — not something the
   existing generic redaction automatically covers, because the journal
   entry for this action is Helm-specific structured data, not a generic
   Kubernetes PATCH payload hash the way `mutation::workflow`'s existing
   `payload_sha256` model already handles scale/restart/delete/label/
   annotate.
4. **No logs containing raw values/manifest/notes** — inherits directly
   from M9.5's own Notes-exclusion decision (§ "sanitize" in
   `integrations/helm.rs`); a native engine's own hook-log-capture feature
   (§9, if ever implemented) is a *new* place raw secrets could leak
   (hook Job stdout can legitimately contain anything, including
   credentials a chart's hook script printed) — this should be treated as
   its own explicit, separately-reviewed security decision if/when hooks
   are implemented, not inherited implicitly from the values/manifest
   redaction model, because log content has no structure to redact by key
   (same reasoning M9.5 already used to justify omitting Notes entirely).

**Recommendation**: widen the M9.5 boundary by exactly one narrow write
capability (`kube::helm::write_release`), never by relaxing
`Object::new`'s redaction or adding a second, more permissive Secret path.
This should be re-stated as its own explicit security design decision in
whatever milestone first implements it (MH1, §17), following M9.5's own
precedent of writing the decision down before implementing it.

## 12. Concurrency / TOCTOU implications

Helm itself has **no locking** (§7) — this is not a gap to be politely
worked around; it is documented, accepted upstream behavior. SAUR-ON's own
M7/M8B TOCTOU model (exact UID/resourceVersion revalidation at commit time,
restated in `HANDBOOK.md`'s own "UID != NAME" invariant) is *stricter* than
what Helm requires of itself, which is good — but it also means SAUR-ON's
model has to be applied at a layer Helm's own code never checks, or the
"stricter" claim is not actually true where it matters.

Concrete design requirements, none of them satisfied by "release name
alone":

1. **Identify the release by its current-revision Secret's UID**, not by
   name — mirroring exactly `kube::helm::read_release`'s existing
   "re-verify UID against the freshly fetched object" pattern. A rollback
   intent built from a stale selection (user picked release X, a
   concurrent real `helm` operation already advanced it to a new revision)
   must fail closed the same way `HelmReadError::TargetReplaced` already
   does for reads.
2. **Revalidate the *target* history revision's own UID** at commit time
   too, not just the release's "latest" Secret — a concurrent operation
   could have pruned (`MaxHistory`) or overwritten the exact revision this
   rollback intends to restore between preview and commit.
3. **Treat any observed `pending-*` status as a hard precondition
   failure**, never proceed past it silently. Real Helm's own `Run` does
   not check this defensively (it will happily let two concurrent
   `helm rollback` invocations race, per §7's own admission) — SAUR-ON
   should not match that specific laxity; refusing to act on an already-
   `pending-*` release (and reporting that fact, not silently retrying or
   overriding) is the deliberately *safer* divergence this project should
   make, and it should be written down as exactly that: an intentional,
   documented divergence from upstream behavior, not an accidental one.
4. **Reuse M7's existing TOCTOU/commit gateway wholesale** for the
   Kubernetes-resource side of the operation (the per-resource
   create/patch/delete calls) — `kube::mutation::commit`'s existing
   fresh-UID/resourceVersion revalidation before every write is exactly the
   right primitive; a native Helm engine's `apply`/`delete` layer (§6)
   should be a new *executor dispatch branch* under the same gateway
   (mirroring M9.2/M9.4's own precedent of "no second gateway"), not a
   parallel implementation.
5. **No hidden retries** — restated from the M8B invariant this document
   was explicitly told to preserve. Helm's own `createResource`/
   `deleteResource` wrap every single API call in
   `retry.RetryOnConflict(retry.DefaultRetry, ...)` (`pkg/kube/client.go`)
   — i.e. **real Helm itself silently retries on 409 Conflict**. A native
   engine that copies this literally would violate SAUR-ON's own no-hidden-
   retry rule. This is a genuine, source-confirmed tension between
   "behave like Helm" and "SAUR-ON owns retry semantics" — the resolution
   this document recommends: surface the 409 as an explicit outcome
   (`Verification`/`MutationOutcome`-shaped, exactly like every other
   SAUR-ON action) and let the *user* decide to retry, documented as a
   deliberate, visible divergence from upstream's silent retry — not
   copy Helm's retry loop verbatim. Silently retrying would also reintroduce
   exactly the hidden-mutation-outcome hazard M8B's own investigation
   already found and fixed once.

## 13. Differential test strategy

Design (no code written — this is the test *design*, per the brief):

```
Fixture (chart + values + prior-revision-history setup)
   │
   ├─► Path A: real `helm` CLI operates on a real release  ──► capture observable result A
   │
   └─► Path B: native engine operates on an *independently re-created*
       equivalent release (same chart/values/history, built via the same
       real `helm` CLI so the STARTING state is identical, then the
       *operation under test* — rollback/uninstall — is performed by the
       native engine instead)  ──► capture observable result B

Compare A vs B, then:
       CRITICAL STEP — hand release B (native-engine-touched) back to the
       real `helm` CLI and run `helm status`, `helm history`, and (for
       rollback fixtures) a follow-up `helm rollback`/`helm upgrade`.
       The real CLI must not error, must not report corruption, and its
       own observable behavior on release B must match what it would have
       produced had A's operation been performed by itself.
```

What to compare, concretely (each with its own upstream source location to
diff against, not just "looks similar"):

- **Kubernetes resources**: full spec diff (not just presence/absence) —
  a native engine using two-way merge where Helm used three-way merge will
  *look* identical on first apply and diverge only on a later change,
  exactly the "LOOKS EQUIVALENT != BEHAVES EQUIVALENT" trap the brief
  names explicitly. Test must include a *second* operation after the
  first (e.g. rollback, then a real `helm upgrade`) specifically to expose
  this class of bug.
- **`managedFields`**: manager name and operation type (`Apply` vs
  `Update`) per §8 — this is the single most important field to diff,
  since it determines whether a *subsequent* real `helm` operation will
  conflict.
- **Labels/annotations** Helm itself sets on both the release Secret
  (`name`/`owner`/`status`/`version`/`createdAt`/`modifiedAt`) and on
  managed resources (`app.kubernetes.io/managed-by: Helm`,
  `meta.helm.sh/release-name`, `meta.helm.sh/release-namespace` — observed
  live in this session's experiment on the scratch ConfigMap).
- **Release revision numbering, status, and full history list** — exact
  string/int equality against what real Helm produced for an equivalent
  operation, not "close enough."
- **Stored release metadata** — full round-trip: decode the native
  engine's written Secret with the *real* `decodeRelease`-equivalent logic
  (in practice: run `helm get manifest`/`helm get values` against it) and
  confirm it parses and matches expectations; a self-decode-only test is
  not sufficient evidence.
- **Hooks executed / hook resources / kept resources / deleted resources**
  — set equality, ordering where order is itself observable (hook weight
  order, uninstall kind order).
- **Errors / partial-failure state** — for each §10 failure row, verify the
  native engine reaches the *same* named failure state (not just "also
  fails"), and that a human using `helm status` afterward sees a
  consistent story.
- **Subsequent real Helm CLI compatibility** (the brief's own "CRITICAL
  TEST") — this is not optional and not last-priority; it should gate
  every fixture, not just a final smoke test, because it's the only check
  that actually falsifies "looks equivalent."

## 14. Fixture matrix

Expanded from the brief's proposed 20 with two additions (marked *) found
necessary by the §8 SSA/CSA finding:

| # | Fixture | Primary risk it targets |
|---|---|---|
| 1 | Simple Deployment + Service | Baseline apply/delete correctness |
| 2 | ConfigMap update between revisions | Two-way vs three-way patch divergence |
| 3 | Secret in chart | Redaction/journal-leak boundary (§11) never triggered by an in-chart Secret resource, as distinct from the release-storage Secret itself |
| 4 | Resource added between revisions | Create-on-rollback path |
| 5 | Resource removed between revisions | Delete-on-rollback path (the `original.Difference(target)` branch in `pkg/kube/client.go:update`) |
| 6 | Manually modified live resource, then rollback | §8's three-way-merge-vs-live-edit interaction — the single riskiest correctness fixture |
| 7 | Hook Job (pre/post-rollback or pre/post-delete) | Hook execution correctness |
| 8 | Multiple hooks, weight ordering | Hook ordering |
| 9 | Hook failure (nonzero exit) | Abort-before-mutation semantics (§9/§10) |
| 10 | `resource-policy: keep` | Uninstall must skip exactly the annotated-in-*stored-manifest* resources (§5.5) |
| 11 | PVC | Delete-propagation risk, StorageClass/reclaim interaction |
| 12 | CRD + CR | Two-way (not three-way) merge path for unstructured objects (§8); `deleteHookByPolicy`'s CRD-never-delete-by-hook-policy rule (§9) |
| 13 | Already-missing resource (deleted out-of-band before rollback/uninstall) | `apierrors.IsNotFound` handling — must be a no-op success, not an error |
| 14 | API conflict (409 on patch/delete) | Retry-vs-surface decision (§12's documented divergence) |
| 15 | Timeout (Wait/WatchUntilReady exceeds Timeout) | Failure-state correctness (§10) |
| 16 | Rollback to an older (not immediately-prior) history entry | `prepareRollback`'s explicit-`Version` path |
| 17 | Repeated rollbacks (rollback, then rollback again) | Monotonic revision numbering under repeated operations; superseded-set correctness |
| 18 | Uninstall with history kept | `helm status`/`history` must keep working afterward |
| 19 | Uninstall without history kept (purge) | All revision Secrets gone; re-install starts at revision 1 |
| 20 | Interrupted/partial operation (kill the native engine process between storage-write and cluster-apply) | §10's "process crash after some writes" — reproducible safely only in a disposable test cluster, by design (kill -9 the SAUR-ON process mid-commit against a throwaway namespace) |
| 21* | Release originally installed/upgraded via Helm v3 (CSA/three-way-merge), then rolled back by the native engine | §8's CSA compatibility path |
| 22* | Release originally installed/upgraded via Helm v4 (SSA-default), then rolled back by the native engine | §8's SSA compatibility path — expected to be the fixture that first exposes whether the native engine's own field-manager string is correct |

## 15. Rust architecture proposal

The brief's proposed shape holds up well against the research; the
following refines it with the specific primitives found in §3-§9:

```
NativeHelmEngine (workspace crate — see §16)
├── storage
│   ├── secret_driver      // read via existing kube::helm-style bounded GET (M9.5 reuse),
│   │                       // write via a new, equally narrow write path (§11)
│   ├── record             // Release-equivalent struct: name/namespace/version/info/
│   │                       // chart/config/manifest/hooks/labels — mirrors pkg/release/release.go
│   ├── history             // query/sort-by-revision/supersede/prune (MaxHistory)
│   └── encode_decode       // reuses integrations::helm::decode_release's decode side;
│                            // adds the inverse encode (gzip+base64x2) for write
├── manifest
│   ├── split_sort          // SplitManifests + kind-ordered sort (install/uninstall order tables, §7 literal lists)
│   └── resource_policy      // helm.sh/resource-policy: keep filter, read from the STORED manifest (§5.5)
├── apply
│   ├── csa                 // three-way strategic-merge-equivalent patch generation (§8, v3-compatible path)
│   ├── ssa                 // Server-Side Apply path with explicit field manager (§8, v4-compatible path)
│   └── delete               // propagation-policy-aware delete, NotFound-is-success (§10/§14 fixture 13)
├── lifecycle
│   ├── status_machine       // pending-*/deployed/failed/superseded/uninstalling/uninstalled transitions (§7, §10)
│   └── revision              // strictly-monotonic next-version allocation
├── hooks (deferred slice, §9 — but the module boundary should exist even
│         before it is implemented, so "no hooks yet" is an explicit,
│         checkable gate rather than an absence)
├── actions
│   ├── rollback              // composes storage+manifest+apply+lifecycle exactly per §3/§4
│   └── uninstall              // composes storage+manifest+apply+lifecycle exactly per §3/§5
└── compat_tests               // differential harness, §13 — real `helm` as oracle, never as runtime dep
```

Design answers to the brief's specific questions:

- **`kube-rs` primitives**: yes, reuse — `kube::Api<DynamicObject>` +
  discovery (SAUR-ON already has a `Resource`/discovery/catalog model,
  `src/kube/discovery.rs` by inspection of imports in `mutation/workflow.rs`)
  is the right base for both CSA-equivalent patch and SSA paths. No need for
  typed per-Kind Rust structs — Helm itself operates generically on
  rendered YAML, and SAUR-ON's own read paths already favor
  `serde_json::Value`/`DynamicObject`-shaped access (see `integrations::helm`'s
  own use of `serde_json::Value` throughout).
- **`DynamicObject` vs typed**: `DynamicObject`, matching Helm's own
  "manifest is just YAML documents" model and this codebase's existing
  convention.
- **SSA vs patch generation**: **both are needed** (§8) — this is not a
  choice, it's a compatibility requirement driven by which regime a given
  release's history was produced under. `kube-rs`'s own
  `Api::patch`/`PatchParams::apply` already exposes SSA directly; a
  three-way-merge-equivalent (matching `strategicpatch.CreateThreeWayMergePatch`'s
  actual algorithm, not a hand-rolled approximation) is the harder half —
  this is real, non-trivial algorithm-porting work, not a thin wrapper.
- **Server-side dry-run**: yes, use it for the engine's own preview step —
  this is exactly the shape M7's existing `dry_run: Option<MutationOutcome>`
  field on `Workflow` (`src/mutation/workflow.rs`) already expects; a
  native Helm action's `Built`/`Workflow` integration should populate this
  the same way every other M7/M8 action does, not invent a second preview
  mechanism.
- **Explicit field managers**: yes — the engine must own and use an
  explicit, documented field-manager string (e.g. `"sauron-helm-engine"`),
  never silently impersonate `"helm"`. This is itself a design decision
  with a real consequence: impersonating `"helm"` might look more
  "compatible" superficially, but a subsequent real `helm` operation would
  then see its own expected field ownership already present from an
  operation it didn't perform — the more honest and genuinely-compatible
  choice is a distinct manager name plus (if adopting the CSA→SSA-style
  migration Helm v4 itself uses) an explicit, logged "taking over
  management" step, never silent impersonation.
- **Kubernetes discovery**: reuse SAUR-ON's existing discovery/catalog
  (already used by M9.2/M9.4's guarded actions per `M9_ACCEPTANCE.md`) —
  no new discovery mechanism.
- **Existing bounded-read infrastructure**: reuse for every read
  (`read_bounded`, M9.5's decode/redaction). New write path is the one
  addition (§11), scoped as narrowly as the read path already is.
- **Existing mutation gateway / verification / journal / policy**: reused,
  not duplicated — restated from §12: the engine's `apply`/`lifecycle`
  layers produce the *effect description*; `kube::mutation::commit`/
  `verify` remain the single place a write actually happens and gets
  verified, and `mutation::policy::evaluate`/`mutation::journal` remain
  the single policy/journal path, exactly as M9.0's own carried-over
  design rule already requires for every M9 action. This is a hard
  constraint, not a suggestion: a Native Helm Engine that bypassed M7 to
  "go faster" would recreate the exact "second gateway" mistake
  `M9_ACCEPTANCE.md`'s own design rule was written to forbid.

## 16. Crate-boundary recommendation

| | (A) inside SAUR-ON modules | (B) internal workspace crate (`crates/helm-engine`) | (C) standalone published crate |
|---|---|---|---|
| Coupling | Tightest — easy to accidentally reach into `Object`/`Store` internals, exactly the risk M9.5's own doc comments repeatedly guard against | Deliberately loose — a workspace crate can only see what it's given (release bytes in, effect description out), forcing the M9.5-style narrow boundary to be structural, not just a comment | Loosest, but that looseness is now a *public API*, permanently |
| Testability | Fine, but differential/compat tests would live awkwardly alongside UI/mutation-gateway tests | Clean — a self-contained `compat_tests` suite (§13) with its own `helm`-CLI-as-oracle harness, independent of SAUR-ON's own test infra/cluster fixtures | Same, plus the burden of a public test/CI matrix across Helm versions this project doesn't otherwise need to promise |
| Security boundary | Weaker by default — nothing stops a future SAUR-ON change from importing the engine's raw-record type somewhere unintended, unless discipline holds (same discipline M9.5 already relies on for `decode_release`'s `pub(crate)`) | Stronger — a crate boundary makes "this type is not `pub` outside the engine crate" an actual compiler-enforced fact `Cargo.toml`/module visibility, not just a comment | Same technical strength as (B), but now visible to arbitrary external consumers who might reasonably expect a *stable*, general-purpose Helm library — a burden this research found no evidence SAUR-ON should take on |
| Reuse | None beyond this app | Possible later reuse by another internal tool without depending on all of SAUR-ON | Real reuse value *if* the engine turns out correct and stable — but that's unproven (§2's own compatibility risk) |
| API stability | N/A (private) | Free to change per-milestone, matches this project's own slice-by-slice acceptance discipline | Freezing a public API before SSA/CSA compatibility (§8) is even proven would very likely bake in a wrong abstraction |
| Maintenance burden | Lowest short-term, highest long-term risk of boundary erosion | Moderate — one more crate to version inside the workspace, worth it for the enforced boundary | Highest — issue triage, semver commitments, Helm-version-drift maintenance for a userbase this project doesn't currently have |

**Recommendation: (B), internal workspace crate**, matching the stated bias
and confirmed rather than merely accepted by this research: the M9.5
security model already treats "narrow, explicit, compiler-enforced
boundary" as load-bearing (`pub(crate)` on `decode_release`), and a write-
capable engine's raw release record is a strictly higher-risk object than
M9.5's read-only one — the case for a hard, crate-level boundary is
*stronger* here, not weaker. Do not evaluate publishing (C) again until
§13's differential compatibility testing has actually passed across both
the CSA and SSA fixtures (§14, #21/#22) — an unproven public crate would
either be quietly wrong or would need to over-promise compatibility it
hasn't earned.

## 17. Initial scope vs future scope

> **Superseded in scope (not in content) by §37.** This section's own
> analysis (rollback/uninstall only) is left exactly as originally written
> below — it remains correct for the question it answered. §37, added in
> the 2026-09-21 extension pass, gives the comprehensive three-tier
> recommendation across Helm's *entire* command surface (install/upgrade/
> template/pull/push/oci/test/repo/dependency/plugin/diff/get/history/lint)
> and should be read as this document's current, complete scope
> recommendation. Read this section first for the rollback/uninstall-only
> reasoning that §37 builds on, then §37 for the full picture.

### Required lifecycle core (needed for rollback/uninstall correctness at all)

- Release storage read+write (Secret driver only — ConfigMap/SQL/Memory
  drivers are real upstream options but out of scope; SAUR-ON only needs
  to interoperate with what real clusters actually use, which is
  overwhelmingly the Secret driver **[DOCS: this is Helm's own documented
  default since v3]**).
- History query/sort/supersede/prune.
- Manifest split/sort/resource-policy filter.
- CSA three-way-merge-equivalent apply (v3-compatible releases).
- Delete with propagation policy, NotFound-is-success.
- Status/pending-state machine.
- Revision allocation.
- Explicit, hard refusal (not silent skip) when the target release/revision
  contains hooks for the relevant event, until hooks are implemented (§9).
- Explicit, hard refusal when the live resources show SSA management the
  engine cannot yet safely interoperate with (§8), until SSA support is
  implemented and proven (fixture #22).

### Nice-to-have later (real value, not required for rollback/uninstall)

- Hook execution (§9) — genuinely valuable, genuinely hard, correctly
  deferred rather than rejected.
- SSA apply path (§8) — required for full compatibility, but the CSA-only
  first slice is honestly narrower and still useful for the (still common)
  population of v3-managed releases.
- Upgrade (existing release, new values/chart) — shares almost all of
  rollback's machinery (same `Update`/patch/apply layer) but was already
  explicitly out of scope for M9 for unrelated reasons (no safe bounded
  values/chart-input model) and that reasoning is untouched by this
  research.
- Install — same caveat as upgrade, plus chart-fetch/render (Go template
  engine) which is an entirely separate, much larger body of upstream
  logic (`pkg/chart`, `pkg/chartutil`, `pkg/engine`) this research did not
  investigate because rollback/uninstall never render a chart — they reuse
  the *already-rendered* manifest stored in the release record. This is an
  important, source-confirmed boundary: **a native rollback/uninstall
  engine never needs a Helm template engine at all.**
- `helm test` (hooks with `test` event) — shares the hook engine, no
  separate primitive.
- History management (`helm history`, already fully covered read-only by
  M9.5).

### Explicitly out of initial scope (validated, not assumed)

- Chart authoring/templating engine (`pkg/engine`, `pkg/chartutil`) —
  confirmed unnecessary for rollback/uninstall (see above).
- Repo management, chart search, packaging, OCI registry client,
  dependency resolution — none of these are referenced anywhere in
  `pkg/action/rollback.go` or `pkg/action/uninstall.go`'s own call graph;
  confirmed genuinely irrelevant to this milestone's actual scope, not
  merely assumed out.
- Plugin system — same, no dependency found.
- Any non-Secret storage driver — no evidence any target cluster needs one.

## 18. Milestone plan

| Milestone | Scope | Non-goals | Stop condition |
|---|---|---|---|
| **MH0** | Upstream behavior map / compatibility contract — essentially this document, formalized into a written contract (exact field/label/status semantics this engine promises to reproduce, versioned against the Helm release-object schema `v1`) | No code | A second engineer can implement MH1 from MH0 alone without re-reading upstream source |
| **MH1** | Release storage read+write, round-trip compatibility: write a release record, read it back with the *real* `helm get manifest/values/hooks`, confirm byte-for-byte semantic equivalence to what real `helm` itself would have written for an equivalent operation | No lifecycle logic yet — this milestone writes releases nobody asked it to compute the *content* of; content is hand-fixtured | Differential test: real `helm`-written Secret vs engine-written Secret for an identical hand-specified `Release` struct decode identically |
| **MH2** | Lifecycle state machine: pending-*/deployed/failed/superseded/uninstalling/uninstalled transitions, revision allocation, supersede-all-prior-deployed | No cluster mutation yet — pure state-machine unit tests against §10's failure table | Every §10 row has a passing unit test reproducing the exact resulting state |
| **MH3** | Resource reconciliation: manifest split/sort/resource-policy filter + CSA three-way-merge-equivalent apply/delete against a real (disposable) cluster | No SSA yet, no hooks yet — explicit hard-refusal guard for both | Fixtures #1-6, #11-14 (§14) pass differentially against real Helm v3-managed releases |
| **MH4** | Hook engine (§9's full state machine: weight order, delete-policy, log capture, failure-abort) | No new lifecycle actions — hooks are a service the actions consume | Fixtures #7-9 pass differentially |
| **MH5** | Native rollback, composing MH1-4 exactly per §4's call graph, including the explicit hook/SSA refusal guards from §17 | No uninstall yet | Fixtures #1-9, #16-17, #21 pass differentially, including the "subsequent real `helm upgrade`" critical test (§13) |
| **MH6** | Native uninstall, composing MH1-4 per §5 | No rollback changes | Fixtures #10, #13-15, #18-20 pass differentially |
| **MH7** | SSA apply path (§8) + CSA→SSA regime detection | — | Fixture #22 passes; a release rolled back by the engine can be correctly `helm upgrade`d afterward regardless of which apply strategy produced its history |
| **MH8** | Differential compatibility acceptance — the full fixture matrix (§14), run as a dedicated live-cluster suite (mirroring `scripts/test-cluster.sh`'s existing `m9-*`-prefixed convention), against a matrix of real `helm` v3.22.0 and v4.x binaries as oracles | — | Every fixture passes for both oracle versions; any fixture requiring a feature not yet built is an explicit, named refusal, never a silent pass |
| **MH9** | SAUR-ON integration: `mutation::workflow`-shaped `Built`/`Workflow` builders for `helm_rollback`/`helm_uninstall`, dispatched through the existing `kube::mutation` executor/journal/policy gateway (no new gateway, per §12/§15) | No UI beyond what M7/M8's existing shared shell already provides | Design test restated from `M9_ACCEPTANCE.md`'s own carried-over discipline: adding this action required no new confirmation/policy/journal/TOCTOU subsystem |

Each milestone's tests should include, per the brief: unit tests (pure
model/state-machine logic), fake-HTTP tests (matching this project's
existing `read_bounded`-style transport test convention, no real cluster),
live tests (disposable-namespace, `mutation_test_cluster_verified`-gated,
exactly like every other M7/M8/M9 live test in this codebase), and
differential Helm-oracle tests (§13) — the last category is *new* to this
project (no prior milestone needed a second real implementation as an
oracle) and should get its own documented harness convention in MH0.

## 19. Major risks

1. **Server-Side Apply / three-way-merge dual-mode correctness (§8)** —
   the single largest, most concrete risk. Getting the CSA path
   byte-for-byte algorithmically equivalent to `strategicpatch.CreateThreeWayMergePatch`
   is real porting work with real edge cases (list merge keys,
   `patchMergeKey` strategic-merge tags per field, etc.) that this research
   did not exhaustively enumerate — flagged, not solved (§20).
2. **Hook semantics are a full sub-lifecycle, not a bolt-on** — deferring
   them (§9) is correct, but the deferral must be an explicit runtime
   refusal, or the engine silently ships an incompatible partial
   implementation exactly like the one M9.6 already rejected once.
3. **No upstream locking to rely on (§7/§12)** — SAUR-ON's own stricter
   TOCTOU discipline must be layered on top of, not assumed to already
   exist inside, Helm's own model.
4. **Silent Helm-internal retries on 409 (§12)** — a literal port would
   quietly violate this project's own no-hidden-retry invariant; this must
   be a deliberate, documented divergence.
5. **Field-manager identity choice (§15)** — impersonating `"helm"` looks
   more "compatible" short-term and is the more dangerous choice long-term;
   this needs to be decided once, explicitly, and never revisited
   ad hoc per-milestone.
6. **Differential testing requires maintaining two real Helm binaries as
   oracles (v3 and v4) indefinitely** — a real, ongoing maintenance cost,
   not a one-time setup.

## 20. Explicit unknowns requiring experiments

This research had live network access to `github.com/helm/helm` and a real
`helm` v4 CLI plus a disposable Kubernetes namespace, and used both — most
of the brief's "verify from source, not assumption" requirement was
satisfiable. What remains genuinely unverified in this pass:

1. **`strategicpatch.CreateThreeWayMergePatch`'s own algorithm** (in
   `k8s.io/apimachinery`, not `helm/helm` itself) was not fetched/read in
   this pass — only *that Helm calls it* and *with what inputs* was
   confirmed from `pkg/kube/client.go`. Porting the actual three-way-merge
   algorithm (list merge-key handling, patchStrategy tag semantics) needs
   its own dedicated source study of `k8s.io/apimachinery/pkg/util/strategicpatch`
   before MH3 can start — this is real, uncovered ground, not merely
   unread background.
2. **HIP-0023's own proposal text** was not fetched directly (only its
   *implemented consequence* in v4.3.0 source, which is stronger evidence
   for behavior but weaker for *intent/rationale* — if MH7 needs to justify
   a design choice against Helm maintainers' own stated reasoning, that
   text should be fetched then.)
3. **`pkg/kube/wait.go`/`ready.go`'s full per-Kind readiness logic** (only
   `Job`/`Pod` were confirmed from `watchUntilReady`; the newer
   `kube.Wait`/`WaitWithJobs` interfaces referenced in `rollback.go` were
   fetched as files but not read line-by-line in this pass — flagged as
   incomplete rather than silently assumed simple).
4. **Helm v4's `install.go`/`upgrade.go`** were not fetched — this
   research deliberately scoped to rollback/uninstall per the brief, but
   MH0 (§18) should re-confirm the "no template engine needed" claim (§17)
   holds for v4 specifically, not just v3, before treating it as settled.
5. **The exact `k8s.io/client-go/util/csaupgrade` `UpgradeManagedFields`
   behavior** referenced in the v4 source comment (§8) — cited, not read.
   This is the literal mechanism MH7 would need to either reproduce or
   deliberately reject; it must be read in full before MH7 starts, not
   before MH0.
6. **ConfigMap storage driver behavior** was not experimentally verified
   (only Secret, which is Helm's actual default and what the M9.5 exception
   already targets) — low priority given §17's own scoping decision to
   exclude non-Secret drivers, but noted as unverified rather than silently
   assumed identical.
7. **No experiment reproduced a mid-operation process crash (§10's "process
   crash" rows) against a live cluster in this session** — the failure-mode
   table in §10 is source-derived (what the code *would* do if interrupted
   at each point), not independently reproduced by actually killing a
   process mid-write. This is exactly the kind of claim the brief warns
   against silently asserting; flagged explicitly rather than presented as
   equally strong evidence to the other, live-confirmed rows.

None of the above were filled in with assumptions in the body of this
document above — every claim in §3-§14 traces to either a fetched source
file, an official doc, or a command actually run in this session. These
seven items are the concrete next-step reading/experiment list for MH0.

## 21. Recommendation: build / narrow / do not build

**Build — narrowly, in the exact scoped order of §17/§18, gated by the
differential proof of §13, and only after the §20 unknowns are closed for
whichever slice needs them.** The prior M9.6 deferral was the right call
given M9's own timeline and the absence of this research; this research
does not overturn that decision retroactively — it gives the specific,
evidence-backed path M9.6's own write-up asked a future milestone to walk.
The single largest reason this is "build, not don't-build": every piece of
Helm's rollback/uninstall behavior traced in this document has a concrete,
readable, non-mysterious upstream source location — there is no unknowable
black box, no closed-source dependency, and no server-side component that
cannot be reimplemented in principle. The risk is *engineering correctness
and completeness*, not *fundamental infeasibility*.

---

## FINAL DECISION FORMAT

FEASIBILITY:
Medium-high. Every behavior needed for rollback/uninstall has a concrete,
source-verified upstream location (§3-§10); nothing is a black box. The
real risk is faithfully reproducing two genuinely different apply
protocols (CSA three-way-merge vs SSA, §8) and the full hook sub-lifecycle
(§9), not "can this be done at all."

RECOMMENDED ENGINE BOUNDARY:
An internal workspace crate (`crates/helm-engine`, §16) exposing only:
release read (reusing M9.5's existing decode/sanitize), a narrow release
write primitive, a manifest/resource-policy layer, a dual-mode (CSA+SSA)
apply/delete layer, and a lifecycle/status-machine layer — composed into
`rollback`/`uninstall` actions that return `mutation::workflow`-shaped
`Built`s for SAUR-ON's *existing* M7 gateway to execute/journal/verify.
Never a second gateway, never a generalized "Helm client" surface.

FIRST SUPPORTED ACTIONS:
Rollback and uninstall, and only for releases the engine can positively
detect as compatible with its current capability (no hooks for the
relevant event present in the target manifest; CSA-style history first,
SSA added in MH7) — with any release outside that detected envelope
explicitly refused with a stated reason, never silently attempted.

MUST-HAVE BEFORE FIRST WRITE:
1. MH0's written compatibility contract, formalized from this document.
2. MH1's round-trip storage compatibility proof (real `helm get` against
   an engine-written Secret).
3. The §20 items #1 and #7 closed for whatever slice is about to write
   (strategic-merge algorithm study before MH3; a real crash-mid-write
   experiment before MH2/MH5's failure-state claims are trusted).
4. An explicit, written decision on the field-manager identity question
   (§15/§19 item 5) and the silent-409-retry divergence (§12/§19 item 4) —
   both cheap to decide now, expensive to change after MH3 ships.

WHAT WE SHOULD NOT IMPLEMENT YET:
Hooks (defer to MH4, but gate around their absence explicitly, §9);
Server-Side Apply (defer to MH7, gate around it explicitly, §8); install/
upgrade/chart-templating (no evidence rollback/uninstall need them, §17);
any non-Secret storage driver; any standalone/published crate (§16) before
differential proof exists.

BIGGEST TECHNICAL RISK:
Porting `strategicpatch.CreateThreeWayMergePatch`'s actual merge algorithm
(and, later, real Server-Side Apply conflict semantics) correctly enough
that a subsequent real `helm upgrade` never spuriously conflicts with, or
silently overwrites, what the native engine wrote (§8, §19.1).

BIGGEST COMPATIBILITY RISK:
A release whose history spans both Helm v3 (CSA) and Helm v4 (SSA)
management — or one where SAUR-ON's own field-manager identity choice
(§15/§19.5) gets adopted inconsistently — producing a release the real
Helm CLI can still technically read but no longer safely operate on
without a conflict or a silent field-ownership loss. This is exactly the
"LOOKS EQUIVALENT != BEHAVES EQUIVALENT" failure mode the brief named up
front, and it is the one this research found the most concrete evidence
for (§8's live experiment plus the v4 source's own explicit
CSA→SSA-upgrade escape hatch).

CRATE RECOMMENDATION:
Internal workspace crate (`crates/helm-engine`), per §16. Do not publish
until the full differential fixture matrix (§14) passes against both a
Helm v3 and a Helm v4 CLI oracle, including fixture #22 (SSA-managed
release rolled back and successfully re-`upgrade`d by real Helm
afterward).

ESTIMATED MILESTONE STRUCTURE:
MH0 (contract) → MH1 (storage round-trip) → MH2 (state machine) → MH3
(CSA apply/delete, no hooks/SSA) → MH4 (hooks) → MH5 (native rollback,
CSA-only) → MH6 (native uninstall) → MH7 (SSA + CSA/SSA regime detection)
→ MH8 (full differential acceptance, dual-oracle) → MH9 (SAUR-ON
integration through the existing M7 gateway). Per §18.

SHOULD WE BUILD IT?:
Yes — with the scope discipline above, not as a general "implement Helm"
effort. The prior M9.6 deferral correctly identified that a shortcut
implementation would be dishonest about its own compatibility; this
research shows the honest, complete version is buildable in traceable
milestones, each individually falsifiable by a differential test against
the real `helm` CLI, which is the exact bar `docs/M9_ACCEPTANCE.md`'s own
M9.6 write-up asked a future milestone to clear. The work required (§19)
is real and nontrivial, particularly the apply-protocol porting (§8) — but
it is bounded, source-traceable engineering, not an open research problem.

STOP. No implementation has been started. This document does not
authorize starting MH0 or any other milestone without separate, explicit
approval.

---

# EXTENSION: Full Helm Command Surface (added 2026-09-21)

Everything above this line is the original research pass, scoped
deliberately to `rollback`/`uninstall`, and is left unmodified except for
one forward-pointer note added to §17. Nothing above was rewritten to
accommodate this extension.

This extension answers a different, broader question the original pass
explicitly declined to answer: of Helm's *entire* command surface — not
just rollback/uninstall — which pieces are worth SAUR-ON eventually
reimplementing natively, which are worth building but mostly by leaning on
existing Rust crates, and which are genuinely outside SAUR-ON's mission as
a Kubernetes *operations* console (operating already-installed things)
rather than a chart-authoring/publishing tool. Status is unchanged from the
original document: **RESEARCH ONLY**. No implementation started, no file
other than this one modified, no code written.

Same sourcing discipline as above: **[SOURCE]** (upstream Go source
file/function, fetched live from `github.com/helm/helm`), **[DOCS]**
(official `helm.sh/docs` or `opencontainers.org`/ORAS/crates.io/docs.rs for
Rust-crate factual claims), or **[EXPERIMENT]**. Unverifiable claims are
flagged explicitly in §40 rather than filled in with assumptions.

Helm versions inspected: same as the original pass — **v3.22.0** (primary)
and **v4.3.0** (where v3/v4 diverge). All new source fetches in this
extension used `v3.22.0` unless a v4-specific divergence is called out.
Live network access to `github.com/helm/helm`, `helm.sh/docs`, `crates.io`,
`docs.rs`, and GitHub was available and used throughout this extension,
the same as the original pass; the real `/usr/bin/helm` (`v4.1+unreleased`)
and the `kind-sauron-m9` cluster (kubeconfig `.test-cluster-m9/config`)
remained available but were not used for new live experiments in this
extension — the questions here (chart rendering, OCI transport, Rust crate
maturity) are answerable from source/docs without a disposable-namespace
experiment, and none was fabricated to appear otherwise; see §40.

## 22. Install behavior map

`pkg/action/install.go`, v3.22.0. Read in full for this extension.

```
Install.Run(chrt, vals)
  └─ Install.RunWithContext(ctx, chrt, vals)
       ├─ KubeClient.IsReachable()                              [skipped if ClientOnly, i.e. `helm template`]
       ├─ Install.availableName()                                [validates release name against Releases.History]
       ├─ chartutil.ProcessDependenciesWithMerge(chrt, vals)      [dependency resolution + values merge into subcharts]
       ├─ [if CRDObjects() present && !SkipCRDs] Install.installCRDs(crds)   [SPECIAL CASE — see below]
       ├─ [if ClientOnly] fake capabilities + kubefake.PrintingKubeClient + storage.Init(driver.NewMemory())
       ├─ Configuration.getCapabilities()                         [pkg/action/action.go — discovery-based Capabilities, shared with upgrade]
       ├─ chartutil.ToRenderValuesWithSchemaValidation(chrt, vals, ReleaseOptions{Revision:1, IsInstall:true}, caps, ...)
       ├─ Install.createRelease(chrt, vals, labels)                [release.Release{Version:1, Status:StatusUnknown}]
       ├─ Configuration.renderResources(chrt, valuesToRender, ...)  [pkg/action/action.go — TEMPLATE ENGINE, §25]
       │      ├─ engine.New(restConfig).Render(ch, values)  — pkg/engine/engine.go:76
       │      ├─ extract NOTES.txt
       │      ├─ releaseutil.SortManifests(files, nil, releaseutil.InstallOrder)  — pkg/releaseutil/kind_sorter.go:31
       │      └─ [if PostRenderer != nil] pr.Run(buf)
       ├─ rel.SetStatus(StatusPendingInstall)
       ├─ KubeClient.Build(rel.Manifest)
       ├─ resources.Visit(setMetadataVisitor(name, ns, force=true))  [stamps managed-by/release-name/release-namespace]
       ├─ [if !ClientOnly && !isUpgrade] existingResourceConflict(resources,...) or requireAdoption(resources,...)
       ├─ [if isDryRun] return rel early
       ├─ [if CreateNamespace] KubeClient.Build+Create(Namespace object)
       ├─ [if Replace] Install.replaceRelease(rel)   [marks prior same-named release Superseded, bumps Version to last+1]
       ├─ Releases.Create(rel)                        [stores as revision 1 (or last+1 under Replace) BEFORE resources are applied]
       └─ Install.performInstallCtx → Install.performInstall(rel, toBeAdopted, resources)
              ├─ execHook(rel, HookPreInstall, timeout)          [pkg/action/hooks.go — SAME hook engine as rollback/uninstall/upgrade]
              ├─ [len(toBeAdopted)==0] KubeClient.Create(resources)
              │  [else] KubeClient.Update(toBeAdopted, resources, Force)  [or UpdateThreeWayMerge if TakeOwnership]
              ├─ [if Wait] KubeClient.Wait / WaitWithJobs(resources, timeout)
              ├─ execHook(rel, HookPostInstall, timeout)
              ├─ rel.SetStatus(StatusDeployed, ...)
              └─ Install.recordRelease(rel) → Releases.Update(rel)
       (on any performInstall error) → Install.failRelease(rel, err)
              └─ [if Atomic] NewUninstall(cfg).Run(ReleaseName)   [full uninstall of the just-created release, KeepHistory=false]
```

**CRD install-first special casing** — `installCRDs` (`pkg/action/install.go`,
same file), invoked before `getCapabilities()` and before rendering
(source comment: *"We do this before Helm contacts the upstream server and
builds the capabilities object"*): for each CRD under the chart's `crds/`
directory (never templated — a separate, non-`templates/` location), build
+ create, tolerating `AlreadyExists`; then `KubeClient.Wait` with a fixed
60s timeout (not user-configurable) and an explicit discovery-cache/REST-
mapper invalidation (`ToDiscoveryClient().Invalidate()`,
`restMapper.(meta.ResettableRESTMapper).Reset()`) so later
rendering/capability code can see the new CRD's types immediately. Skipped
under `DryRun` with only a logged warning. Neither rollback nor uninstall
has an equivalent step — CRDs installed this way are, per **[DOCS]**
helm.sh/docs, permanent unless a human deletes them; `pkg/action/hooks.go`
separately hard-codes that hook-driven deletion never targets
`CustomResourceDefinition` either, a second independent CRD carve-out.

**Confirmed: install requires the full chart template engine; rollback/
uninstall do not.** `Configuration.renderResources` calls
`engine.New(restConfig).Render(chrt, values)` (`pkg/engine/engine.go:76`)
against every file under `templates/` (recursively over subcharts) — this
is the one call in the entire install path that rollback/uninstall never
make, since those two actions replay an already-rendered manifest read
straight out of release storage (§17's own finding, reaffirmed here from
source, not merely repeated).

**Risk assessment**: storage/apply/hook/status-machine layers — **easy**,
verbatim reuse of what rollback/uninstall already need (§6 primitives
table applies unchanged). CRD-install-first — **medium**, a self-contained
~60-line function, but the discovery-cache/REST-mapper invalidation dance
must be replicated precisely or a chart whose own templates reference its
own just-installed CRD kind will fail to render. Chart loading (local dir/
tarball/OCI/repo-index resolution, dependency resolution) — **medium to
extremely risky** depending on how much of §26-28's scope is assumed away.
Template rendering — **extremely risky**, see §25.

## 23. Upgrade behavior map, and precise reuse of rollback's own machinery

`pkg/action/upgrade.go`, v3.22.0. Read in full for this extension.

```
Upgrade.Run(name, chart, vals)
  └─ Upgrade.RunWithContext(ctx, name, chart, vals)
       ├─ KubeClient.IsReachable()
       ├─ chartutil.ValidateReleaseName(name)
       ├─ Upgrade.prepareUpgrade(name, chart, vals)
       │    ├─ Releases.Last(name)                       [errPending if lastRelease.Info.Status.IsPending() — a real, source-confirmed
       │    │                                              pessimistic-lock-like guard against a concurrent upgrade/rollback/install,
       │    │                                              notably present here even though §7/§12 already established Helm has NO
       │    │                                              locking elsewhere — this one check is the exception, not a refutation]
       │    ├─ pick currentRelease: lastRelease if StatusDeployed, else Releases.Deployed(name) (falls back to lastRelease if
       │    │    Failed/Superseded and no deployed release exists)
       │    ├─ Upgrade.reuseValues(chart, currentRelease, vals)        [VALUE-MERGE LOGIC — genuinely new, see below]
       │    ├─ chartutil.ProcessDependenciesWithMerge(chart, vals)
       │    ├─ revision := lastRelease.Version + 1
       │    ├─ Configuration.getCapabilities()
       │    ├─ chartutil.ToRenderValuesWithSchemaValidation(chart, vals, ReleaseOptions{IsUpgrade:true, Revision:revision}, caps, ...)
       │    ├─ Configuration.renderResources(chart, valuesToRender, "", "", ...)   [SAME function as install — chart re-rendered fresh]
       │    ├─ build upgradedRelease := release.Release{Version:revision, Status:StatusPendingUpgrade, Manifest:..., Hooks:..., Labels:...}
       │    └─ validateManifest(KubeClient, manifestDoc.Bytes(), openAPIValidation)   [KubeClient.Build() dry-parse]
       ├─ Releases.MaxHistory = u.MaxHistory
       └─ Upgrade.performUpgrade(ctx, currentRelease, upgradedRelease)
            ├─ KubeClient.Build(originalRelease.Manifest)      [parse CURRENT, already-rendered, FROM STORAGE → `current`]
            ├─ KubeClient.Build(upgradedRelease.Manifest)      [parse NEW, freshly rendered → `target`]
            ├─ target.Visit(setMetadataVisitor(name, ns, force=true))
            ├─ diff current vs target by objectKey(gvk+ns+name) → toBeCreated                [NEW vs rollback — see below]
            ├─ existingResourceConflict(toBeCreated,...) or requireAdoption(toBeCreated,...)  [same helper as install]
            ├─ toBeUpdated resources folded into `current` for the coming merge
            ├─ [if isDryRun] return upgradedRelease early
            ├─ Releases.Create(upgradedRelease)                [stores new revision N+1 BEFORE applying — mirrors install]
            └─ Upgrade.releasingUpgrade(rChan, upgradedRelease, current, target, originalRelease)  [raced against ctx cancellation]
                   ├─ execHook(upgradedRelease, HookPreUpgrade, timeout)      [SAME execHook as install/rollback]
                   ├─ KubeClient.Update(current, target, Force)      *** IDENTICAL CALL SHAPE TO ROLLBACK'S KubeClient.Update ***
                   ├─ [if Recreate] recreate(cfg, results.Updated)   [SAME function, shared with rollback — see below]
                   ├─ [if Wait] KubeClient.Wait / WaitWithJobs(target, timeout)
                   ├─ execHook(upgradedRelease, HookPostUpgrade, timeout)
                   ├─ originalRelease.Info.Status = StatusSuperseded; recordRelease(originalRelease)
                   └─ upgradedRelease.Info.Status = StatusDeployed
       (on failure anywhere in releasingUpgrade) → Upgrade.failRelease(rel, created, err)
            ├─ rel.Info.Status = StatusFailed; recordRelease(rel)
            ├─ [if CleanupOnFail] KubeClient.Delete(created)
            └─ [if Atomic] — see "`--atomic`" below
  └─ [if !isDryRun] Releases.Update(upgradedRelease)
```

### Precise reuse breakdown — what upgrade shares with rollback verbatim vs. what's new

| Layer | Rollback | Upgrade | Shared? |
|---|---|---|---|
| `KubeClient.Build(manifest)` ×2 (current/target parse) | yes | yes | **Verbatim same call**, `pkg/kube` `Client.Build` |
| `setMetadataVisitor(name, ns, force)` | yes | yes | **Verbatim same function**, `pkg/action/validate.go` |
| `KubeClient.Update(current, target, force)` — the actual reconciliation/patch/apply engine (three-way-merge in v3; SSA-with-options in v4) | yes | yes | **Verbatim same call signature and same underlying merge/apply code path** |
| `execHook` (weight-sort, watch-until-ready, delete-by-policy, log-by-policy) | yes | yes | **Verbatim same function**, `pkg/action/hooks.go` |
| `recreate()` (pod-recreate for `--recreate-pods`) | yes | yes | **Verbatim same function** — source comment: *"captures all the logic for recreating pods for both upgrade and rollback. If we end up refactoring rollback to use upgrade, this can just be made an unexported method on the upgrade action."* |
| Storage `Releases.Create`/`Update`, supersede bookkeeping | yes | yes | Same `pkg/storage` API, same revision-append model |
| Diffing to find brand-new resources (`existingResourceConflict`/`requireAdoption`, `objectKey` set-diff) | **no** — rollback has no "new resource" concept | **yes** | **New in upgrade** |
| Chart template rendering (`engine.Render`) | **no** | **yes** | **New in upgrade** (§25) |
| Values coalescing/reuse (`reuseValues`) | **no** | **yes** | **New in upgrade** (below) |
| `--atomic` rollback-on-failure wrapping | n/a | **yes**, calls `NewRollback(cfg).Run(...)` directly | **New orchestration**, but its payload is literally the Rollback action |
| CRD install-first special casing | n/a | only via `cmd/helm`'s `--install` upgrade path, which constructs an `Install` action | Shared with install, invoked externally, not inside `upgrade.go` itself |

**Bottom line**: upgrade's core reconciliation skeleton (`Build → setMetadataVisitor
→ KubeClient.Update → Wait → hooks → supersede`) is the *same code, called
the same way*, as rollback's — `pkg/kube.Client.Update` is a single shared
implementation, not duplicated. What upgrade adds on top lives entirely in
`prepareUpgrade`: chart template rendering, values coalescing, new-resource
diffing/adoption, and the atomic-rollback orchestration wrapper (which
itself just re-invokes `Rollback`).

### `--force` / `--atomic` / `--reset-values` / `--reuse-values`

- **`--force`**: passed straight through as the third argument to the
  *same* `KubeClient.Update(current, target, force)` call rollback uses —
  not new logic, same flag threaded through the same shared implementation.
  (v4.3.0 splits this concept into `ForceReplace` plus a separate
  `ForceConflicts` for SSA field-manager conflict override.)
- **`--atomic`**: new orchestration inside `Upgrade.failRelease` — on
  failure it fetches full history (`NewHistory(cfg).Run`), filters to
  `StatusSuperseded`/`StatusDeployed` revisions, picks the most recent, and
  constructs+runs a `Rollback` action pinned to that revision. This is new
  *orchestration* code, but the rollback work itself is 100% delegated to
  the existing `Rollback.Run` — "atomic" is a thin wrapper around an
  already-scoped primitive, not a parallel reimplementation. `Install.Atomic`
  is analogous but calls `NewUninstall(cfg).Run` instead (no prior revision
  to roll back to on a fresh install).
- **`--reset-values` / `--reuse-values` / `--reset-then-reuse-values`**
  (`Upgrade.reuseValues`): genuinely new value-merge logic with no
  rollback/uninstall analog — `ResetValues` ignores `current.Config`
  entirely; `ReuseValues` rebuilds the old release's fully-coalesced values
  via `chartutil.CoalesceValues` then merges new explicit values on top via
  `chartutil.CoalesceTables`; `ResetThenReuseValues` does the `CoalesceTables`
  merge only, without the full old-chart-defaults rebuild; the default (no
  flag) copies `current.Config` wholesale if `vals` is empty. Neither
  rollback nor uninstall ever calls these — they replay a stored *manifest*,
  never stored *values*, so there is no values-coalescing surface in those
  two actions at all.

**Risk assessment**: shared layers (storage, apply/patch, hooks, `recreate`,
atomic-wraps-rollback *if* rollback exists as a reusable unit) — **easy**.
New-resource diffing/adoption — **easy/medium**, small self-contained
addition. Values coalescing/`reuseValues`'s four branches — **medium**,
well-specified precedence rules, straightforward but must match exactly.
Chart template rendering — **extremely risky**, identical concern as
install, see §25 (this is the one component upgrade cannot avoid that
rollback/uninstall never needed).

## 24. `helm template` behavior map, and its relationship to install/lint/dry-run

`helm template` is not a separate rendering subsystem — it is `helm
install` with its cluster-touching side effects switched off, sharing the
same `engine.Render`/`Configuration.renderResources` call as install and
upgrade (§22-23).

`cmd/helm/template.go` builds a normal `action.NewInstall(cfg)` client and
sets `client.DryRun = true; client.DryRunOption = "true";
client.ClientOnly = !validate`, then calls the exact same `runInstall`
function `helm install` itself calls, extracting the manifest from the
resulting (never-persisted) `*release.Release` **[SOURCE:
`cmd/helm/template.go`, v3.22.0]**. Inside `pkg/action/install.go`,
`Install.isDryRun()` returns true for `DryRun` or `DryRunOption` ∈
{`"client"`, `"server"`, `"true"`}; an `interactWithRemote` bool is
computed (`server`/`none`/`false` still talk to the API server) and
threaded into the same `renderResources` call install/upgrade use
**[SOURCE: `pkg/action/install.go:isDryRun`/`RunWithContext`, v3.22.0]**.
Under `ClientOnly` (plain `helm template`), `KubeClient` is swapped for
`&kubefake.PrintingKubeClient{Out: io.Discard}` so `lookup`/capability-
discovery calls no-op instead of touching a cluster; under
`--dry-run=server` the real client is kept and `IsReachable()` is
required, so resources go through **actual server-side validation**
before the function returns early (`rel.Info.Description = "Dry run
complete"`) rather than persisting **[SOURCE: `pkg/action/install.go`,
v3.22.0]**. The precise wire-level mechanics of how `--dry-run=server`
attaches to the API-server request inside `pkg/kube/client.go` were **not
confirmed from source in this pass** — flagged in §40, not asserted.
`helm lint`'s core rule instantiates `engine.Engine{LintMode: true}` and
calls the same `Render` entrypoint, with `LintMode` adjusting internal
error tolerance rather than switching to a different code path **[SOURCE:
`pkg/lint/rules/template.go`, v3.22.0]**.

**Implication for a native engine**: an install/upgrade engine that
already implements real chart rendering plus server-side dry-run
validation gets `helm template`, `--dry-run`, and most of `helm lint`'s
rendering step "for free" — same pipeline, flags flipped, not separate
subsystems. This is genuine leverage. What is *not* free is the render
step itself — see §25.

## 25. Template engine portability — the single largest open question

This section investigates whether Helm's Go `text/template`+Sprig chart
renderer is realistically portable to Rust, since install/upgrade/template/
lint/diff-against-a-new-chart-version all depend on it (§22-24, §33) even
though rollback/uninstall never do (§17's original finding, reaffirmed).

### 25.1 Architecture

Helm's renderer builds "an empty parent template and then build[s] up a
list of templates — one for each file," parsed in an order fixed by
`sortTemplates()` (shallower paths before deeply nested ones) **[SOURCE:
`pkg/engine/engine.go:render`/`sortTemplates`, v3.22.0]**. Builtin objects
are assembled inside `recAllTpls()`:
```go
next := map[string]interface{}{
    "Chart": chartMetaData, "Files": newFiles(c.Files),
    "Release": vals["Release"], "Capabilities": vals["Capabilities"],
    "Values": make(chartutil.Values), "Subcharts": subCharts,
}
```
**[SOURCE: `pkg/engine/engine.go:recAllTpls`, v3.22.0]** — the upstream
`vals` map is produced beforehand by `chartutil.ToRenderValues()`
(`pkg/chartutil/values.go`), which composes `Chart` from `chrt.Metadata`,
`Capabilities` from the passed value or `DefaultCapabilities`, `Release`
from `ReleaseOptions` (Name/Namespace/IsUpgrade/IsInstall/Revision, plus a
hardcoded `Service: "Helm"`), and `Values` from `CoalesceValues(chrt,
chrtVals)` **[SOURCE: `pkg/chartutil/values.go:ToRenderValues`, v3.22.0]**.

**FuncMap**: `pkg/engine/funcs.go` starts from `sprig.TxtFuncMap()`,
deliberately deletes `env`/`expandenv`, then adds Helm-specific functions
(`toToml`/`fromToml`/`toYaml`/`fromYaml`/`toJson`/`fromJson`, `required`,
`lookup` — live cluster queries) plus two functions registered as
`"not implemented"` stubs and rebound with real closures later inside
`Engine.Render()` because they need per-render context: `include` and
`tpl` **[SOURCE: `pkg/engine/funcs.go`, v3.22.0]**.

**Subchart rendering**: not a separate pass — `recAllTpls()` recurses into
each dependency and merges its templates into the same flat map executed
together in one pass: `for _, child := range c.Dependencies() {
subCharts[child.Name()] = recAllTpls(child, templates, next) }` **[SOURCE:
`pkg/engine/engine.go:recAllTpls`, v3.22.0]**. Values cascading is
`chartutil.CoalesceValues` (`pkg/chartutil/coalesce.go`) — "values in a
higher level chart always override values in a lower-level dependency
chart"; scalars/arrays replace wholesale, maps merge; each subchart's own
context is scoped to `vals.Table("Values." + c.Name())`, so a subchart
cannot see a sibling's namespace, only its own subtree plus `global`
**[SOURCE: `pkg/chartutil/coalesce.go:CoalesceValues`, v3.22.0]**.
Global-value propagation is a distinct step, `coalesceGlobals()`, described
in-source as an "experimental" reversal of an earlier decision to disallow
nested tables under `global` **[SOURCE: `pkg/chartutil/coalesce.go`,
v3.22.0]**. A second variant, `MergeValues`, intentionally preserves `nil`
keys `CoalesceValues` strips, because early-action processing (e.g.
`--dry-run` diffing) needs nulls retained — a second, distinct merge
algorithm, not a alias.

### 25.2 Sprig's size

Sprig (`github.com/Masterminds/sprig`) registers on the order of
**100-170+ functions** across ~13 categories fetched from `functions.go`
(v3.2.3): date/time (~14), string manipulation (~28), math (~17), list ops
(~19), dict/map ops (~15), JSON (~8), b64/b32 encoding (~4), cryptography
(~10, incl. `genSelfSignedCert`, `derivePassword`, `encryptAES`), regex
(~11, incl. `must*` variants), semver (2), reflection (~6), UUID, URL
parsing, plus a large family of `must*` variants returning errors instead
of panicking **[SOURCE: `github.com/Masterminds/sprig` v3.2.3,
`functions.go`]**. Helm's own docs describe this more conservatively as
"over 60 available functions" **[DOCS: helm.sh/docs/chart_template_guide/
function_list/]**, but the full registered surface (all `must*`/crypto/
network helpers included) is materially larger — a non-trivial function
library, not a handful of string helpers.

### 25.3 Rust templating ecosystem — none are syntax-compatible

None of the mainstream Rust templating crates are syntax- or semantics-
compatible with Go's `text/template`:

| Crate | Syntax basis | Go-template compatible? | Sprig-equivalent? | Maintenance (Sept 2026) |
|---|---|---|---|---|
| **tera** | Jinja2/Django-inspired, explicitly *not* a Jinja2 clone | No — `{% for %}`/`{% if %}` block syntax, not `{{range}}`/`{{if}}`/pipe-only grammar; no documented `{{- -}}`-equivalent whitespace-trim semantics | No native Sprig port; own filter/function set | Active — v2.4.0, Sept 2026; ~2.29M downloads/month |
| **minijinja** | Jinja2 (Python) syntax | No — same block-syntax mismatch | No Sprig equivalent; own filters, feature-gated | Active — v2.24.0, Aug 2026 |
| **handlebars-rust** | Handlebars/Mustache (`{{#each}}...{{/each}}`) | No — different block-helper model entirely | No Sprig equivalent; own helper model | Active — v6.4.4, Aug 2026; ~4.6M downloads/month |

All three are mature and well-maintained, but adopting any for chart
rendering means either transpiling every chart's templates into that
engine's grammar (impractical/lossy at scale, especially for third-party
charts nobody controls) or writing a Go-template-syntax frontend and
Sprig-equivalent backend from scratch and using one of these crates, if at
all, only as an internal implementation detail. The syntax gap
(`{{- -}}` whitespace-trim rules, pipeline chaining semantics where "the
result of each command is passed as the last argument of the following
command" **[DOCS: pkg.go.dev/text/template]**, `text/template`'s specific
dot-scoping through `with`/`range`) is too fundamental to bridge with a
filter shim.

### 25.4 Existing Rust attempts at Go-template/Sprig compatibility

Contrary to a plausible assumption of "nothing exists," several prior-art
Rust projects specifically target Go `text/template` (and, more narrowly,
Sprig) compatibility — none target Helm specifically, and none are
production-grade for that purpose:

- **`gtmpl-rust`** (`github.com/fiji-flo/gtmpl-rust`) — explicitly framed
  around Kubernetes/Docker/Helm-adjacent tooling, self-described as "not
  perfect, yet" (missing complex numbers, `html`/`js` escaping, unstable
  `printf`). **Last push 2022-03-30** — over 4 years stale as of this
  research, not archived but effectively dormant. No Sprig support.
- **`gotmpl`** (`github.com/phsym/gotmpl-rs`, docs.rs) — more recent
  (latest `v0.6.1`, 2026-07-13), actively-CI'd, claims "full template
  syntax including pipelines, control flow, custom functions, template
  composition, and whitespace trimming." Documented gaps: **no struct-
  field access or method calls** (no Rust equivalent of Go's
  `reflect`-based dot-access — a real gap for Helm, since chart authors
  routinely do `.Values.foo.bar`, `range $k, $v := .Values.list` over
  arbitrary nested structures, needing a dynamic value-tree walker), no
  complex numbers, differing NaN-comparison/UTF-8-slicing behavior. Does
  **not** implement Sprig — callers must supply their own func map.
- **`go-template`**/**`gotpl`** — `go-template` (docs.rs) appears to have
  had its last release ~4 years prior to this research and looks
  abandoned; `gotpl` instead wraps the real Go runtime via FFI/cgo for
  perfect fidelity at the cost of embedding an actual Go runtime — defeats
  most of the point of "native Rust."
- **`lithos-gotmpl-rs`** + companion **`lithos-sprig`** (crates.io,
  `v0.1.0`, 2025-11-01, marked unstable) — the closest thing to a
  deliberate "Sprig for Rust" effort found. Explicitly scoped as covering
  only "the subset of Go templates that downstream tooling depends on":
  implements flow control (`default`/`coalesce`/`ternary`/`empty`/`fail`),
  string ops (case conversion, `trim`, `contains`, `replace`, `substr`,
  `wrap`, `indent`), collection ops (`list`/`first`/`last`/`append`/
  `reverse`/`compact`/`uniq`/`dict`/`merge`/`keys`/`values`) — and
  **explicitly omits** randomness, regex, and pluralization helpers; also
  no `else if`/`define`/full keyword-helper support. A project-scoped
  subset, not a drop-in Sprig replacement.
- Searches for "helm template rust", "rust helm chart renderer" turned up
  no project claiming behavioral compatibility with real `helm template`
  output — only IDE wrappers that still shell out to the real `helm`
  binary (e.g. `helmad`). **Finding: no existing Rust project has
  attempted, let alone achieved, a Helm-template-compatible rendering
  engine.**

### 25.5 Honest effort estimate

Not a "swap the templating crate" task, and not a multi-year moonshot —
an uncomfortable, expensive middle:

- **Go-template-compatible grammar/executor**: `gotmpl` (2026) shows this
  is achievable as a standalone crate, but even after multiple iterations
  it still lacks struct-field/method dot-access — a real gap for Helm's
  `.Values`/`.Chart` traversal, needing its own dynamic value-tree walker
  (buildable, but real engineering, not a byproduct of parsing). Order of
  magnitude: **several weeks to 2-3 months**, assuming reuse of `gotmpl`/
  `lithos-gotmpl` as a starting point rather than a blank slate.
- **Sprig-equivalent function library**: the bulk (string/math/list/dict,
  encoding) is mechanical — days, not weeks, using `chrono`/`regex`/
  `base64`/`sha2`/`semver`. The long tail (byte-exact PRNG for crypto
  helpers, Go RE2 vs. Rust `regex` flavor differences, locale-sensitive
  casing) is where `lithos-sprig`'s own decision to skip randomness/regex/
  pluralization to ship v0.1.0 is itself evidence of disproportionate cost.
  Estimate: **3-6 weeks** for a mostly-complete port, plus an open-ended
  tail for perfect fidelity on crypto/regex edge functions.
- **Helm-specific semantics on top**: `include`/`tpl` (recursive self-
  referential invocation with late-bound closures over the *current*
  render context), `required`, `lookup` (needs a live/faked K8s client
  plumbed into the function environment), the full `.Files`/
  `.Capabilities`/`.Release`/`.Chart`/`.Subcharts` object model, named-
  template (`define`/`template`) resolution across a whole chart+subchart
  tree with Helm's exact path-sorting and per-subchart values-scoping/
  `global`-merging rules (§25.1). This is where "renders something
  similar" diverges hardest from "renders identically" — scoping bugs
  only surface on complex real-world umbrella charts. Estimate:
  **4-8 weeks**, correctness provable only via differential testing
  against real charts (Bitnami, ingress-nginx, cert-manager,
  prometheus-community, etc.), diffing engine output against real `helm
  template`.
- **Total for genuine behavioral compatibility on real-world charts**:
  roughly **4-6 months** of focused engineering plus an ongoing tail of
  "found a chart that breaks it" — mirroring the shape of effort visible
  in the prior art (`gtmpl-rust` never finished, went stale after ~2 years
  of intermittent work; `lithos-sprig` shipped v0.1.0 by explicitly
  cutting scope). Compressible with 2-3 engineers on parallel tracks
  (engine core / function library / Helm-semantics-and-differential-
  testing); the irreducible risk is the long tail of undocumented Sprig/
  Go-template edge-case behavior real charts depend on, discovered
  empirically, not by reading a spec.

### 25.6 Conclusion

**Portable, but not cheap, and no one has done it for Helm yet.** The
grammar is conceptually separable and has precedent (`gtmpl-rust`,
`gotmpl`); Sprig's ~100-170 functions are mostly mechanical to port; the
genuinely hard 20% (whitespace-trim edge cases, dot-scoping, `include`/
`tpl` late-binding, subchart values-cascading, crypto/regex edge
functions) only reveals itself against a large real-chart corpus. No
mainstream Rust templating crate is syntax-compatible with Go templates —
they'd need to be bypassed entirely, not adapted. This is a substantial,
isolated subsystem worth building deliberately and early if install/
upgrade/template are ever in scope, with differential testing against
real charts as the acceptance bar — not a drop-in dependency swap, and not
a rounding error against the rest of this document's already-scoped
rollback/uninstall work.

## 26. Pull — classic chart-repo fetch and OCI fetch

All citations `github.com/helm/helm` v3.22.0 unless noted.

### 26.1 Classic repo-index pull

`index.yaml`'s in-memory model, `IndexFile` (`pkg/repo/index.go`):
`APIVersion`, `Generated`, `Entries map[string]ChartVersions` (chart name →
versions, each a `chart.Metadata` plus `URLs`/`Created`/`Removed`/
`Digest`), `PublicKeys`, `Annotations`. Key functions: `LoadIndexFile`/
`loadIndex` (parse + "minimal validity checking," auto-detects JSON vs.
YAML), `MustAdd`, `Merge` (merge by name+version, existing records win on
conflict), `Get`/`Has` (empty version resolves to latest stable),
`SortEntries` (descending version sort), `IndexDirectory` (rebuilds an
index by scanning `*.tgz`, backs `helm repo index`) **[SOURCE:
`pkg/repo/index.go`]**. A flat, non-content-addressed manifest: one YAML
document, no per-chart cryptographic root of trust beyond the optional
`Digest` field and the separate provenance mechanism below — just plain
HTTP(S) URLs per chart version.

Download flow, `ChartDownloader` (`pkg/downloader/chart_downloader.go`):
`ResolveChartVersion` distinguishes full URL / `oci://` (delegated
entirely to `pkg/registry`, §26.2) / `repo/chart` shorthand / local path,
resolving `repo/chart` via the repo's cached `index.yaml` + `i.Get(name,
version)`. `DownloadTo` fetches tarball bytes through a pluggable
`getter.Getter` (HTTP/HTTPS backends), writes atomically, and gates
provenance handling on a `VerificationStrategy` enum (`VerifyNever`/
`VerifyIfPossible`/`VerifyAlways`/`VerifyLater`) **[SOURCE:
`pkg/downloader/chart_downloader.go:ResolveChartVersion`/`DownloadTo`]**.

Provenance/PGP (`pkg/provenance/sign.go`): a `.prov` file is a PGP
clearsigned message wrapping a two-part YAML doc (chart metadata + a
`SumCollection{Files: map[filename]"sha256:<hex>", Images: []string}`).
`Verify(chartpath, sigpath)` decodes the clearsign block, validates the
signature against a loaded keyring (standard OpenPGP trust check, no
custom trust model), and recomputes/compares the tarball's SHA-256 against
the recorded digest **[SOURCE: `pkg/provenance/sign.go:Verify`]**. Small
and self-contained: PGP clearsign + SHA-256, no chain-of-trust
infrastructure beyond "does this signature verify against a local
keyring." A Rust port needs an OpenPGP crate with clearsign support (e.g.
`sequoia-openpgp`, `pgp`) — a well-trodden but non-trivial format (RFC
4880 clearsign framing, dash-escaping, hash-algorithm armor headers).

### 26.2 OCI pull (and push, §27)

Helm treats an OCI-registry chart as a small OCI artifact: one manifest,
one config blob (chart metadata as JSON), one content layer (the `.tgz`),
optionally one provenance layer. Confirmed media types (`pkg/registry/
constants.go`):
```
ConfigMediaType           = "application/vnd.cncf.helm.config.v1+json"
ChartLayerMediaType       = "application/vnd.cncf.helm.chart.content.v1.tar+gzip"
ProvLayerMediaType        = "application/vnd.cncf.helm.chart.provenance.v1.prov"
LegacyChartLayerMediaType = "application/tar+gzip"   // pre-standardization, still accepted on pull
```
**[SOURCE: `pkg/registry/constants.go`]**

`Client.Pull(ref, options...)` (`pkg/registry/client.go`): parses/
normalizes the reference (handles Helm's underscore↔`+` conversion for
OCI-tag-illegal semver build metadata, e.g. `1.0.0+build` → `1.0.0_build`),
builds an in-memory oras content store plus a media-type allow-list, then
**`oras.Copy()` performs the actual registry manifest+layer fetch** — i.e.
Helm delegates OCI Distribution protocol mechanics entirely to **oras-go**,
it does not hand-roll registry HTTP calls itself. Validates the manifest's
descriptor set, extracts config (→ `chart.Metadata`), chart content layer
(current or legacy media type), prov layer if requested **[SOURCE:
`pkg/registry/client.go:Pull`]**.

## 27. Push — OCI, the inverse of pull

`Client.Push(data, ref, options...)` (`pkg/registry/client.go`): parses/
validates the reference; extracts `chart.Metadata` by unpacking `Chart.yaml`
from the raw tarball bytes in memory (the config blob is a *re-derived*
JSON of the metadata, not the raw tarball); in strict mode validates the
reference's name/tag against `Chart.yaml`'s own name/version;
`oras.PushBytes()` pushes the `.tgz` tagged `ChartLayerMediaType`; marshals
metadata to JSON, pushes it tagged `ConfigMediaType`; pushes a provenance
blob tagged `ProvLayerMediaType` if supplied; sorts layers by digest
(deterministic manifest output); builds OCI annotations
(`generateChartOCIAnnotations`, §27.1); `tagManifest()` assembles the OCI
Image Manifest; `oras.ExtendedCopy()` uploads manifest + all blobs
**[SOURCE: `pkg/registry/client.go:Push`]**.

### 27.1 Helm's annotation convention is standard OCI, not custom

`generateChartOCIAnnotations` (`pkg/registry/util.go`) maps chart metadata
onto **standard OCI annotation keys** — `org.opencontainers.image.title`,
`.description`, `.version`, `.url`, `.created`, `.source`, `.authors` —
not a Helm-invented namespace. `generateOCIAnnotations` merges chart-
provided custom annotations on top but protects `AnnotationVersion`/
`AnnotationTitle` from being overridden (`immutableOciAnnotations`) — the
only Helm-invented policy here is "these two keys are immutable" **[SOURCE:
`pkg/registry/util.go`]**. Reference-parsing/normalization (the
underscore/`+` conversion) was confirmed to happen inside `Pull`/`Push` but
its exact file/function location within `pkg/registry` was not pinned down
in this pass — flagged in §40.

## 28. OCI as a general mechanism — mostly generic, one real seam of custom logic

Honest assessment: **mostly "charts are just OCI artifacts with two/three
media types," with one real piece of custom logic a generic OCI client
will not give you for free.**

Pure generic-OCI (a Rust OCI-distribution-spec crate covers this
completely): manifest/blob push/pull, tag listing, catalog, auth
(basic/bearer), chunked/resumable upload — this is exactly what a
distribution-spec client implements regardless of payload semantics. The
annotation scheme is stock `org.opencontainers.image.*` — no proprietary
namespace to reverse-engineer. The media types are just string constants
on blob descriptors — trivial to set on push, filter on pull.

Genuinely Helm-specific, needed on top of a generic OCI client:
1. **Chart-tarball → config-blob derivation** — the config blob is a
   fresh JSON marshal of `chart.Metadata` extracted by unpacking
   `Chart.yaml` from the tarball at push time, not the tarball itself. A
   native engine needs its own tar+gzip reader and `Chart.yaml`
   deserializer (which it likely needs anyway for the classic-repo path).
2. **Reference validation/normalization** — strict-mode name/version-vs-
   tag matching, and the underscore↔`+` substitution for `+build`-suffixed
   semver (OCI tags disallow `+`). Small, easy-to-get-wrong, must match
   exactly for interop with charts already pushed by real Helm.
3. **Legacy media-type acceptance on pull** (`application/tar+gzip`) — a
   compatibility shim for charts pushed before CNCF media-type
   standardization; not discoverable from the OCI spec alone.
4. **Immutable-annotation policy** — a tiny Helm business rule.
5. **Provenance-over-OCI** — the `.prov` blob is pushed/pulled as a third
   layer; the PGP verification logic itself (§26.1) is identical whether
   the chart came from a classic repo or OCI — shared dependency, not new
   OCI-specific work, but means the OCI path also needs the PGP engine
   wired in.

A generic Rust OCI-distribution-spec crate covers the registry *transport*
(the bulk of the engineering effort — auth flows, chunked upload, manifest
negotiation, digest verification) essentially completely. On top, SAUR-ON
needs a thin, well-scoped "Helm OCI artifact" module — on the order of a
few hundred lines, not a re-implementation of registry protocol
semantics.

### 28.1 Rust OCI crates

- **`oci-client`** (formerly `oci-distribution`; `github.com/oras-project/
  rust-oci-client`, moved from the in-tree `krustlet` crate to a
  standalone ORAS-project-maintained repo) — current `0.18.0`, last
  publish 2026-09-18 (3 days before this research per crates.io), 184
  stars, 30 open issues, not archived, `pushed_at` 2026-09-21. Supports
  (per docs.rs `Client` method list): manifest pull/push/multi-platform
  resolution, blob pull/push including resumable partial pull and chunked
  push with on-the-fly SHA-256, `blob_exists`, cross-repo blob mount, tag
  listing, catalog, full OAuth2/bearer-token auth per the distribution
  spec **[DOCS: crates.io, docs.rs]**. The most complete and actively
  maintained generic OCI-distribution client found; descends from the
  same krustlet lineage and is now under the same `oras-project` GitHub
  org as the `oras-go` library Helm itself uses. A reasonable production
  candidate; I could not independently verify broad production adoption
  beyond WASM-runtime contexts (krustlet) — flagged **UNVERIFIED** for
  that specific claim.
- **`oci-spec`** (`github.com/youki-dev/oci-spec-rs`) — `0.10.0`, last
  updated 2026-05-27. **Types/struct definitions only** for the OCI
  Image/Runtime/Distribution specs (used by the `youki` container
  runtime) — not a registry client, would sit under a transport layer as
  a schema crate, not replace one.
- **`ocipkg`** (`github.com/termoshtt/ocipkg`) — `0.4.0`, last updated
  2025-11-17 (~10 months stale relative to `oci-client`). Targets
  distributing developer-compiled binaries via OCI, not a general
  registry client; some of its own CLI docs are marked "TBW." Not a fit
  for a general Helm-OCI transport layer.
- **`helm2oci`** (crates.io) — a small utility converting Helm chart
  tarballs to OCI layout, surfaced by search but not independently
  fetched/inspected — flagged **UNVERIFIED**, worth a follow-up look
  specifically for §28 point 1 (tarball→OCI conversion) if this work is
  picked up.

### 28.2 Risk assessment

(a) Classic repo-index pull + provenance — **easy**: flat YAML, plain
HTTP(S) GET, mature Rust OpenPGP crates exist for the one fiddly piece
(clearsign verification), no Helm-invented cryptography. (b) OCI pull/push
via `oci-client` — **easy/medium**; residual risk is validating
`oci-client`'s auth-flow edge cases against real-world registries (Docker
Hub, GHCR, ACR/ECR token quirks) match `oras-go`/Helm's behavior — an
integration-test-matrix task, not a research gap. From scratch instead of
a crate — **extremely risky**, a poor use of engineering time given a
maintained, feature-complete, days-old-at-time-of-writing crate exists.
(c) Helm-specific OCI metadata logic on top — **easy**, a few hundred
lines, well-specified, no protocol-level subtlety; main risk is silent
interop breakage if the `+`/`_` tag normalization or legacy-media-type
fallback is missed (recommend explicit conformance tests against charts
pushed by the real `helm` binary).

## 29. `helm test`

**File**: `pkg/action/release_testing.go` (not `test.go` — that path does
not exist in `pkg/action`; CLI wiring is `cmd/helm/release_testing.go`),
v3.22.0. Runs a chart's `helm.sh/hook: test`-annotated Pods/Jobs, reports
pass/fail.

**Overlap with the existing hook engine (nothing new)**: `ReleaseTesting.
Run()` does not reimplement any hook orchestration — it calls the exact
same `execHook` install/upgrade/rollback/uninstall already use, just with
event `release.HookTest`: `r.cfg.execHook(rel, release.HookTest,
r.Timeout)` **[SOURCE: `pkg/action/release_testing.go:Run`]**. Same
weight-ordering (`GetPodLogs` reuses the identical `sort.Stable
(hookByWeight(...))` sorter from `hooks.go`) — there is **no distinct
weight-ordering semantics for test hooks**; `helm.sh/hook-weight` means
the same thing regardless of event. Same watch-until-ready, same
delete-policy cleanup, same failure-path log capture — all inherited for
free because it's literally the same call.

**Genuinely new**: (1) name-based `--filter name=foo`/`!name=bar`
filtering, partitioning `rel.Hooks` before calling `execHook` and splicing
skipped hooks back in afterward — no analog in install/upgrade/rollback
hook execution **[SOURCE: `pkg/action/release_testing.go:Run`]**. (2)
`--logs`: a *separate* method, `GetPodLogs(out, rel)`, not part of
`execHook`'s own failure-log capture — runs unconditionally after the test
completes (pass or fail), re-applies the same filters, streams logs
directly via `CoreV1().Pods(ns).GetLogs(...)` for every hook whose
`Events` includes `HookTest` **[SOURCE: `pkg/action/release_testing.go:
GetPodLogs`]**. (3) Result aggregation: no bespoke aggregator beyond
`execHook`'s own error propagation; `rel.Hooks` (filtered+unfiltered) is
written back either way so outcomes persist in history.

**Risk/effort**: **easy**, conditional on the hook engine already
existing — ~90% shared (execHook, weight sort, delete policy,
watch-until-ready); the new surface is a thin filter/log-dump wrapper.

## 30. `helm repo` management — honest in-scope assessment

Packages `pkg/repo/*` (manage `repositories.yaml` and cached `index.yaml`
files), CLI `cmd/helm/repo_*.go`. `repo add` registers a remote repo
URL+credentials locally; `repo update` fetches/caches each configured
repo's `index.yaml`; `repo index` is chart-*authoring* tooling that
generates an `index.yaml` for a directory of packaged charts — irrelevant
to a console operating a cluster **[DOCS: helm.sh/docs/helm/helm_repo/]**.
None of this touches a live cluster or a release; it is local-filesystem-
and-HTTP bookkeeping for discovering what charts are *installable*.

**Assessment**: reasoning from what SAUR-ON's actually-scoped operations
need — rollback reads a *previously stored* manifest/values from
in-cluster release history, never touches `Chart.yaml`/a repo/`index.yaml`
(§4-5). Uninstall only needs the stored manifest to know what to delete —
same, no repo dependency. Upgrade is the one place repo management
*could* matter, but only for "upgrade to a chart version I haven't
fetched yet" — itself a chart-*acquisition* problem, not an *operations*
problem; if SAUR-ON's upgrade flow instead takes a chart the user already
has locally, or re-renders from the chart already recorded in release
storage the way Helm's own three-way-merge diffing does, repo management
is never invoked. **Conclusion: `repo add/update/index` is a chart-
discovery/authoring/publishing concern, fundamentally outside SAUR-ON's
mission** (operating already-installed things, not answering "what chart
versions exist and where do I get them"). It would only become relevant
if scope later expands to net-new installs from an unfetched chart — not
the case today.

**Risk/effort**: not worth building under current scope. **Shared vs.
new**: zero overlap with the apply/hook engine — an entirely independent
subsystem the stated mission doesn't need.

## 31. Dependency management — same honest assessment

`pkg/downloader/manager.go`'s `Manager` type ("handles the lifecycle of
fetching, resolving, and storing dependencies"), invoked by thin
`pkg/action/dependency.go` wrappers. `Manager.Update()` reads a chart's
`Chart.yaml` dependency declarations, resolves each against cached repo
indexes (`resolve`/`findChartURL` — depends on §30's `pkg/repo` cache),
downloads into `charts/`, writes `Chart.lock` pinning exact versions and
digests. `Manager.Build()` rebuilds `charts/` from an existing `Chart.lock`
deterministically, without re-resolving versions **[SOURCE:
`pkg/downloader/manager.go` doc comments on `Update`/`Build`/
`downloadAll`/`writeLock`]**.

**Assessment**: unambiguously a chart-*authoring/packaging* concern —
exists to assemble a chart's own `charts/` subchart tree before that
chart is ever installed. By the time a release exists in the cluster,
whatever installed it has already flattened all dependencies into the
stored release manifest — rollback, uninstall, and diff-preview against
an existing release all read that already-resolved manifest; none re-runs
dependency resolution. Same conclusion as §30, for the same underlying
reason, and it transitively depends on §30 (also out of scope) —
compounding the case against building it.

**Risk/effort**: not worth building — no code path in rollback/uninstall/
upgrade-of-an-existing-release ever calls into this. **Shared vs. new**:
zero overlap with the apply/hook engine; entirely separate, and its only
consumer (`helm install`/`helm package` of a chart not yet materialized)
is outside the stated mission.

## 32. Plugin system — out of scope, briefly justified

`pkg/plugin/plugin.go`'s `Plugin.PrepareCommand`/`PrepareCommands` resolve
a plugin's declared `command`/`platformCommand` into an argv executed via
`exec.Command` — an external, out-of-process executable the `helm` binary
shells out to, extending its command surface (e.g. `helm diff`, §33;
`helm secrets`). Helm's own doc comment: "the command is not executed in
a shell. To do so, we suggest pointing the command to a shell script"
**[SOURCE: `pkg/plugin/plugin.go`]**.

Out of scope for a simple, structural reason: SAUR-ON is a native Rust
engine with an explicit no-shell-out policy — the entire point of a
native Helm engine is to avoid depending on external processes for
correctness and portability. A plugin system whose purpose is "shell out
to an arbitrary external executable" is the opposite of that design goal.
Adopting it would mean either bundling/depending on the `helm` binary
just to host plugins (defeating the native-engine premise) or
reimplementing an exec-based extension point providing no operational
benefit to a console that already controls its own feature surface
directly in Rust. Not worth further research time.

## 33. `helm diff` — third-party plugin, and what it gets for free vs. not

Confirmed third-party, not core Helm: `github.com/databus23/helm-diff`,
not under the `helm` org; its own README states it is "a Helm plugin
giving you a preview of what a `helm upgrade` would change" **[SOURCE:
github.com/databus23/helm-diff README]** — distributed and installed as
an external plugin (§32's mechanism), not compiled into `helm.sh/helm`.

Per its README, it diffs the latest deployed release against a `helm
template`-rendered (or `helm upgrade --dry-run`-rendered, if
`HELM_DIFF_USE_UPGRADE_DRY_RUN=true`) manifest; supports upgrade-preview,
revision-to-revision comparison, rollback-preview, local-chart-directory
diffing; a `--three-way-merge` mode additionally pulls live in-cluster
state to detect drift against both old and new release manifests.

**Assessment — two genuinely separate pieces, mapping differently onto
scoped work**: (1) "Compute the patch, don't apply it" — given two
fully-resolved manifests (old release manifest vs. new candidate) plus
live cluster state, compute a three-way-merge/SSA patch and render it
instead of PATCHing the API server. This is a thin wrapper over the
apply-strategy engine already scoped for rollback/upgrade — same object
diffing, same field-manager/ownership semantics, same conflict detection,
only the last step differs (Apply vs. print). **Free once §8's apply
logic exists — a rollback or already-rendered-upgrade diff-preview needs
zero new code.** (2) The template-render step — producing the "new"
manifest from a chart+values via `helm template`. Rollback never needs
this (its "new" manifest is a stored prior release manifest, already
rendered). Upgrade-diff and install-diff *do* need it — "what would
upgrading to chart X with values Y change" requires rendering chart X
first, pulling in §25's entire template-engine question (and potentially
§26/§30-31 if the chart isn't already local).

**Risk/effort**: rollback/uninstall diff-preview — **easy**, direct
byproduct of the apply-strategy engine. Upgrade/install diff-preview
against a genuinely new chart version — **medium-to-risky**, requires a
correct Helm-compatible template renderer (§25), a substantial, separate
scope item. **Shared vs. new**: rollback-diff is fully shared with the
apply-strategy engine (zero new code); upgrade/install-diff against an
unrendered chart is genuinely new, gated entirely on §25.

## 34. `get` subcommands — confirmed already covered by M9.5, gaps noted

Re-derivation was explicitly out of scope per the brief; this section
confirms the existing M9.5 boundary directly against `src/integrations/
helm.rs` and `src/kube/helm.rs` rather than re-researching Helm's own
`get` behavior from scratch.

`HelmReleaseView` (`src/integrations/helm.rs`) exposes: `name`,
`namespace`, `revision`, `status`, `chart_name`, `chart_version`,
`app_version`, `values` (redacted), `resources: Vec<ManifestResource>`
(identity-only: `api_version`/`kind`/`namespace`/`name` per rendered
document — never the full document body). `kube::helm::read_release`
fetches exactly one Secret — whichever the user has selected in SAUR-ON's
generic object list/watch — re-verifies UID/type fresh, decodes, and
returns the sanitized view; it never queries a *set* of revisions itself.

**Confirmed covered, matching `helm get values`/`helm get manifest`'s
identity-level use case**: chart identity, status, revision number,
redacted values — this is `helm get values` with mandatory redaction
(safer, not identical) and the *identity* half of `helm get manifest`
(what resources this release owns), for whichever single release Secret
the user selected.

**Confirmed NOT covered, explicitly**:
- **`helm get manifest`'s full YAML body.** `ManifestResource` carries
  only `apiVersion`/`kind`/`namespace`/`name` per document — the full
  rendered manifest text (container specs, config data, etc.) is never
  parsed past that identity, by design (M9.5's own "manifest view may
  show resource identity/ownership metadata; it must never show a
  Secret's own data/stringData body" — and by extension never retains the
  full document text for *any* kind, not just Secret-kind documents).
- **`helm get hooks`.** `HelmReleaseView` has no `hooks` field at all —
  the decoded release record's `hooks` array is read by `decode_release`
  (needed structurally to decode the record) but never surfaced. There is
  no hook-listing capability today.
- **`helm get notes`.** Deliberately, permanently excluded — M9.5's own
  documented decision (freeform NOTES.txt prose has no structured key to
  redact by; see the original document's §11/M9_ACCEPTANCE.md's own
  write-up). This is not a gap to close later; it is a standing security
  decision.
- **Per-revision `get manifest`/`get values` for revisions other than the
  latest, as a *dedicated* feature.** There is no explicit "pick revision
  N, then get its values/manifest" UI flow. In practice this is *already
  reachable indirectly*: SAUR-ON's generic Secret list/watch already
  surfaces every `sh.helm.release.v1.<name>.v<revision>` Secret as its own
  object (labels visible, body redacted generically as with any Secret);
  a user can select any specific revision's Secret object and invoke the
  `:helm` command on it, which calls `read_release` against *that exact*
  Secret and returns its own sanitized view — so per-revision inspection
  works today via direct object selection, not via a revision-aware
  "history browser." The gap is UX/discoverability (no explicit "show me
  revision 3 of release X"), not a missing read capability.

**Conclusion**: M9.5 is the correct, load-bearing final Helm read
capability for this document's purposes; the only genuine feature gaps
are hooks-listing and full-manifest-body viewing, both of which were
*deliberately* scoped out for the same M9.5 security reasons (manifest
body could contain a Secret's literal content inline as a rendered
document; hooks are themselves manifests with the same property) — not
oversights. Closing them, if ever desired, is a security-review-first
decision exactly like M9.5's own original one, not a research gap this
extension needs to fill further.

## 35. `history` — confirmed trivial on top of the existing read path

`helm history` lists every revision's version/status/updated-time/chart/
app-version/description **[DOCS: helm.sh/docs/helm/helm_history/]**. Per
§7 (original document) and re-confirmed here: revision Secrets are named
`sh.helm.release.v1.<release>.v<revision>` and carry `name`/`owner`/
`status`/`version`/`createdAt`-or-`modifiedAt` as **labels** (not inside
the redacted `data.release` body) — this metadata is visible from a plain
Secret *list*, without decoding any body at all, exactly as SAUR-ON's
existing generic Store/watch pipeline already surfaces for every other
Secret. There is currently no dedicated "history" view assembling these
into a single revision-list UI (confirmed by grepping `src/app/mod.rs` —
the only `history`-named state there is the app's own UI-navigation
history, unrelated to Helm release history), but the underlying data is
already flowing through the generic pipeline today; building a "history"
view would be a thin presentation layer over data the generic list
already has, not a new read/decode capability.

**Nontrivial edge case flagged, not fully resolved**: rollback-of-a-
rollback numbering. Per §4 (original document), a rollback to "v1" from
"v3" produces a **new v4** record mirroring v1's content — it never
resurrects v1's own Secret. A subsequent rollback of *that* rollback
(rolling back v4 to, say, v2) produces v5, and so on — `helm history`
renders every one of these as a distinct row with its own revision
number, `Info.Description` (Helm sets a default description like
"Rollback to N" per rollback — confirmed by the original document's
citation of `rollback.go`'s targetRelease construction, though the exact
default-description string format was not independently re-verified in
this pass, flagged in §40), and status. A naive "history" presentation
that tried to show a *derived* "logical revision this restores" column
(rather than just the flat list Helm itself produces) would need to trace
each rollback's `Info.Description`/manifest-equality back to its origin
revision — Helm's own CLI does not do this derivation either, it just
lists the flat revision sequence. **Recommendation: match Helm's own flat
list exactly (no derived "restores revision N" column) rather than invent
a presentation Helm itself doesn't provide** — this avoids a whole class
of subtle-mismatch bugs for zero lost fidelity, since the flat list *is*
what `helm history` shows.

**Conclusion**: trivial on top of the existing read path; no new
milestone needed distinct from §37/§38's general storage-read reuse — at
most a UI-only addition, not a new engine capability.

## 36. `lint` / `--dry-run` / `template --validate` — mostly out of scope, one real exception

`helm lint`'s core rule instantiates `engine.Engine{LintMode: true}` and
calls the same `Render` entrypoint install/upgrade/template use, with
`LintMode` adjusting internal error tolerance rather than switching code
paths (§24, re-cited: `pkg/lint/rules/template.go`). It is fundamentally
**chart-authoring-time validation** — "is this chart I'm writing well-
formed" — which is irrelevant to SAUR-ON's mission of operating already-
installed releases: by definition, a release SAUR-ON is rollback-ing/
uninstall-ing/upgrading already has a chart that was already installed
once (lint, if it was ever going to fail, already had its chance to do so
at that point, by a different tool). No evidence found that lint has any
role in operating an existing release.

**The stated exception**: `--dry-run`'s *server-side* validation mode
(`--dry-run=server`) IS relevant to SAUR-ON's own operational mutation-
preview philosophy — and per §24, this is **already effectively covered**
by whatever apply/patch logic rollback/upgrade need anyway. Confirmed
from source: under `--dry-run=server`, `Install.isDryRun()` keeps the real
`KubeClient` (unlike `--dry-run=client`'s fake-client substitution),
requires `IsReachable()`, and runs the *same* render→build→apply call
path as a real install/upgrade, just returning early after
(`rel.Info.Description = "Dry run complete"`) instead of persisting
(§24). This means a native engine's own server-side dry-run preview for
rollback/upgrade — which the original document's §15 already recommends
building on `kube-rs`'s dry-run support, matching M7's existing
`dry_run: Option<MutationOutcome>` field — is not a separate capability
to build for Helm specifically; it is the identical mechanism, reused. The
one piece `--dry-run=server`'s exact wire-level mechanics inside
`pkg/kube/client.go` (whether a `metav1`-style dry-run query parameter is
attached, and precisely where) was not independently confirmed from
source in this pass — flagged in §40, not asserted as identical to
kube-rs's own dry-run parameter without that final check.

**Conclusion**: `lint` itself — not worth building, outside the mission.
`--dry-run=server`'s validation mode — already covered by the same
apply/patch logic rollback/upgrade need; no new milestone required beyond
what §18 (original) and §38 (below) already scope.

## 37. Full Helm surface scope recommendation

This section supersedes §17's "Initial scope vs future scope" in *scope*
(the whole command surface, not just rollback/uninstall) while leaving
§17's own rollback/uninstall-specific reasoning intact and correct for
the question it answered — see the forward-pointer note added at the top
of §17.

### Tier 1 — Worth building natively in Rust

Compatibility-critical, and Helm-specific enough that no existing Rust
crate covers it:

- **Release storage, history, status machine, manifest split/sort/
  resource-policy filter** (§6-7, original document) — already the
  foundation; nothing in this extension changes that conclusion.
- **The apply/patch layer — CSA three-way-merge-equivalent and SSA**
  (§8, original document) — the single largest risk item in the *entire*
  surface, not just rollback/uninstall, because install/upgrade/diff-
  preview/`helm test`'s implicit apply step all route through the exact
  same primitive (§22-23, §33). Building this once, correctly, pays for
  rollback, uninstall, upgrade, install's update-path, and diff-preview
  simultaneously.
- **The hook engine** (§9, original document; confirmed unchanged by
  §29's `helm test` research) — needed by install/upgrade/rollback/
  uninstall/test alike; one engine, five consumers.
- **Chart loading + values coalescing** (§22-23) — chart-directory/
  tarball parsing, `Chart.yaml`/`Chart.lock` reading (not *resolving* —
  see Tier 3 on dependency management), `CoalesceValues`/`CoalesceTables`/
  `reuseValues`'s four merge modes. Well-specified, bounded, and Helm-
  specific enough (exact precedence rules) that no generic crate applies.
- **The Go-template + Sprig-compatible chart rendering engine** (§25) —
  the single largest *new* undertaking in this entire extension, and the
  one item where "worth building" is a real commitment, not a foregone
  conclusion: 4-6 months of focused engineering (§25.5), no existing Rust
  crate is a substitute (§25.3-25.4), and it gates install, upgrade,
  `template`, most of `lint`, and upgrade/install-side `diff`-preview
  (§24, §33, §36). It is worth building **only if** install/upgrade/
  template are themselves in scope — if SAUR-ON's mission stays
  rollback/uninstall-only, this entire item drops out (§17's original,
  still-correct finding: rollback/uninstall never need it).
- **CRD-install-first special casing** (§22) — small, self-contained,
  but genuinely Helm-specific (discovery-cache/REST-mapper invalidation
  sequencing) and needed once install is in scope.
- **Classic repo-index provenance/PGP verification** (§26.1) — bounded,
  well-specified (RFC 4880 clearsign + SHA-256), Helm-specific in its
  exact `.prov`/`SumCollection` format even though the underlying crypto
  primitives are off-the-shelf (`sequoia-openpgp`/`pgp`).

### Tier 2 — Worth building, but mostly via existing Rust ecosystem crates

- **OCI pull/push transport** (§26.2, §27-28): use **`oci-client`**
  (`github.com/oras-project/rust-oci-client`, crates.io `oci-client`
  v0.18.0, last published 2026-09-18, actively maintained under the same
  `oras-project` org umbrella Helm's own `oras-go` dependency belongs to)
  for the registry-protocol transport — manifest/blob pull/push, chunked/
  resumable upload, cross-repo blob mount, auth. This is the honest,
  mature choice; reimplementing OCI Distribution-spec transport from
  scratch (§28.2) would be extremely risky and unjustifiable given a
  maintained, feature-complete crate exists. On top of it, build the
  small (few-hundred-line) Helm-specific layer natively: tarball↔config-
  blob marshaling, `+`/`_` tag normalization, legacy-media-type fallback,
  immutable-annotation policy (§28, points 1-4) — this thin layer is
  Tier-1-shaped work sitting on a Tier-2 dependency, not itself something
  a crate provides.
- **Classic repo-index `index.yaml` parsing** (§26.1) itself is simple
  enough (`serde_yaml` + a semver crate) that it barely counts as "via a
  crate" rather than "trivial to hand-write" — noted here for
  completeness since it's adjacent to the OCI-crate decision, but it does
  not carry the same "don't reimplement a protocol" risk pull/push do.

### Tier 3 — Not worth building, explicitly out of SAUR-ON's mission

- **`helm repo add/update/index`** (§30) — chart-discovery/publishing
  concern; no rollback/uninstall/upgrade-of-an-existing-release code path
  needs it. Different persona (someone finding/publishing charts) than a
  Kubernetes *operations* console (someone operating already-running
  things).
- **Dependency management** (`helm dependency update/build`, `Chart.lock`
  resolution, §31) — chart-authoring/packaging concern; by the time a
  release exists in-cluster its dependencies are already flattened into
  the stored manifest SAUR-ON reads. Depends on §30 (also out of scope),
  compounding the case.
- **Plugin system** (§32) — structurally incompatible with a native,
  no-shell-out Rust engine; adopting it would mean depending on the very
  `helm` binary this project exists to avoid depending on.
- **`helm lint`** (§36) — chart-authoring-time validation; irrelevant to
  operating an already-installed release. (Its one operationally-relevant
  cousin, `--dry-run=server`, is not this item — see Tier 1's apply layer,
  which already covers it for free.)

### Items that are neither "build" nor "don't build" but "already done" or "thin layer on top of Tier 1"

- **`get` subcommands** (§34) — already covered by M9.5's
  `HelmReleaseView`/`read_release`, confirmed not re-derived; the only
  genuine gaps (hooks-listing, full-manifest-body viewing) were
  *deliberately* excluded for the same security reasons M9.5 already
  documented, not overlooked.
- **`history`** (§35) — trivial UI-only presentation layer over data the
  generic Secret-list pipeline already surfaces; no new engine capability,
  no new milestone.
- **`helm test`** (§29) — a thin filter/log-dump wrapper around the
  already-scoped hook engine (Tier 1); ~90% free once hooks exist.
- **`helm diff`'s rollback-preview case** (§33) — fully free once the
  Tier-1 apply layer exists (compute-the-patch-don't-apply-it is a mode
  switch, not new code); its upgrade/install-against-a-new-chart case is
  gated entirely on the Tier-1 template engine and inherits that item's
  cost, not a separate line item.

## 38. Revised milestone sequence — full surface

Builds on, and does not replace, §18's original MH0-MH9 (rollback/
uninstall). This sequence shows where the rest of the surface slots in
*if and only if* Tier 1's larger items (chart rendering, install/upgrade)
are ever taken on — nothing here is scoped as committed, only as ordered
*if* undertaken.

| Milestone | Scope | Depends on | Non-goals | Stop condition |
|---|---|---|---|---|
| **MH0-MH9** | Rollback/uninstall, exactly as §18 (original) | — | — | Unchanged from §18 |
| **MH10** | Chart loading + values coalescing (`Chart.yaml`/`Chart.lock` *reading*, `CoalesceValues`/`CoalesceTables`/`reuseValues`) | MH1 (storage) | Dependency *resolution* (Tier 3) — charts are assumed already-fetched/vendored at this milestone | A hand-fixtured chart+values pair coalesces identically to `helm template`'s own internal coalescing, verified via a values-only differential test (no rendering yet) |
| **MH11** | Go-template + Sprig-compatible rendering engine (§25) | MH10 | No install/upgrade wiring yet — this is the standalone renderer, testable in isolation against `helm template`'s own output | Differential test: rendering a corpus of real charts (Bitnami, ingress-nginx, cert-manager, prometheus-community, at minimum) produces byte-identical (or documented, justified diff) manifests to real `helm template` |
| **MH12** | Native `template` action (client-side only — no cluster calls) | MH11 | No `--dry-run=server`, no install | `helm template` differential parity across the MH11 corpus, exposed as SAUR-ON's own render-only preview |
| **MH13** | CRD-install-first special casing + native `install` | MH7 (SSA), MH10-12 | No repo/OCI fetch yet — charts assumed pre-fetched/local at this milestone | Differential test: installing the MH11 corpus via the native engine, then operating on the result with the real `helm` CLI (`helm status`/`upgrade`/`uninstall`), matches real-`helm`-installed behavior |
| **MH14** | Native `upgrade` (reuses MH3/MH7's apply layer verbatim per §23's reuse table; adds new-resource diffing/adoption, `reuseValues`, atomic-wraps-MH5-rollback) | MH5 (rollback), MH11-13 | — | Fixture matrix extended with upgrade-specific cases (`--force`/`--atomic`/each `reuseValues` mode); atomic-failure path verified to correctly re-invoke MH5's rollback, not a reimplementation |
| **MH15** | Classic repo-index pull + provenance/PGP verification (§26.1) | — (independent of MH10-14; only needed if net-new-chart-acquisition is ever in scope) | No repo *management* (add/update/list, Tier 3) — this is fetch-and-verify only, given a URL | A chart fetched+verified natively decodes/hashes identically to what `helm pull` would produce for the same URL |
| **MH16** | OCI pull/push via `oci-client` + the thin Helm-specific artifact layer (§28) | Independent of MH10-15 | No generic OCI-registry-browsing UI — chart-artifact fetch/push only | A chart pushed by the native engine round-trips through a real `helm pull oci://...`, and vice versa, byte-identically |
| **MH17** | `helm test` as a hook-engine extension (§29) | MH4 (hooks) | — | Fixtures extended with a `helm.sh/hook: test` chart; native test execution + `--logs`-equivalent output matches real `helm test --logs` |
| **MH18** | Diff-preview: rollback/uninstall case (free extension of MH3/MH7) immediately; upgrade/install-against-new-chart case only after MH11-14 | MH3/MH7 (immediate case); MH11-14 (full case) | Not `helm-diff`-plugin-compatible output formatting — SAUR-ON's own preview UI, not a plugin clone | Rollback-case diff-preview ships as a side effect of MH5's own preview step; upgrade-case ships only once MH14 exists |

**Sequencing answer to the brief's explicit question**: yes, `template`
(MH11-12) must exist before `install` (MH13) can — install's render step
*is* `template`'s render step (§24), not a separate implementation.
`get`/`history` need **no new milestone at all** — both are already done
(M9.5) or a thin UI layer with zero new engine capability (§34-35).

## 39. Updated final decision — FULL HELM SURFACE

This block answers for the full surface researched in this extension. It
does not retract §21/the original FINAL DECISION FORMAT block above,
which remains this document's answer for rollback/uninstall specifically
and is unchanged.

FEASIBILITY (full surface):
Mixed, by tier. Core lifecycle (storage/apply/hooks/status-machine,
already Medium-high per §2) stays Medium-high. Chart rendering (§25) is
the one item whose feasibility is genuinely uncertain in *scope*, not
*existence* — no black box, but a real 4-6-month subsystem with an
open-ended correctness tail, and the first Helm-surface item in this
whole investigation (original + extension) where "feasible" and "cheap"
diverge sharply. OCI (§26-28) is High via an existing, actively-maintained
Rust crate (`oci-client`). Repo/dependency/plugin/lint are not a
feasibility question at all — they are a scope question, answered "no."

RECOMMENDED ENGINE BOUNDARY (full surface):
Unchanged in shape from §21/original: the internal workspace crate
(`crates/helm-engine`, §16) grows two new internal modules if Tier 1's
larger items are ever undertaken — a `chart` module (loading + values
coalescing, MH10) and a `render` module (Sprig/Go-template engine, MH11)
— composed into `install`/`upgrade`/`template` actions the same way
`rollback`/`uninstall` already compose `storage`+`manifest`+`apply`+
`lifecycle` (§15). An `oci` module wraps `oci-client` behind the same
narrow-boundary discipline M9.5 already established for reads. No second
gateway, ever, for any of these — every mutation still routes through the
existing M7 gateway (§12, §15, unchanged).

FIRST SUPPORTED ACTIONS (full surface, unchanged from original — this
extension does not accelerate anything):
Still rollback and uninstall only, per §21's original decision. Nothing
in this extension's findings argues for reordering that — if anything,
§25's discovery that install/upgrade/template all share one large,
not-yet-built rendering dependency argues for *more* patience before
committing to that tier, not less.

MUST-HAVE BEFORE EXPANDING BEYOND ROLLBACK/UNINSTALL:
1. Everything §21's original "MUST-HAVE BEFORE FIRST WRITE" already lists
   (unchanged, and still gates rollback/uninstall specifically).
2. Before MH10-14 (install/upgrade): a written decision on whether the
   4-6-month rendering-engine investment (§25) is worth it *at all* for
   this project's actual roadmap — this is a product/scope decision, not
   a technical unknown; the technical unknowns are already closed enough
   by §25 to make that decision without more research.
3. Before MH15-16 (pull/push): a written decision on which of classic-
   repo-pull and OCI-pull/push (or both, or neither) the project actually
   needs — §26-28 found no *blocking* technical unknown for either, so
   this is purely a "do we need this" scope call.
4. Before MH17 (`helm test`): nothing new — it is a strict subset of
   MH4's hook engine, gated only on MH4 existing.

WHAT WE SHOULD NOT IMPLEMENT (full surface, revised):
Everything §21's original list already excludes (hooks/SSA until their
own milestones, non-Secret drivers, a published crate before differential
proof) — **plus, newly and explicitly from this extension**: `helm repo`
management, dependency management (`Chart.lock` resolution), the plugin
system, and `helm lint` (§37, Tier 3) — none of these four are deferred
pending more research; this extension's own research already concluded
they are out of SAUR-ON's mission, not merely not-yet-scheduled.

BIGGEST NEW TECHNICAL RISK (beyond §19's original list):
The Go-template+Sprig rendering engine (§25) — not because it is
unbuildable (precedent exists: `gtmpl-rust`, `gotmpl`), but because its
correctness bar (byte-compatible rendering of real-world charts, not
toy examples) can only be proven empirically against a large chart
corpus, and the two most relevant prior-art Rust projects
(`gtmpl-rust`, stale since 2022; `lithos-sprig`, v0.1.0 with explicit
scope cuts) are both evidence that "the last 20%" of this problem resists
being finished, not just started.

BIGGEST NEW COMPATIBILITY RISK:
None beyond what §19's original item 5 (§8) already identifies — install/
upgrade's apply step is the *same* primitive as rollback's (§22-23), so
this extension does not add a new compatibility-risk category, it
confirms the existing one (CSA/SSA regime detection) is load-bearing for
a larger fraction of the surface than the original document scoped.

CRATE RECOMMENDATION (full surface):
Unchanged: internal workspace crate (§16), never published before
differential proof. New, specific addition: **`oci-client`
(`oras-project/rust-oci-client`)** as an external dependency for OCI
transport if/when MH16 is undertaken — the one place in this entire
investigation (original + extension) where "depend on an existing crate
rather than build it" is the clear, evidence-backed answer, given its
active maintenance (published 3 days before this research) and complete
coverage of the OCI Distribution protocol surface Helm itself needs.

ESTIMATED MILESTONE STRUCTURE (full surface):
§18's original MH0→MH9 (rollback/uninstall) is unchanged and remains the
committed near-term path. §38's MH10→MH18 (chart loading → rendering →
template → install → upgrade → pull → OCI → test → diff) is the
*conditional*, longer-term path, ordered so that `template` (MH11-12)
exists before `install` (MH13) can, and so that upgrade (MH14) reuses
rollback (MH5) and the apply layer (MH3/MH7) rather than duplicating
them — none of MH10-18 is committed by this research; each requires its
own explicit go/no-go, per the MUST-HAVE list above.

SHOULD WE BUILD IT? (full surface):
**Split answer, by tier, not a single yes/no** — this is this extension's
central finding and the reason a single verdict would misrepresent the
evidence: **Yes** for the core lifecycle (storage/apply/hooks/status-
machine — Tier 1's foundation, and rollback/uninstall specifically, per
§21's unchanged original verdict) **and** for `helm test` (a near-free
extension of the hook engine) **and** for rollback/uninstall-case
diff-preview (free once the apply layer exists). **Yes, but only via an
existing crate** for OCI pull/push (`oci-client`) — building this from
scratch would be a poor use of engineering time given a mature dependency
exists. **Worth building, but only as a deliberate, separately-scoped
4-6-month investment, not a rounding error** for chart rendering
(Sprig/Go-template) and, downstream of it, install/upgrade/template/
upgrade-case-diff-preview — real, bounded, source-traceable work per
§25's own conclusion, but large enough to warrant its own go/no-go
decision independent of the rollback/uninstall track. **No** for `helm
repo` management, dependency management, the plugin system, and `helm
lint` — these are chart-authoring/publishing-persona concerns this
project's operations-console mission does not need, not deferred
research gaps.

STOP. No implementation has been started. This extension, like the
document it extends, does not authorize starting any milestone (MH0-MH18)
without separate, explicit approval.

## 40. Explicit unknowns from this extension pass

Mirroring §20's original discipline — nothing below was filled in with
assumptions in the body of §22-39; each is flagged here rather than
silently guessed:

1. **`--dry-run=server`'s exact wire-level mechanics inside `pkg/kube/
   client.go`** (§24, §36) — confirmed *that* the real `KubeClient` and
   `IsReachable()` are used and that the render→build→apply path is
   identical to a real install/upgrade, but not confirmed *where/how* a
   `metav1`-style dry-run parameter is attached to the outgoing API-server
   request. Needed before treating this as identical to `kube-rs`'s own
   dry-run parameter without a final direct check.
2. **Exact location of Helm's `+`/`_` OCI-tag-normalization logic within
   `pkg/registry`** (§27) — confirmed it happens inside `Pull`/`Push`, not
   confirmed which specific file/function owns it. Needed before a native
   engine's reference-normalization code can cite an exact upstream
   counterpart rather than "somewhere in pkg/registry."
3. **`oci-client`'s real-world production adoption beyond WASM-runtime
   (krustlet) contexts** (§28.1) — maintenance/completeness were
   confirmed from crates.io/docs.rs directly; broad production-readiness
   beyond that lineage was not independently verified.
4. **`helm2oci` crate's scope/maturity** (§28.1) — surfaced by search,
   not independently fetched/inspected; worth a follow-up look
   specifically for the tarball→OCI-artifact conversion step if MH16 is
   picked up, not treated as confirmed prior art here.
5. **The exact default `Info.Description` string Helm writes for a
   rollback record** (§35) — cited from the original document's own
   `rollback.go` call-graph reading, not independently re-verified
   byte-for-byte in this pass; relevant only to a "history" presentation
   layer that tries to show rollback-specific description text verbatim.
6. **No new live experiments were run against `helm-research-scratch` or
   any disposable namespace in this extension.** Every claim above is
   [SOURCE] (fetched Helm/Rust-ecosystem source or package-registry
   metadata) or [DOCS] — none required a live-cluster experiment to
   verify (unlike the original document's §8/§13, which did), and none
   was fabricated to appear as if one had been run. If a future pass
   wants to validate the OCI-crate choice (§28) or the `--dry-run=server`
   unknown (item 1 above) against a real registry/cluster, that remains
   open, disposable-namespace-scoped work for later, not something this
   pass silently skipped without flagging.

None of the above were filled in with assumptions in §22-39's own body —
every claim there traces to a fetched source file, an official/package-
registry doc, or is explicitly marked otherwise inline. These six items
are this extension's own concrete next-step reading/experiment list,
parallel to §20's role for the original document.
