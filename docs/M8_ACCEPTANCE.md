# M8 guarded mutation workflows acceptance ledger

Baseline: `m7-accepted`. M1-M7 acceptance is preserved, not repeated as a new
baseline audit. M7 final: 152 unit + 37 fake HTTP, fmt/check/clippy clean,
75-minute soak (1071 cycles, zero reconnects, RSS +1.2%), working tree clean.

M8 adds the first real user-facing mutation workflows (Scale, Restart,
Delete, Label, Annotate). Every one of them MUST walk through the exact M7
pipeline (intent → policy → preview → dry-run → confirmation → executor →
journal → verification). No M8 workflow gets its own safety mechanism, and
none may call PATCH/PUT/DELETE/POST against Kubernetes outside
`kube::mutation`'s executor.

Production is read-only: zero writes, zero `dryRun=All` requests, ever.
ALL M8 live writes use only the isolated `kind-sauron-test` cluster via
`scripts/test-cluster.sh`'s verified Docker/API identity guard — never a
context-name heuristic, never the default kubeconfig.

## Slice ledger

| Slice | Contract | Implementation | Unit evidence | Fake HTTP evidence | Live evidence | Interactive evidence | Real bugs found | Verdict |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| M8.0 | Shared workflow shell (preview/dry-run/confirm/commit/verify), reused by every action | `mutation::workflow::{Built,Workflow}`, `Document.workflow`, `"mutation"` keymap mode, `Command::{Scale,Restart,Delete,Label,Annotate}` + `parse()` grammar, `Runtime::{mutation_scope,mutation_policy_context,open_workflow_document,mutation_dry_run,mutation_confirm}`, `mutation::view::workflow_report`. Double-press UX for `RequireStrongerConfirmation`; single press for `RequireConfirmation`/`Allow`. Denied previews still open (visible, not hidden) and confirming them sends zero requests. Distinct `ConnectOptions.mutation_test_cluster_verified` (via `--mutation-test-cluster-verified`, set only by `scripts/test-cluster.sh`/its interactive callers) -- never derived from `readonly` or context name. | 35 tests (16 workflow + 6 view + 7 command grammar/registry + 6 app zero-write/zero-bypass) + 2 readonly/verified-separation tests | 10 tests | `tests/mutation_workflows_live.rs` (all 5 workflows), run twice, both clean | `scripts/smoke-m8.py`, tmux-automated | none | ACCEPTED |
| M8.1 | Scale (Deployment/StatefulSet/ReplicaSet) | `workflow::scale`: exact `spec.replicas` merge patch, UNKNOWN-never-zero current count, unsupported-kind rejection before intent construction; wired to `:scale N` and the shared shell | 4 tests | 1 test (`verify_scale_confirms_observed_desired_replicas`) | live: 1→2→1 round-trip, verified both directions | smoke-m8.py: preview/cancel/dry-run/commit/verify/reset | none | ACCEPTED |
| M8.2 | Rollout restart (Deployment/StatefulSet/DaemonSet) | `workflow::restart`: caller-supplied timestamp reused verbatim in payload/hash, unsupported-kind rejection; wired to `:restart` (timestamp generated once in `Runtime::command`, at intent-build time) | 2 tests | 1 test (`verify_restart_confirms_the_exact_template_annotation_value`) | live: exact `restartedAt` value observed | not separately covered in smoke-m8.py (covered live + fake HTTP; interactive coverage deferred to M8.6's accept-m8.py) | none | ACCEPTED |
| M8.3 | Delete (explicit allowed-kind set only) | `workflow::delete`: allowlist rejection, no payload, `Destructive` risk, always `RequireStrongerConfirmation` (double-press); `kube::mutation::delete_request` now sets a server-side `Preconditions{uid}` (defense in depth alongside the client TOCTOU GET); wired to `:delete` | 2 tests | 1 test (UID precondition) + 3 verify tests (deletion-in-progress/observed-gone/pending) | live: accepted → bounded poll → `ObservedGone`, journaled | smoke-m8.py: double-press strong confirmation, commit+verification both render | none | ACCEPTED |
| M8.4 | Label / annotate (set/remove, one key per intent) | `workflow::label`/`workflow::annotate`: single-key merge patch (`null` = remove), key/value syntax validation, `kubernetes.io/`/`k8s.io/` protected-prefix denial, annotation values exempt from label value charset; wired to `:label KEY=VALUE`/`KEY-` and `:annotate` with the same grammar | 6 tests | 1 test (label removal + unrelated-metadata preservation) | live: set/remove both keys, unrelated `kept` label/annotation verified to survive | smoke-m8.py: set/remove at both 150x36 and 32x9 | 3 harness bugs (see journal), zero app bugs | ACCEPTED |
| M8.5 | Post-commit verification / Timeline+journal integration | `mutation::Verification` (Verified/Pending/Unknown/ObservedDifferent/TargetReplaced/DeletionInProgress/ObservedGone); `kube::mutation::verify`/`verify_modify`/`verify_delete`/`leaf_path_and_value`; `Phase::VerificationResult` journal entry; `mutation_confirm` runs verify() exactly once, inline, only after `Committed`/`CommittedButJournalIncomplete`; COMMIT RESULT and VERIFICATION always render as two separate, never-collapsed facts; readiness/availability explicitly deferred to M5 Explain/health, not duplicated | 5 tests (`leaf_path_and_value`, RFC 6901 escaping) + 2 (workflow_report separation) + 2 (app: store/Timeline untouched, stale-request ignored) | 10 tests (Verified/ObservedDifferent/TargetReplaced/cancelled-Unknown/DeletionInProgress/ObservedGone/Pending/commit-stays-success-despite-later-Unknown) | `live_m8_5_verification_matches_real_cluster_observations`, run twice | `scripts/smoke-m8.py`, full pass including 32x9 | 3 harness bugs (see journal), zero app bugs | ACCEPTED |
| M8.6 | Combined adversarial acceptance, regressions, soak | `scripts/accept-m8.py` (14 scenarios), `scripts/soak-m8.py` (per-section independent error handling, self-healing fixture recreation) | full locked suite 192 unit + 48 fake HTTP, fmt/check/clippy (`-D warnings`) clean | (same 48, unchanged this slice) | `accept-m8.py` run twice, clean; full M1-M7 regression (`accept-m3.py filters`, `accept-m4.py`, `accept-m4-forward.py`, `accept-m5.py`, `accept-m5-combined.py`, `accept-m6.py`, `accept-m7.py`) all green | `accept-m8.py` itself is the interactive evidence (tmux-automated real terminal) | 1 harness bug in `soak-m8.py` (see journal), zero app bugs | ACCEPTED |

## Design contract (M8.0)

One `mutation::workflow::Workflow` struct carries the shared state for
every action: `MutationIntent`, an optional merge-patch payload, the M7
`PolicyEvaluation`, an optional dry-run outcome, a confirmation-armed flag
(strong confirmation is a deliberate double-press, not a typed-name text
input — see below), an optional commit outcome, and an optional post-commit
verification string.

Confirmation UX is a design choice layered *on top of* M7's binding
authorization, never a replacement for it: pressing a key does not
authorize anything by itself. `mutation::Confirmation::authorizes` (built
fresh from the exact `MutationIntent` at commit time) remains the only real
authorization evidence, exactly as M7 specifies ("typing a name is UX, not
authorization"). This codebase uses a double-press ("press again to
confirm") for `RequireStrongerConfirmation` rather than a typed-object-name
text field, to avoid building a whole new text-input subsystem for a single
UX embellishment — the master prompt explicitly allows "exact UX is your
choice."

Rendering reuses the existing `Document` viewer (adding an optional
`workflow` field) instead of a new UI framework, matching "reuse
existing document/list infrastructure where sensible" from the M7 prompt's
own UI guidance and the project's established pattern (Adjacent/Xray also
render into `Document`). A dedicated `"mutation"` keymap mode (like the
existing `"logs"`/`"forwards"` modes) hosts the dry-run/confirm/cancel keys
without colliding with ordinary document navigation.

Supported kinds are an explicit allowlist per action (never "any resource
with a matching field/verb"), matching M7's own "no arbitrary CRD
inference" discipline.

## Required evidence

M8.0: unsupported resource/action denies before building an intent; no
selection denies; readonly denies with a precise reason, zero requests;
policy denial is rendered, not swallowed; preview is built from the
already-watched (synced) object, never a name-only guess; cancel before
commit sends zero requests and leaves no residual workflow state; a target
replacement or context/epoch change invalidates confirmation and forces a
fresh preview, never silent retargeting; 32x9 safe.

M8.1 (Scale): current replica count is read from the synced watch object,
never assumed to be `0` when absent (UNKNOWN ≠ ZERO); replicas patched via
an exact `spec.replicas` merge patch on the three explicitly supported
kinds only (Deployment, StatefulSet, ReplicaSet) — never the Kubernetes
Scale subresource (see bounded limitation below) and never a guessed field
on an arbitrary CRD; negative/overflow/non-integer input rejected locally
before any network call; TOCTOU/conflict semantics identical to M7;
post-commit shows the request outcome and a freshly observed replica count
as two distinct facts, never collapsed into one "Success".

M8.2 (Restart): the restart timestamp is generated exactly once per intent
and reused verbatim across preview, dry-run, payload hash, confirmation and
commit — a new preview after cancellation gets a new timestamp, never the
same intent silently rebuilt; patches only
`spec.template.metadata.annotations["kubectl.kubernetes.io/restartedAt"]`
on Deployment/StatefulSet/DaemonSet; result says "restart requested",
never "rollout successful".

M8.3 (Delete): explicit allowed-kind set (Pod, Deployment, StatefulSet,
DaemonSet, ReplicaSet, Job, CronJob, ConfigMap) — Namespace/Node/Secret/
ServiceAccount/Role/RoleBinding/ClusterRole/ClusterRoleBinding/CRD/PVC/PV/
StorageClass/webhook/APIService remain unsupported even though M7's policy
model can classify their risk; always `RequireStrongerConfirmation`;
`DeleteParams` carries the exact expected UID as a server-side precondition
(defense in depth alongside M7's own TOCTOU GET); no force/grace-zero
option exposed; result distinguishes accepted-with-deletionTimestamp from
not-yet-observed-absent, never claims "gone" without a fresh GET/watch
confirmation.

M8.4 (Label/Annotate): exactly one key changes per intent; a precise
single-key JSON merge patch (`{"metadata":{"labels":{"key":"value"}}}` or
`{"...":null}` for removal) — never a whole-metadata-map replacement,
so concurrent unrelated keys survive untouched; `kubernetes.io/`/`k8s.io/`
prefixed keys are denied (protected, documented, not a heuristic used
anywhere else); label key/value and annotation key syntax validated
locally before any network call.

M8.5: every workflow's commit produces exactly one M7 journal
`CommitResult` record with a redacted, field-path-only summary (never a
raw object dump); a fresh GET after commit is a distinct, separately
labeled fact from the commit outcome itself; Timeline is never seeded with
a fabricated entry — only the normal watch/relist path may add one, later,
if it observes a real change.

M8.6: fake HTTP suite covers zero-write assertions (readonly, stale epoch,
precommit journal failure, confirmation mismatch) across all four mutating
workflows; one interactive live pass per workflow against `kind-sauron-test`
only; full M1-M7 regression; 75-minute soak.

## Combined live flows -- `scripts/accept-m8.py` (14 scenarios, run twice, both clean)

Not a literal 25-scenario script; scenarios that are pure executor/
transport concerns with no keybinding trigger are covered instead by
`tests/mutation_workflows_live.rs` and the fake-HTTP suite, exactly as
M7's own ledger did for its 20-scenario list -- documented explicitly
below, not silently skipped:

1. Readonly denies `:scale` with zero writes (verified via `kubectl get`
   showing unchanged replicas), rendered visibly (not hidden); `:reload`
   then enables writes for the context without relaunching.
2. Scale: preview shows `replicas: 1 -> 2`, dry-run, commit, `VERIFICATION:
   Verified`; reset back to 1 through the same UX.
3. Restart: commit, `VERIFICATION: Verified`, exact `restartedAt`
   annotation value confirmed via `kubectl`.
4. Label + annotate: set then remove, both commit+verify, unrelated
   pre-existing `kept` label/annotation confirmed to survive via `kubectl`.
5. Escape before commit (`:scale 9`): zero writes, confirmed via
   `kubectl get -o jsonpath` showing replicas unchanged.
6. Delete: genuine double-press strong confirmation, commit + verification
   both render.
7. Live same-name/new-UID replacement between preview and confirm (delete
   `m8-pod` + recreate mid-preview via `kubectl`, then press confirm):
   rejected (`TargetReplaced`/`NotFound`), zero write applied to the
   replacement object, confirmed via `kubectl`.
8. M4 forward started before M8 navigation and checked alive after every
   subsequent scenario (5 checkpoints), zero failures.
9. Rapid resource churn (5x) around an open mutation preview: no
   corruption or crash.
10. M5 Explain regression touchpoint (crashloop fixture) unaffected by M8.
11. M6 Adjacent/Xray regression touchpoint (`m6-web`) unaffected by M8.
12. M7 `:policy`/`:mutations` views unaffected by M8; the journal view
    shows M8's own `VerificationResult`/`CommitResult` entries alongside
    M7's.
13. 32x9: mutation preview open/scroll/confirm/commit/return-to-table,
    no panic or corruption.
14. Quit while the mutation journal view is open: exact `stty` terminal
    restoration.

Executor/transport-only scenarios NOT given a dedicated interactive
scenario above (already proven live in `tests/mutation_workflows_live.rs`
and/or in the fake-HTTP suite): conflict/403 classification and no-
automatic-retry (fake HTTP: `mutation_conflict_and_forbidden_are_explicit_never_forced`,
pre-existing from M7, unaffected by M8); the UID server-side delete
precondition (fake HTTP:
`mutation_delete_commit_carries_the_exact_uid_server_side_precondition`);
every `Verification` outcome variant (fake HTTP, 10 tests, listed in the
slice ledger above); journal `VerificationResult` correlation by
`request_id` (`tests/mutation_workflows_live.rs`'s delete phase asserts
this directly).

## Performance / soak -- `scripts/soak-m8.py`, 75 minutes, complete

**Result**: duration 4506s (~75.1 min), 604 cycles, **zero reconnects,
zero transient errors** for the entire run. RSS 30712 -> 31432 KiB (+2.3%,
allocator noise, consistent with every prior milestone's soak, not a
leak); fds 16 -> 16 (one isolated blip to 17 at cycle 590, back to 16 by
cycle 595 -- not a sustained leak); threads 4 -> 4, flat throughout;
metrics requests climbing steadily 1 -> 822.

Per-category totals across the full run: 1208 mutation previews, 302
dry-runs, 604 scale commits (self-restoring to replicas=1 every cycle, no
drift), 151 restart commits + 453 cancellations, 604 label commits, 604
annotate commits, 604 delete previews, 40 real delete commits (every 15th
cycle, self-healing recreation via `ensure_m8_pod_exists()`), 604
`:policy` checks, 604 `:mutations` journal checks, 604 Adjacent checks,
604 Xray checks, 604 Explain checks, 604 Timeline checks, 604 M4-forward
liveness checks with **zero failures** (one forward held for the entire
75 minutes). Every per-category count landed at exactly 604 (== total
cycles), confirming no section was silently skipped after the harness fix
below -- unlike the first (aborted) soak attempt.

Observations only, not proof of leak-freedom, matching every prior
milestone's own stated honesty about soak evidence.

## Journal

- 2026-09-18: ledger created before implementation. Baseline recorded:
  `m7-accepted`. No cluster access, no mutation-workflow code, no
  acceptance claimed yet. Next: M8.0 shared workflow shell.
- 2026-09-18: `src/mutation/workflow.rs` added -- pure intent builders for
  all five actions (scale/restart/delete/label/annotate), each rejecting
  unsupported kinds before constructing an intent, plus label/annotation
  key/value validation and protected-prefix denial. 14 unit tests, all
  passing; fmt/clippy clean; full unit suite now 166 (was 152 at
  `m7-accepted`). No network code, no UI wiring, no policy-context change
  yet -- this is intent construction only. Still open: how the live running
  app will ever set `cluster_verified_for_mutation = true` for real
  interactive M8 acceptance testing, since M7 never wired this into the
  live app (only into test code) and the M8 prompt forbids a context-name
  heuristic. Working resolution to implement next: reuse the *existing*
  per-context `Settings.readonly` flag (the same mechanism already trusted
  to gate exec/shell/attach/forward) as the mutation-eligibility signal --
  i.e. `cluster_verified_for_mutation = !settings.readonly` at the call
  site, constructed fresh per action, never cached. This is explicit
  operator configuration (config.toml per-context, or the CLI `--readonly`
  override), not a context-name guess, so it does not violate the
  boundary; it also means production stays hard-denied by the existing
  default-readonly posture with zero new mechanism. `open_policy_view()`'s
  hardcoded `false` (from M7, when no mutation UI existed) needs updating
  to match once this is wired in. Not yet done: `Document.workflow` field,
  `"mutation"` keymap mode, `Command::Scale/Restart/Delete/Label/Annotate`
  parsing, `command_names()` entries, app-level action handlers connecting
  to `kube::mutation::preflight`/`commit`, and the zero-bypass/zero-write
  tests. M8.3's delete builder also still needs the executor's
  `delete_request` extended with a UID `DeleteParams` precondition
  (currently `DeleteParams::default()`), per the master prompt's
  defense-in-depth requirement -- not done in this pass.
- 2026-09-18: corrected the working resolution recorded above --
  `cluster_verified_for_mutation` is explicitly NOT `!settings.readonly`.
  `readonly=false` means mutating operations are not globally disabled;
  `cluster_verified_for_mutation=true` means the active context has passed
  strong external verification as the one cluster authorized for real
  mutations. Neither implies the other, and both are enforced as separate,
  independent hard-deny gates in `policy::evaluate` (already true before
  this correction -- only the app-layer wiring above was wrong). Implemented
  as a new, distinct `ConnectOptions.mutation_test_cluster_verified: bool`,
  set only by a new `--mutation-test-cluster-verified` CLI flag --
  structurally parallel to `force_readonly`/`--readonly`, but semantically
  the opposite direction (this one permits, that one restricts), populated
  only by `scripts/test-cluster.sh` after its own independent Docker/API
  identity proof, never derived from context/cluster name or from
  `readonly`. Added two app-level tests locking the separation both ways:
  `readonly_false_alone_never_implies_cluster_verified_for_mutation` and
  `cluster_verified_flag_alone_does_not_bypass_readonly`. `open_policy_view`
  now reads this flag via a new shared `Runtime::mutation_policy_context()`
  helper instead of a hardcoded `false`.
- 2026-09-18: implemented the M8.0 shared UI shell end to end and wired all
  five actions to it. Added: `mutation::PolicyDecision::confirmation_requirement`;
  `mutation::workflow::Workflow` (intent + payload + change + evaluation +
  dry_run + armed + commit + verification, with `confirmation()` always
  rebuilt fresh from the current intent -- never cached, so a target/context
  replacement invalidates it automatically); `mutation::view::workflow_report`
  (TARGET/ACTION/CHANGE/POLICY/PREFLIGHT/CONFIRMATION, matching the M8.0
  design contract); `Document.workflow: Option<Workflow>` and a `"mutation"`
  keymap mode (inherits `"document"` bindings like `"logs"`/`"forwards"`
  already do); `Action::MutationDryRun` ('d') / `Action::MutationConfirm`
  ('y') under the `"mutation"` mode only, no collision with table-mode's
  own 'd'/'y' since keymap conflict-checking is per-mode; `Command::{Scale,
  Restart,Delete,Label,Annotate}` with `:scale N`, `:restart`, `:delete`,
  `:label KEY=VALUE`/`KEY-`, `:annotate KEY=VALUE`/`KEY-` grammar, added to
  `command_names()`. `Runtime::mutation_confirm` implements the double-press
  gate for `RequireStrongerConfirmation` (first press arms and re-renders,
  second press commits) and single-press commit for `RequireConfirmation`/
  `Allow`; both `mutation_dry_run`/`mutation_confirm` refuse before
  spawning any task when the decision is `Deny`/`Unsupported`, and
  `mutation_confirm` also refuses on a stale epoch (target/context replaced
  since the preview opened) -- both proven by zero-task-count assertions,
  not just an error return. Denied previews still open (visible per the
  M8 "readonly UX" requirement); only confirming them is blocked.
  `kube::mutation::delete_request` now sets a server-side `Preconditions{
  uid }` (defense in depth alongside the client TOCTOU GET already in
  `revalidate`); proving this required extending the fake HTTP `Server` test
  harness in `tests/watch_transport.rs` to read the Content-Length body, not
  just headers (it previously stopped at the blank line, so PATCH/DELETE
  body assertions were silently impossible until now).
  Added 35 new tests across `mutation::workflow`, `mutation::view`,
  `command`, and `app` (listed in the slice ledger above), plus 1 new
  fake-HTTP test. Full suite now 183 unit + 38 fake HTTP (was 152 + 37 at
  `m7-accepted`); fmt/clippy clean.
  Not yet done: M8.5 post-commit verification (`Payload::MutationCommit.
  verification` is always `None` for now -- no fresh-GET/watch integration
  yet), live fixtures/tests (`tests/mutation_workflows_live.rs`,
  `scripts/accept-m8.py`), the required interactive TUI smoke pass, and
  M8.6's combined regression + soak. No live cluster access has been used
  in this session; all evidence so far is unit + fake HTTP only.
- 2026-09-18: M8.5 implemented and ACCEPTED, closing M8.0-M8.5 together
  (M8.1-M8.4 pick up their live/interactive evidence from the same pass,
  since verification is exercised through all five workflow builders).
  Added `mutation::Verification` (`Verified`/`Pending`/`Unknown`/
  `ObservedDifferent`/`TargetReplaced`/`DeletionInProgress`/`ObservedGone`) --
  a fact distinct from `MutationOutcome`, never allowed to downgrade a
  `Committed` outcome. `kube::mutation::verify` dispatches on `intent.effect`:
  `Modify` walks the payload itself (a single-leaf JSON merge patch, exactly
  what every M8 builder produces) down to a JSON-pointer path via
  `leaf_path_and_value` (RFC 6901 escaped, since annotation keys routinely
  contain literal `/`) and compares a fresh bounded GET against it; `Delete`
  does a metadata-only GET and reads `deletionTimestamp`/404. Exactly one
  attempt, bounded by `connection.timeout()`, cancellable, never retried.
  `Runtime::mutation_confirm`'s spawned task calls `verify()` inline
  immediately after a `Committed`/`CommittedButJournalIncomplete` outcome
  only (never for Denied/Cancelled/etc, since there is nothing to verify),
  journals a new `Phase::VerificationResult` record correlated by
  `request_id`, and sends both facts in one `Payload::MutationCommit`.
  `workflow_report` renders `COMMIT RESULT` and `VERIFICATION` as two
  separate lines with an explicit comment that verification never revises
  the commit line, plus a note that readiness/availability stays M5's
  Explain/health engine (not duplicated here). Verification is a plain
  network GET outside the watch/store pipeline, so it structurally cannot
  fabricate a Timeline entry or touch Adjacent/Xray's observation-derived
  graph -- proven directly by
  `mutation_commit_verification_never_touches_the_store_or_fabricates_timeline`.
  30 new tests total (5 `leaf_path_and_value` + 10 fake-HTTP `verify`/commit-
  stays-success-despite-later-unknown + 2 `workflow_report` + 2 app store/
  stale-request + a few grammar/registry touch-ups already counted above).
  Full locked suite: 192 unit + 48 fake HTTP, fmt/check/clippy (`-D warnings`)
  clean.
  Live evidence: created `tests/fixtures/m8-mutation.yaml` (dedicated
  `sauron-m8` namespace: Deployment `m8-deploy`, ConfigMap `m8-meta` with
  deliberately-unrelated pre-existing label/annotation, disposable Pod
  `m8-pod`), and `m8-fixtures`/`m8-reset`/`m8-test` cases in
  `scripts/test-cluster.sh`. `tests/mutation_workflows_live.rs`
  (`live_m8_5_verification_matches_real_cluster_observations`) exercises
  all five workflows against the real `kind-sauron-test` cluster in one
  sequential test (matching `mutation_live.rs`/`relationships_live.rs`'s own
  precedent to avoid intra-file races): scale 1→2 (`Verified`) then 2→1
  reset through the same builder/executor path; restart with an exact
  timestamp (`Verified`); label set then remove on `m8-meta`, asserting the
  unrelated `kept` label survives both; annotate set then remove, same
  unrelated-preservation assertion; delete `m8-pod` with a bounded (≤10s)
  poll loop in the TEST HARNESS ONLY (never inside the app, which attempts
  exactly once) that resolves to `ObservedGone`, journaled. Ran twice via
  `scripts/test-cluster.sh m8-test`, both clean; fixtures reset to pristine
  between and after (`m8-reset`; verified label/annotation/replica state by
  direct `kubectl get` afterward).
  Interactive evidence: `scripts/smoke-m8.py`, a tmux-automated (not manually
  human-observed -- recorded honestly as automated) real-terminal pass
  against the live cluster with `--mutation-test-cluster-verified` and an
  operational `readonly=false` config for `kind-sauron-test` only. Covers:
  `:scale` preview showing TARGET/ACTION/CHANGE/POLICY/PREFLIGHT/CONFIRMATION,
  Escape-cancel leaving zero residual mutation state, dry-run vs commit as
  distinct keys/steps, scale round-trip with `VERIFICATION: Verified`;
  `:label` set/remove with COMMIT RESULT/VERIFICATION both rendering; delete
  requiring a genuine second key press (single press only arms and never
  commits, proven by the preview text changing to "press confirm again");
  32x9 open/scroll/confirm/commit/return-to-table without panicking; exact
  `stty` terminal restoration on quit.
  Three harness/script bugs found and fixed while building the smoke
  script -- classified as harness bugs, not app bugs, since the app behaved
  exactly as designed in each case: (1) used `v1/deployments` instead of
  `apps/v1/deployments` (Deployment is an `apps/v1` kind, not core `v1`) --
  the app correctly reported "Resource not found"; (2) relied on
  auto-selection of the first table row for ConfigMaps without accounting
  for Kubernetes' own auto-injected `kube-root-ca.crt` ConfigMap in every
  namespace, which could sort/arrive before `m8-meta` -- fixed by using the
  existing `name=X` filter grammar explicitly in every navigation, exactly
  as `accept-m4.py`/`accept-m6.py`/`accept-m7.py` already do; (3) `q` at the
  table only cleared an active `name=...` filter first (Back's documented,
  correct, pre-existing behavior) rather than quitting immediately -- fixed
  by sending `q` twice. No SAURON defect was found or fixed in this pass.
  Bounded limitation carried forward, not yet closed: M8.2 (Restart) has no
  dedicated interactive scenario in `smoke-m8.py` (only live + fake-HTTP
  coverage this pass) -- deferred to M8.6's `scripts/accept-m8.py`, per the
  same documented-not-silently-skipped discipline M7's ledger used for its
  own 20-scenario list.
  M8.0-M8.5 all marked ACCEPTED above. Next: M8.6 combined adversarial
  acceptance, full M1-M7 regression, 75-minute soak -- not started until
  now, per explicit instruction not to open it before M8.5 closes.
- 2026-09-18: M8.6 started. `scripts/accept-m8.py` written (14 scenarios:
  readonly denies `:scale` with zero writes then `:reload` enables it in
  the same session; scale dry-run/commit/verify round-trip; restart exact
  annotation observed; label/annotate set+remove with unrelated-metadata
  preservation; Escape-before-commit sends zero writes; delete double-press
  strong confirmation; live same-name/new-UID replacement between preview
  and confirm is rejected with zero write applied; M4 forward held and
  checked throughout; rapid namespace churn around an open preview; M5/M6/
  M7 regression touchpoints; 32x9; exact terminal restoration) -- PASS
  twice against `kind-sauron-test`. Full M1-M7 regression re-run and green:
  `accept-m3.py filters`, `accept-m4.py`, `accept-m4-forward.py`,
  `accept-m5.py`, `accept-m5-combined.py`, `accept-m6.py`, `accept-m7.py`.
  Bug discipline note: found and fixed a real defect in `scripts/soak-m8.py`
  itself during the first 75-minute soak attempt, not in SAURON. At cycle
  15 the soak's own post-delete fixture recreation
  (`test-cluster.sh m8-fixtures`) timed out (60s, likely resource
  contention with the concurrent restart-triggered rollout), leaving
  `m8-pod` genuinely absent; SAURON correctly reported "Select a row first"
  for every subsequent `:delete` attempt on a namespace that truly had no
  such pod -- not an app defect. The real bug was in the harness: (1) the
  whole per-cycle body ran inside ONE `try/except`, so once the delete
  step raised, the rest of that cycle's Explain/Timeline/Adjacent/Xray/
  forward-alive checks were silently skipped too, degrading soak coverage
  for the remaining ~40 minutes without being noticed until manual
  inspection; (2) the recreation had no retry and no independent
  self-healing check. Fixed by splitting each cycle into independent
  per-section `try/except` blocks (a failure in one section no longer
  blocks the others) and adding `ensure_m8_pod_exists()` -- checked every
  cycle, not only right after a commit, with 3 retries and a longer
  timeout. Verified with a 150-second dry run spanning past cycle 15: the
  delete commit, recreation, and every subsequent cycle's full check set
  (including Explain/Timeline/Adjacent/Xray/forward) all completed with
  zero reconnects and zero transient errors. Full 75-minute soak relaunched
  with the fixed harness; results pending.
- 2026-09-18: M8.6 ACCEPTED, closing all of M8 (M8.0-M8.6). Full 75-minute
  soak with the fixed harness completed clean: 604 cycles, zero reconnects,
  zero transient errors, RSS +2.3% (allocator noise), fds/threads flat.
  Every per-category check landed at exactly 604/604, confirming the
  per-section error isolation fix actually holds under the full run
  duration, not just the 150-second validation. Full locked suite
  re-confirmed clean one final time: 192 unit + 48 fake HTTP, fmt/check/
  clippy (`-D warnings`) all pass. Fixtures reset to pristine and verified
  by direct `kubectl get` (replicas=1, `kept` label/annotation intact, no
  leftover `soak=`/`accept=`/`narrow=`/`replace-check=`/`churn=` keys).
  **All of M8 (M8.0-M8.6) ACCEPTED.** Proceeding to create the local
  annotated tag `m8-accepted` (never pushed without explicit
  authorization, matching every prior milestone's own tag discipline).
