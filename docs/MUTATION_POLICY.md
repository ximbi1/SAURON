# Mutation policy — M6 relationships are read evidence; M7 is the doorway

M7 is infrastructure, not a mutation feature. It builds the mandatory
pipeline every future mutation (M8) must pass through:

    selected object
        -> canonical target identity (MutationTarget)
        -> mutation intent (MutationIntent)
        -> central policy evaluation (mutation::policy::evaluate)
        -> confirmation contract (Confirmation)
        -> single execution gateway (kube::mutation::preflight/commit)
        -> UID/resourceVersion revalidation
        -> server result
        -> durable, redacted journal (mutation::journal)

M7 ships **no user-facing mutation workflow**. There is no keybinding that
dry-runs, confirms, or commits a real mutation. `:policy` is read-only. The
only mutation this milestone actually performs is the one narrowly-scoped
internal proof action (`tests/mutation_live.rs`), exclusively against the
isolated `kind-sauron-test` fixture, never against production.

## Identity: name is never authorization

`MutationTarget` reuses `app::session::Scope` — the same context/cluster/
resource-id/namespace/name/UID/epoch/request tuple already used by every
owned session (logs, exec, port-forward). A mutation against an existing
object with an empty UID is `TargetIdentityIncomplete` and denied; there is
no "mutate by name, hope the UID matches" path anywhere in the pipeline.

Same-name replacement (an object deleted and recreated with a new UID,
still called the same thing) is rejected as `TargetReplaced`, never quietly
retargeted onto the replacement. This is checked twice: once implicitly by
whatever constructed the intent, and again — the one that actually matters —
immediately before the real mutation request, in `kube::mutation::commit`'s
TOCTOU revalidation (a fresh metadata-only GET).

## Policy: UNKNOWN never means Allow

`mutation::policy::evaluate(context, intent)` is pure and deterministic — no
Kubernetes transport dependency, fully testable in isolation. It evaluates a
fixed set of gates, in order, and accumulates *every* reason that applies
rather than stopping at the first one, so a caller always sees the complete
picture:

1. Global readonly state and the `--readonly` CLI override (a distinct,
   stronger reason: `ReadonlyOverride` vs. `ReadonlyMode`).
2. Cluster/context safety — `PolicyContext.cluster_verified_for_mutation`
   must be positively proven true by the caller. **There is no context-name
   heuristic anywhere in this codebase.** A context literally named
   `kind-sauron-test` does not verify itself; nothing about the intent or
   the connection can set this flag from inside the running application.
   Only `scripts/test-cluster.sh`'s Docker/API loopback identity guard, an
   external harness, ever sets it true, and only for live test/acceptance
   code. **In the shipped application, this flag is never true anywhere,**
   which is why `:policy` denies every hypothetical mutation on every
   cluster, including the isolated test cluster, when reached through the
   actual running binary.
3. Target identity completeness (UID required).
4. Namespace protection: `kube-system`, `kube-public`, `kube-node-lease` by
   default (`PolicyContext::default()`), always deny.
5. Cluster-scoped/privilege-sensitive kind (`Namespace`, `Node`,
   `ClusterRole`, `ClusterRoleBinding`, `CustomResourceDefinition`,
   `APIService`, webhook configurations, `StorageClass`, `Secret`,
   `ServiceAccount`, `Role`, `RoleBinding`, `PersistentVolume`,
   `PersistentVolumeClaim`) — a stronger guardrail regardless of effect.
6. Effect/risk: `Delete` and `MutationRisk::Destructive` always require the
   strongest confirmation tier.

If any hard-deny gate fires, the decision is `Deny` with every applicable
`PolicyReason` attached — never a bare `bool`, never a vague "operation not
allowed" when a precise reason exists. Otherwise the decision is `Allow`,
`RequireConfirmation`, or `RequireStrongerConfirmation`, again with
structured reasons.

This repository's development/acceptance safety contract is deliberately
**stricter** than the generic policy architecture SAURON exposes: a real
product might one day let an operator explicitly, deliberately mark a
cluster as eligible for mutation (with its own strong confirmation UX). This
codebase, as it stands for M7 acceptance, never does — `cluster_verified_for_mutation`
is set to `true` only inside guarded test code that has independently proven
cluster identity via Docker/API introspection, never inside the app itself.

## Confirmation: binds to the exact intent, never reusable

A `Confirmation` authorizes exactly one `MutationIntent`: it compares
`request_id`, the full `Scope` (context, cluster, resource id, namespace,
name, UID, epoch), the `MutationEffect`, and the payload hash. Any change to
any of those — a different object, a same-name replacement, a different
namespace, a different context, a different effect, a different requested
change — invalidates it. Confirmations are session-local values, never
persisted to disk, never reusable across a process restart.

## Dry-run vs. commit: never the same phase

`kube::mutation::preflight` issues a server `dryRun=All` request and
journals it under `PreflightStarted`/`PreflightResult`, structurally
distinct from `commit`'s `CommitStarted`/`CommitResult`. **Dry-run success is
never treated as proof a later commit will succeed** — there is no code path
that promotes a successful preflight into an automatic commit.

## Commit: one gateway, TOCTOU-safe, fail-closed on the journal

`kube::mutation::commit` is the single execution gateway. In order:

1. Re-evaluates policy — never trusts an earlier preview's decision.
2. Requires a `Confirmation` that authorizes the exact intent, when policy
   demands one.
3. Rejects on a caller epoch mismatch or prior cancellation.
4. Revalidates the target's live UID/resourceVersion via a fresh
   metadata-only GET (TOCTOU: preview-time identity ≠ commit-time identity).
5. Journals the attempt **before** the real request. If this write fails,
   the mutation is never sent — fail closed.
6. Issues exactly one bounded, cancellable PATCH/DELETE.
7. Classifies the result and journals it. A journal-write failure *after* a
   real server success is reported as `CommittedButJournalIncomplete`,
   never silently dropped and never reported as if nothing happened.

`Create` intentionally returns `Unsupported` — it needs a full object body
and different identity semantics than Modify/Delete, and building that out
is explicitly M8 scope, not a half-finished M7 feature.

### Outcome semantics

`MutationOutcome` preserves the one distinction this type exists to protect:
*"definitely not committed"* vs. *"commit outcome cannot be proven"*.
`Cancelled` (never dispatched) and `OutcomeUnknown` (dispatched, but the
connection dropped, timed out waiting for a response, or the response body
stream failed before a status code could be trusted) are structurally
different variants. A `409` is `Conflict`, never auto-forced or retried. A
`403`/`404` are explicit `Forbidden`/`NotFound`, never a false success.

## Protected targets are defaults, not universal claims

`PolicyContext::default()`'s protected namespaces and sensitive-kind sets are
this repository's acceptance defaults, not a claim that they cover every
cluster's actual security posture. A real deployment would make these
configurable; M7 does not build that configuration surface (out of scope,
matches "do not overclaim universal safety policy for every cluster").
