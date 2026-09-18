# M7 mutation policy / guardrails / journal acceptance ledger

Baseline: `m6-accepted`. M1-M6 acceptance is preserved, not repeated as a new
baseline audit (M6 final: 128 unit + 28 fake HTTP; 18/18 `accept-m6.py` live
scenarios twice; full M1-M5 regression; 75-minute soak, 1263 cycles, RSS
+0.15%, fds/threads flat, one transient self-recovered timeout).

M7 builds the mandatory infrastructure every future mutation must pass
through. It does NOT implement user-facing mutation workflows (M8).

Governing philosophy (verbatim, must be preserved):
**NO MUTATION WITHOUT POLICY.** **NO MUTATION BY NAME ALONE.**
**UNKNOWN ≠ ALLOWED.** **STALE ≠ CURRENT.**
**DRY-RUN SUCCESS ≠ COMMIT SUCCESS.** **CONFIRMATION ≠ AUTHORIZATION.**
**A SUCCESSFUL HTTP RESPONSE ≠ VERIFIED DESIRED EFFECT.**
**JOURNAL FAILURE BEFORE COMMIT = FAIL CLOSED.**
**POLICY DECISIONS MUST BE EXPLAINABLE.**

Production is read-only. ALL M7 live mutation tests use only the isolated
`kind-sauron-test` cluster via `scripts/test-cluster.sh`'s verified Docker/API
identity guard — never a context-name heuristic, never the default
kubeconfig. No production mutation, ever, including dry-run.

## Slice ledger

| Slice | Contract | Implementation | Unit evidence | Fake HTTP evidence | Live evidence | Real bugs found | Verdict |
| --- | --- | --- | --- | --- | --- | --- | --- |
| M7.0 | Mutation identity/effect/risk model | `src/mutation.rs`: `MutationTarget` reuses `session::Scope` (context/cluster/resource/namespace/name/UID/epoch, the same incarnation-safe convention as logs/exec/forward) + `Resource`; `Confirmation::authorizes` binds request_id/scope/effect/payload hash exactly | 1 unit test (7 assertions: UID replacement, namespace, context, effect, payload, request, resource all invalidate) | n/a (pure model) | n/a yet | none | IMPLEMENTING |
| M7.1 | Central policy engine, hard guardrails | `src/mutation/policy.rs`: pure `evaluate(context, intent)`, fixed deterministic gate order, accumulates every applicable reason (never stops at first), UNKNOWN (unverified cluster) always denies | 12 unit tests: readonly/override, unverified cluster, missing UID, protected namespace, cluster-critical kind, privilege-sensitive kind, destructive delete, routine confirmation, Create-allowed, deterministic repeat | n/a (pure, no transport) | n/a yet | none | IMPLEMENTING |
| M7.2 | Preview / server dry-run / confirmation contract | | | | | | NOT ACCEPTED |
| M7.3 | Single mutation execution gateway | | | | | | NOT ACCEPTED |
| M7.4 | Durable redacted mutation journal | | | | | | NOT ACCEPTED |
| M7.5 | TUI policy/journal surfaces | | | | | | NOT ACCEPTED |
| M7.6 | Combined adversarial acceptance, regressions, soak | | | | | | NOT ACCEPTED |

## Required evidence

M7.0: incarnation-safe target identity (UID required for existing-object
operations; same-name+new-UID is `TargetReplaced`, never silently mutated);
deterministic, non-AI effect (Create/Modify/Delete) and risk
(Routine/Destructive/PrivilegeSensitive/ClusterCritical) classification.

M7.1: readonly denies; `--readonly` override denies; unverified cluster
denies (no context-name heuristic); protected namespace
(kube-system/kube-public/kube-node-lease) denies; cluster-scoped
sensitive-kind stronger guardrail; UNKNOWN never silently Allow; every
decision carries structured `PolicyReason`s, never a bare bool.

M7.2: local preview vs server dry-run vs commit are three distinct states,
never an implicit transition; confirmation binds the exact intent (context,
namespace, GVR/GVK, name, UID, effect, payload hash, epoch) and is
invalidated by any change to those fields; confirmations are session-local,
never persisted.

M7.3: one central executor; policy re-evaluated at commit time, not just
preview time; target re-fetched and UID/resourceVersion revalidated
immediately before commit (TOCTOU); journal written before the real mutation
request, fail-closed on journal-write failure; ambiguous transport outcomes
after send are `OutcomeUnknown`, never reported as `Failed` or `Success`
without proof; no mutation request from render or idle frames; no lock held
across await; owned/cancellable tasks.

