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
| M7.2 | Preview / server dry-run / confirmation contract | `MutationPreview` (local, no network) is separate from `kube::mutation::preflight` (server `dryRun=All`, journaled `PreflightStarted`/`PreflightResult`) and from `commit` (real write); `Confirmation` from M7.0 binds the exact intent | covered by M7.0's confirmation test + M7.3's dry-run test | 1 (dry-run never commits, journaled distinctly) | n/a yet | none | IMPLEMENTING |
| M7.3 | Single mutation execution gateway | `kube::mutation::commit`: re-evaluates policy at commit time, requires an authorizing confirmation, checks epoch, revalidates UID/resourceVersion via a fresh metadata GET (TOCTOU), journals before sending, issues exactly one bounded/cancellable PATCH or DELETE, classifies the result (`Committed`/`Forbidden`/`NotFound`/`Conflict`/`TargetReplaced`/`Cancelled`/`OutcomeUnknown`/`TransportFailure`), journals the result | n/a (integration-level) | 9 tests: policy-denied zero-write, missing-confirmation zero-write, UID-mismatch zero-write, precommit-journal-failure zero-write, real revalidate+patch+journal, dry-run never commits, 409/403 explicit, epoch-change rejected, cancellation-before-commit zero-write | n/a yet | Create effect is an explicit `Unsupported` bounded limitation (needs a full object body + different identity semantics; M8 scope, not silently half-implemented) | IMPLEMENTING |
| M7.4 | Durable redacted mutation journal | `src/mutation/journal.rs`: JSONL, schema-versioned, bounded field sizes (2048B, 32 reasons), append-only; `Record` has no field capable of holding raw payload/Secret content by construction | 5 unit tests: round-trip order, malformed-line-skipped, missing-file-is-empty, oversized-field-bounded, no-secret-shaped-field | covered via M7.3's journal assertions | n/a yet | none | IMPLEMENTING |
| M7.5 | TUI policy/journal surfaces | `:policy`/`u`: read-only, synchronous, zero network -- renders hypothetical Modify/Delete policy decisions for the selected object via `mutation::view::policy_report`; runtime `PolicyContext.cluster_verified_for_mutation` is always `false` (no in-app mechanism exists to set it), so this view honestly denies everywhere SAURON actually runs. `:mutations`/`m`: bounded (200) read of the local journal file, zero Kubernetes requests | 5 unit tests (`mutation::view`: never-Allow-without-verified-cluster, both-effects-shown, journal-empty/non-empty) + 2 app-level tests (zero tasks spawned by either action) | n/a (no transport) | n/a yet | none | IMPLEMENTING |
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

- 2026-09-18: M7.5 implementing. Added `src/mutation/view.rs` (pure
  rendering, no I/O beyond what the caller already loaded) and wired
  `:policy`/`u` and `:mutations`/`m` (table mode) into `app/mod.rs` as fully
  synchronous actions -- no async task, no network, matching the
  requirement that policy/journal rendering issue zero Kubernetes requests.
  `open_policy_view` builds a `session::Scope` for the selected object from
  the live connection/state (same convention as every other single-object
  action) and evaluates both a hypothetical Modify and Delete through the
  real `policy::evaluate`. Since SAURON has no in-app setting to mark a
  cluster as "verified for mutation" (that verification is deliberately
  kept external, per the stricter development/acceptance safety contract),
  the view will show `UnverifiedCluster` and never `Allow` no matter what
  cluster is connected -- this is correct and expected for M7, which ships
  no mutation workflow. `open_mutations_view` reads the bounded local
  journal (`$XDG_CONFIG_HOME/sauron/mutations.jsonl` or
  `~/.config/sauron/mutations.jsonl`) via `Journal::recent(200)`; a missing
  file reads as empty, never an error. 5 new `mutation::view` unit tests +
  2 app-level tests asserting neither action spawns a task. Full locked
  fmt/check/clippy/test green: 152 unit + 37 fake HTTP. No live evidence
  yet -- next: M7.6 (fake HTTP already covers the executor; remaining work
  is the live proof mutation, full M1-M6 regression, and the soak).

- 2026-09-18: M7.2/M7.3/M7.4 implementing. Added `src/mutation/journal.rs`
  (JSONL, schema-versioned, bounded, malformed-line-tolerant) and
  `src/kube/mutation.rs` (the single executor: `preflight`/`commit`).
  `commit` re-evaluates policy at commit time rather than trusting an
  earlier preview decision, requires a confirmation that exactly authorizes
  the intent when policy demands one, checks the caller's epoch, then
  revalidates the target's live UID/resourceVersion via a fresh
  metadata-only GET immediately before mutating (reusing
  `kube::relationships::read_bounded`, now `pub(crate)`). Journal writes
  happen before the real request (fail-closed on write failure) and after
  it (a post-commit journal failure is reported as
  `CommittedButJournalIncomplete`, never silently lost). Transport
  ambiguity after a request is dispatched (connection drop, timeout waiting
  for a response, a stream error while draining the body) is classified as
  `OutcomeUnknown`, never `Failed` or `Success` without proof; cancellation
  *before* dispatch is `Cancelled`, kept structurally distinct from
  `TargetReplaced` (epoch mismatch). Added `http = "1"` as a direct
  dependency (previously only transitive via kube) since the executor needs
  to name `http::Request` explicitly, unlike the report/relationships code
  which never spells out the type. Added `PartialEq`/`Eq` to
  `kube::discovery::Resource` (needed for M7.0's model equality checks).
  9 new fake-HTTP tests, all passing on first real run: policy-denial,
  missing-confirmation, UID-mismatch, precommit-journal-failure (via an
  unwritable path) all assert zero HTTP writes (the fake server panics if
  called at all); a real revalidate-then-patch-then-journal path; a
  dry-run that is journaled under `PreflightResult`, never `CommitResult`;
  explicit 409/403; an epoch change rejected before any write; cancellation
  before commit. Full locked fmt/check/clippy/test green: 147 unit + 37
  fake HTTP. Create effect intentionally returns `Unsupported` (documented
  bounded limitation, not silently half-implemented) since it needs a full
  object body and different identity semantics -- M8 scope. No live
  evidence yet -- next: M7.5 TUI surfaces (`:policy`/`:mutations`), then
  M7.6 live acceptance/regression/soak.

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