M7.4: journal never records Secret data/stringData/tokens/credentials/exec
stdin; redacted summaries and payload hashes only; append-oriented, bounded
record size, explicit schema version; a malformed prior line never crashes
SAURON; precommit journal-write failure blocks the mutation; postcommit
journal-write failure after a real server success is truthfully represented
(`CommittedButJournalIncomplete`), never silently lost or misreported as
failure.

M7.5: `:policy` (read-only, explains what a hypothetical mutation would do)
and `:mutations` (bounded, paginated journal viewer, no sensitive payload);
32x9 safe.

M7.6: no known UI bypass path around the executor; fake HTTP suite covers
denial/replacement/conflict/unknown-outcome zero-write assertions; one
narrowly-scoped internal-only proof mutation (annotation patch on an
`m7-target` ConfigMap in a dedicated `sauron-m7` fixture namespace) proves
the full pipeline live, end to end, exclusively against `kind-sauron-test`;
full M1-M6 regression; 75-minute soak.

## Combined live flows (pending, `scripts/accept-m7.py`)

1. Verified kind-sauron-test → fixture target → policy allows the internal
   proof action.
2. Same action under `--readonly` → denied, zero object change.
3. Protected namespace/resource hypothetical policy → denied before mutation.
4. Preview → dry-run → confirmation → commit → fresh GET verifies effect →
   journal verifies exact UID/action/outcome.
5. Same-name replacement between preview and commit → commit rejected,
   replacement untouched.
6. resourceVersion conflict → explicit `Conflict`, no force/retry.
7. Confirmation generated → context switched → confirmation invalid.
8. Confirmation generated → namespace changed → invalid.
9. Confirmation generated → requested mutation changed → invalid.
10. Cancel during preflight → no commit.
11. RBAC-denied mutation identity/account → `Forbidden`, no false success.
12. Journal precommit write intentionally fails → no mutation.
13. Successful server commit + simulated journal result-write failure →
    commit-happened/journal-incomplete state, never "mutation failed".
14. Network/transport ambiguity after send → `OutcomeUnknown` where
    reproducibly simulatable in the fake transport (not forced live).
15. M4 forward remains functioning during policy/journal navigation.
16. M5 metrics/Explain remain functional.
17. M6 Adjacent/Xray remain functional.
18. 32x9 policy/journal/preview surfaces.
19. Rapid context/namespace/resource churn while preview is in flight →
    stale result discarded, no commit.
20. Quit while preflight/policy task active → task joined/cancelled, exact
    terminal restoration.

Run combined flows twice where practical with a freshly built binary.

## Performance / soak (pending)

Target 75-minute isolated kind soak rotating navigation, policy view, journal
view, previews, dry-runs (fixture only), cancelled previews, Adjacent, Xray,
Explain, metrics, with an M4 forward active. Record duration, cycles,
previews, dry-runs, commits, denials, cancellations, conflicts, unknown
outcomes, journal records, request counts, RSS/fds/threads start-end,
unexpected vs. transient-recovered errors, bound hits. Observations, not
proof of leak-freedom.

## Journal

- 2026-09-18: M7.0/M7.1 implementing. Added `src/mutation.rs` (pure model:
  `MutationTarget`/`MutationEffect`/`MutationRisk`/`MutationIntent`/
  `PolicyReason`/`PolicyDecision`/`PolicyEvaluation`/`ConfirmationRequirement`/
  `Confirmation`/`MutationPreview`/`MutationOutcome`) and
  `src/mutation/policy.rs` (pure `evaluate()`, no transport dependency).
  `MutationTarget` reuses `app::session::Scope` for identity, the same
  convention already used by owned logs/exec/forward sessions, instead of
  inventing a parallel identity type. Added `PartialEq`/`Eq` to
  `kube::discovery::Resource` (needed for intent/confirmation equality in
  tests, harmless elsewhere). 13 new unit tests, full locked
  fmt/check/clippy/test green (142 unit + 28 fake HTTP, unchanged). No
  transport, no live evidence yet -- next: M7.2 preview/confirmation and
  M7.3 executor.

- 2026-09-18: ledger created before implementation. Baseline recorded:
  `m6-accepted`. No cluster access, no mutation code, no acceptance claimed
  yet. Next: M7.0 mutation identity/effect/risk model and unit tests, before
  any real mutation transport exists.
