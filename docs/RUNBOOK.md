# SAURON development runbook

Updated 2026-09-21. Canonical architecture/status: [HANDBOOK](../HANDBOOK.md).

## Production protection

The user's existing cluster is **production**. Reads are allowed. Any operation that
could alter it is forbidden. Never run fixture creation/cleanup against default context.
No exec, debug/helper pods, uploads, port-forward-based application writes, mutations,
Helm/GitOps actions, or plugin execution against production during this development task.
No application writes have been performed there. Only its version endpoint was read.

Fixture cluster is independently hosted by Docker:

| Identity | Value |
| --- | --- |
| kind cluster | `sauron-test` |
| Docker node | `sauron-test-control-plane` |
| Context | `kind-sauron-test` |
| Kubeconfig | `/home/ximbi/SAURON/.test-cluster/config` (ignored; contains credentials) |
| Kubernetes | v1.33.1 |
| Fixture namespace | `sauron-fixtures` |

`scripts/test-cluster.sh` verifies the context's API URL against this Docker container's
published loopback port before any fixture writes. A context name alone is not enough.
It never falls back to the default kubeconfig and has no production cleanup path.

M9 uses a separate Docker kind cluster `sauron-m9`, node
`sauron-m9-control-plane`, context `kind-sauron-m9`, ignored kubeconfig
`.test-cluster-m9/config`. Its sibling `scripts/test-cluster-m9.sh` verifies
the Docker label and loopback API endpoint before fixture operations. Keep
the two clusters and guards separate; production remains excluded.

## Current checkpoint — reconciled 2026-09-22

M1–M8B are ACCEPTED. M9.0–M9.5 and M9.7 are ACCEPTED; M9.6
(Helm rollback/uninstall) is explicitly DEFERRED, not implemented.
M10.0–M10.9 are ACCEPTED. M11.0–M11.8 are ACCEPTED (M11.6 context diff at
a reduced scope). M12.0–M12.8 are ACCEPTED (M12.3 providers explicitly
DEFERRED, evidence-backed; M12.5 packaging proven live for Linux x86_64
only, the other 3 platforms CI-defined but never executed) — `main` and
every tag through `m12-accepted` are pushed to `origin`; the project is
published on crates.io as `saur-on` (binary/crate name stays `sauron`)
and has a draft GitHub Release with all 4 platform archives. M13
(live theme switching, persistence, header banner/info panel) is the
current milestone in progress: M13.1–M13.3 and M13.5 are ACCEPTED, M13.4
(banner style selection) explicitly DEFERRED — see
`docs/M13_ACCEPTANCE.md`.

Recorded M9 checks: 305 unit + 74 fake HTTP, plus 9 Flux/Argo CD/Helm live
tests; locked fmt/check/clippy/test green in the acceptance record. Combined
acceptance and M1–M8B regressions passed twice (one prior M3 timing flake is
documented, not erased). M9's bounded soak was **240 seconds per run**,
53–54 cycles, flat RSS/fds/threads, zero reported reconnects/transient errors.
Earlier 75-minute soaks belong to their own milestones, not M9.

Recorded M10 checks: 390 unit + 76 fake HTTP; locked fmt/check/clippy/test
green. `scripts/accept-m10.py` (bulk multi-select mutations, workspace/
bookmark round trips, config persistence across a real process restart,
malformed keymap/theme fail-safe, 32x9, terminal restoration) passed twice
clean. Full M1–M9 regression (every existing `accept-m*.py`, unmodified)
reconfirmed green — including a rebuild of both isolated kind clusters
(`sauron-test`, `sauron-m9`) after both drifted into a kubelet/API x509
trust failure from long uptime, an environment issue unrelated to any
SAUR-ON code. M10's bounded soak was **300 seconds**, 178 cycles
(selection/workspace/bookmark every cycle, `bulk_label` throttled to every
10th, matching M9's own throttling precedent), flat RSS/fds/threads, zero
reconnects/transient errors. See `docs/M10_ACCEPTANCE.md`'s M10.9 journal
entry for two real bugs found and fixed during this milestone's own
acceptance work and one suspected regression root-caused to a stale test
binary, not a defect.

Recorded M11 checks: 427 unit + 76 fake HTTP; locked fmt/check/clippy/test
green. M11 is a composition milestone: Eye/Pulse are pure re-renders of
already-loaded state (zero new Kubernetes requests); evidence bundle
reuses the exact same Explain/Events/Adjacent collectors plus a new atomic
multi-file writer modeled on `Config::save`; blast radius reuses the same
bounded Xray collector, grouped by the existing `graph::Provenance`;
context diff (investigated, then accepted at reduced scope rather than
deferred) does one bounded GET on a temporary second `Connection` that
`Runtime` never stores. `scripts/accept-m11.py` (8 sequences, including a
live proof that Eye/Pulse never claim healthy/zero under a real
RBAC-forbidden LIST) passed twice clean. Full M1-M10 regression
reconfirmed green, including finding and fixing a real pre-existing bug in
`accept-m9.py`/`soak-m9.py`'s own Argo CD rollback-revision source
(`.status.sync.revision` can mirror an unresolved branch name;
`:argocd_rollback`'s own denial of that was correct — the test scripts'
assumption was not). M11's bounded soak was **300 seconds**, 103 cycles,
flat RSS/fds/threads, zero reconnects/transient errors. See
`docs/M11_ACCEPTANCE.md`'s M11.8 journal entry for full detail.

Recorded M12 checks: 445 unit + 76 fake HTTP; locked fmt/check/clippy/test
green. M12.1 plugin execution is genuinely new architecture (first local-
subprocess boundary), reusing `app::session::Sessions` verbatim for task
ownership/cancellation. A real live bug was found and fixed in M12.2: task
`abort()` (the real shutdown path, distinct from cooperative cancel) never
reached the running future's own cancellation branch, so a plugin's
grandchild process (e.g. `sh -c "... & wait"`) could survive app quit;
fixed with a `GroupKillGuard` held for the whole run scope, regression
test added, re-verified live. `scripts/accept-m12.py` (real plugin stdout/
exit code, live no-credential-leak proof, live timeout and live mid-run
cancellation each confirmed via a real OS-process-table check, headless
`schemaVersion`, packaged-archive extract+run) passed twice clean. Full
M1-M11 regression reconfirmed green, including cleaning up genuine
ReplicaSet fixture drift (accumulated across this long session,
`revisionHistoryLimit=10` never pruned it) and correcting a false claim in
M12.4's own journal (`schemaVersion` is present with value 1, but is NOT
guaranteed to be the first serialized key — `serde_json::Value` has no
`preserve_order` feature, deliberately not enabled since it would make
`mutation::workflow::payload_hash`'s canonical-JSON fingerprint depend on
source key order, a real risk to a safety-critical mutation-confirmation
path). M12's bounded soak was **180 seconds**, 64 cycles, flat RSS/fds/
threads, zero reconnects/transient errors, and an explicit post-shutdown
`pgrep` sweep confirming zero orphan plugin processes. See
`docs/M12_ACCEPTANCE.md`'s M12.8 journal entry for full detail.

These are recorded results, not tests rerun during this documentation update.

Current architecture: M6 bounded Adjacent/Xray; M7 policy/confirmation/
TOCTOU/journal; M8 shared mutation UI and post-commit verification; M8B
advanced operations including bounded drain; M9 Flux/Argo inspection and
guarded actions through that same gateway; M10 bounded multi-select driving
bulk mutations that reuse the unmodified M7/M8/M8B gateway per target, plus
workspaces/bookmarks and their persistence in `Config` (the first write
path `Config` has ever had: atomic, `0600`, temp-file-then-rename),
configurable keymaps and themes (both fail safe on malformed config); M11
read-only evidence composition -- Eye, Pulse, evidence bundle
(`:bundle PATH [--force]`), blast radius (`:blast_radius`), context diff
(`:context_diff CONTEXT`); M12 trust-gated plugin subprocess execution
(`:plugin NAME`, env-allowlisted, bounded output, timeout/process-group
cancellation via `Sessions`), headless `schemaVersion` hardening, Linux
x86_64 packaging (dual MIT/Apache-2.0, `scripts/package.sh`,
`docs/INSTALL.md`), checksums/license inventory (`scripts/checksums.py`,
`docs/LICENSE_INVENTORY.md`), and a performance campaign
(`benches/pipeline.rs` cold-startup + 20k-object tier).
Helm is inspection-only, with an
explicit user-triggered, bounded, UID/type-checked Secret-body reader.
Values masking is heuristic, manifest output is identities only, NOTES are
omitted. No general Secret-reveal permission is implied.

Production remains read-only. Regression fixtures use `sauron-test`,
`.test-cluster/config`, `kind-sauron-test`, and `scripts/test-cluster.sh`.
M9 fixtures use separate `sauron-m9`, `.test-cluster-m9/config`,
`kind-sauron-m9`, and `scripts/test-cluster-m9.sh`. Both require
Docker-label/API-endpoint verification; never default kubeconfig. Both are
local, self-contained Docker containers with no production data, and may
need to be recreated (`kind delete cluster` + the matching
`scripts/bootstrap-test-cluster{,-m9}.sh`) if their kubelet drifts into a
kubelet/API x509 trust failure after long uptime.
The explicit mutation-test flag is a harness assertion, not automatic proof
of cluster identity or permission to operate on production.

M12 is ACCEPTED and was the final milestone of the current roadmap. Read
the relevant acceptance ledger before continuing any future work; preserve
accepted behavior and the unresolved bounded terminal stdin limitation in
`docs/EXEC.md`. No implementation is authorized merely by this status
update.

## Historical milestone checkpoints

The entries below preserve milestone-time instructions and test counts. Statements
such as “in progress” or “not started” apply to their recorded stage, not current HEAD.

M6 started 2026-09-18 from accepted M5 (`1a45dbd`; HEAD also includes README).
M6.0 graph metadata foundation and ledger implemented in the working tree;
no relationship API collector or UI yet, no M6 acceptance. Current continuation:
M6.0 foundation committed `14f4562` (110 unit + 19 fake HTTP green); M6.1
typed Pod/workload/owner extraction and timed metadata-aware transport implemented.
Full locked suite 116 unit + 22 fake HTTP green; new guarded `m6-fixtures` and
`m6-test` commands, live ownership/template reference check PASS. Aggregate
report also passed live; 116 unit + 25 fake HTTP green (final fmt correction
verified). Network/storage resolver families and UI remain pending. Track
actual checks and live evidence in [M6_ACCEPTANCE.md](M6_ACCEPTANCE.md).
No cluster operations performed during this initial foundation work.

Latest continuation: M6.2 network resolver in progress. First guarded live network
test found EndpointSlice targetRef commonly omits apiVersion; fix/regressions added,
full suite and exact `bash scripts/test-cluster.sh m6-test` replay running. Wait for
that result before continuing. Fixtures now include Service/Ingress/TLS-reference
Secret in isolated sauron-m6. Aggregate checkpoint `92f6b8b`; current code uncommitted.

M6.2 ACCEPTED 2026-09-18: EndpointSlice fix independently reviewed and verified
(unit + fake HTTP + guarded live m6-test all green with the real controller
object missing apiVersion). Added the missing reverse Pod→Service selector
fake HTTP test to close the last required-evidence gap. Continuing to M6.3.

M6.3 ACCEPTED 2026-09-18: PVC/PV/StorageClass extraction and bounded reverse
Config/Secret/ServiceAccount/PVC scanning added and live-verified against a
real dynamically-provisioned local-path PV, including exact claimRef UID
match. Continuing to M6.4 (`:adjacent` view).

M6.4 ACCEPTED 2026-09-18: `:adjacent`/`a` opens a grouped relationship view
(OWNED BY/OWNS/SELECTED BY/REFERENCES/REFERENCED BY); `enter` (Follow) jumps
to the related object under the cursor via its exact canonical identity,
reusing the existing history stack. Unit + app-level + live coverage green.
No interactive terminal smoke test run this session. Continuing to M6.5
(Xray).

M6.5 ACCEPTED 2026-09-18: `:xray`/`x` opens a bounded (depth 1-3) cycle-safe
BFS reusing Adjacent's exact single-hop logic and the same deterministic
health per node. Each frontier node is re-fetched and UID-validated before
its edges are trusted. Live-verified with a real 2-hop
Deployment→ReplicaSet→Pod traversal. Continuing to M6.6 (combined
adversarial acceptance, full M1-M5 regression, soak) -- the final M6 slice.

M6.6 ACCEPTED 2026-09-18 -- M6 fully closed: `scripts/accept-m6.py` (18 live
scenarios) all PASS, run twice, against `kind-sauron-test`. Found and fixed
a real Follow bug along the way (only the first Adjacent/Xray target was
ever reachable once a report fit on screen without scrolling); now an
explicit `adjacent_selected` cursor. Full M1-M5 regression re-run and green
(`accept-m3/m4/m4-forward/m5/m5-combined.py`). 75-minute soak
(`scripts/soak-m6.py`) complete: 1263 cycles, RSS +0.15% over the whole
run, fds/threads flat, one transient self-recovered timeout. **All of M6
(M6.0-M6.6) ACCEPTED.** Local tag `m6-accepted`, never pushed without
explicit authorization.

M7 (mutation policy/guardrails/journal infrastructure, no user-facing
workflow) started 2026-09-18 from accepted M6. M7.0-M7.5 implemented and
unit/fake-HTTP tested (see [M7_ACCEPTANCE.md](M7_ACCEPTANCE.md)): mutation
model + central policy engine (`src/mutation.rs`, `src/mutation/policy.rs`),
the single execution gateway with TOCTOU revalidation
(`src/kube/mutation.rs`), the redacted append-only journal
(`src/mutation/journal.rs`), and the read-only `:policy`/`:mutations` TUI
surfaces (`src/mutation/view.rs`). The one internal proof mutation
(`tests/mutation_live.rs`, `sauron-m7` fixture) live-verified twice against
`kind-sauron-test` only: preview→dry-run→confirmation→commit→verify→journal,
plus same-name/new-UID replacement rejection. `scripts/accept-m7.py` (13
scenarios covering `:policy`/`:mutations` plus M4/M5/M6 regression
touchpoints, since M7 exposes no mutation-triggering keybinding) PASS twice.
Full M1-M6 regression re-run and green after all M7 changes. 75-minute soak
(`scripts/soak-m7.py`) complete: 1071 cycles, zero reconnects (better than
every prior milestone's soak), RSS +1.2% (allocator noise), fds/threads
flat. Found and fixed a real flaky fake-HTTP test helper
(`test_journal()`'s path counter always returned the same value) before
tagging. **All of M7 (M7.0-M7.6) ACCEPTED.** Local tag `m7-accepted`,
never pushed without explicit authorization. M7 ships no user-facing
mutation workflow by design; M8 has not started.

M8 (guarded user-facing mutation workflows) started 2026-09-18 from
accepted M7. M8.0-M8.5 ACCEPTED (see [M8_ACCEPTANCE.md](M8_ACCEPTANCE.md)):
shared `mutation::workflow::Workflow` shell + `"mutation"` keymap mode +
`:scale`/`:restart`/`:delete`/`:label`/`:annotate` grammar; a new distinct
`ConnectOptions.mutation_test_cluster_verified` (via
`--mutation-test-cluster-verified`, never derived from `readonly` or
context name) for guarded live testing; M8.5 post-commit verification
(`mutation::Verification`, `kube::mutation::verify`) rendering COMMIT
RESULT and VERIFICATION as two separate, never-collapsed facts. Full
locked suite 192 unit + 48 fake HTTP green. Live:
`tests/mutation_workflows_live.rs` against dedicated `sauron-m8` fixtures
(all five workflows), run twice. Interactive: `scripts/smoke-m8.py`
(tmux-automated), full pass including 32x9. Three harness/script bugs
found and fixed, zero app bugs.

M8.6 ACCEPTED 2026-09-18 -- M8 fully closed: `scripts/accept-m8.py` (14
scenarios) all PASS, run twice, against `kind-sauron-test`. Full M1-M7
regression re-run and green. 75-minute soak (`scripts/soak-m8.py`)
complete: 604 cycles, zero reconnects, zero transient errors, RSS +2.3%,
fds/threads flat, M4 forward alive throughout with zero failures. Found
and fixed a real bug in the soak harness itself (a single wide
`try/except` silently skipped the rest of a cycle's checks after one
recoverable failure; fixed with per-section isolation and a self-healing
fixture-recreation check) -- zero SAURON app bugs found. **All of M8
(M8.0-M8.6) ACCEPTED.** Local tag `m8-accepted`, never pushed without
explicit authorization.

M8B (Advanced Cluster Operations: Cordon/Uncordon, Set image, CronJob
trigger, Evict, Drain, Force delete) ACCEPTED end to end
(M8B.0-M8B.7) -- see [M8B_ACCEPTANCE.md](M8B_ACCEPTANCE.md). Combined
acceptance (`accept-m8b.py`, 19 scenarios) run twice clean, full M1-M8
regression green, 75-minute soak (488 cycles, one recoverable transient
hiccup, zero unrecovered errors). Two real live-cluster bugs found and
fixed (Drain preview cancellation-token staleness; a missing keymap
mode-availability rule for Drain's confirm key). Local tag
`m8b-accepted`, never pushed.

M9 (Flux, Argo CD, Helm integrations) started from accepted M8B -- see
[M9_ACCEPTANCE.md](M9_ACCEPTANCE.md) for the full slice-by-slice record.
M9.0 (shared capability model), M9.1-M9.2 (Flux read + guarded actions:
reconcile/suspend/resume), M9.3-M9.4 (Argo CD read + guarded actions:
sync/refresh/rollback) and M9.5 (Helm read-only inspection) all
ACCEPTED, each live-verified against a dedicated second kind cluster
(`sauron-m9`, real Flux v2.9.5 and Argo CD v3.5.3 installed via pinned
manifests, `kind-sauron-test` left untouched as the M1-M8B regression
environment). M9.5 required its own explicit security design decision:
`resources::Object::new()`'s unconditional Secret redaction means a
Helm release body (stored in a `Secret`'s `data.release` field) can
never flow through the generic object pipeline, so a single narrow,
bounded, TOCTOU-safe `kube::helm::read_release` was added as the one
permitted exception -- never a relaxation of the generic invariant (see
M9_ACCEPTANCE.md's own M9.5 write-up for the full contract). M9.6 (Helm
rollback/uninstall) is explicitly DEFERRED: investigation found no
maintained Rust crate implements Helm's client-side action logic, Helm
v3+ has no server/API component to target instead (Tiller was removed
in v3), and reimplementing rollback/uninstall by hand against the
release Secret storage risks corrupting Helm's own bookkeeping in ways
that could break the real `helm` CLI's ability to operate on a release
afterward -- none of shelling out, direct storage manipulation, or a
reduced-scope reimplementation were judged acceptable inside M9. M9.7
(combined acceptance) ACCEPTED: `accept-m9.py` (interactive, covering
Flux/Argo CD/Helm live evidence, readonly zero-write checks,
CRD-absence handling, cancellation, 32x9, terminal restoration, M4
forward held throughout) run twice clean against `sauron-m9`, plus the
full unmodified M1-M8B regression suite green each time, plus a
4-minute bounded soak (`soak-m9.py`, scoped down from M8B.7's own
75-minute run since that soak already proved the shared mutation
gateway's stability -- this one's own job is narrower, proving M9's
additions don't regress it) with zero reconnects and zero transient
errors across 53-54 cycles. **All of M9 (M9.0-M9.5, M9.7) ACCEPTED;
M9.6 explicitly DEFERRED, not a blocker.** Local tag `m9-accepted`,
never pushed without explicit authorization.

M10 (Bulk/workspaces/bookmarks/themes/keymaps) started from accepted M9 --
see [M10_ACCEPTANCE.md](M10_ACCEPTANCE.md) for the full slice-by-slice
record. M10.1-M10.3 (bounded `Selection`, bulk read-only ops, bulk guarded
mutations reusing the unmodified M7/M8/M8B gateway per target), M10.4-M10.5
(workspaces, bookmarks), M10.6-M10.7 (configurable keymaps, themes -- both
fail safe on malformed config rather than crash startup), and M10.8
(`Config` persistence: the first write path it has ever had, atomic,
`0600`, temp-file-then-rename) all ACCEPTED individually with their own
unit/fake-HTTP/live evidence. M10.9 (combined acceptance/full regression/
soak) found and fixed two real bugs during its own acceptance work -- a
`startup_warning` that `Payload::Ready` cleared before a user could see it,
and an `accept-m3.py` cursor-detection break caused by M10's own new row
markers -- and root-caused one suspected `accept-m3.py` descending-sort
regression to a stale test binary, not a defect (two new regression tests
added anyway, closing a real, previously-untested coverage gap). Full
M1-M9 regression (every existing `accept-m*.py`, unmodified) reconfirmed
green, including recreating both isolated kind clusters after an
environment-only kubelet/x509 trust failure from long uptime. Combined M10
acceptance (`accept-m10.py`) passed twice clean. A 300-second bounded soak
(`soak-m9.py`'s own scoping precedent -- M8B.7's soak already proved the
shared gateway's stability, this one's own job is narrower, proving M10's
selection/bulk/workspace/bookmark additions don't regress it) ran 178
cycles with zero reconnects and zero transient errors, flat RSS/fds/
threads. **All of M10 (M10.0-M10.9) ACCEPTED.** Local tag `m10-accepted`,
never pushed without explicit authorization.

M11 (Eye/Pulse/evidence bundle/blast radius/context diff) started from
accepted M10 -- see [M11_ACCEPTANCE.md](M11_ACCEPTANCE.md) for the full
slice-by-slice record. Reconnaissance (M11.0) found every evidence
primitive M11 needed already existed and was accepted: `src/evidence.rs`
is literally the "shared evidence vocabulary" the milestone needed,
`graph::Provenance` is literally the relationship-category set blast
radius needed. M11.1 (shared priority/count foundation over
`Object.health`), M11.2 (Eye), M11.3 (Pulse, deliberately narrower than
`:info`, zero new Kubernetes requests), M11.4 (evidence bundle, atomic
multi-file export modeled on `Config::save`), M11.5 (blast radius,
reordered ahead of context diff per `docs/SOFKA_PARITY.md`'s own P1 vs P3
classification), M11.7 (cross-feature UX, confirmed accepted by
construction -- every command already went through the one central
registry as it shipped) all ACCEPTED individually with their own
unit/live evidence. M11.6 (context diff) was investigated per the
project's own M9.6 precedent and found NOT blocked -- accepted at a
reduced scope (single-target comparison only) instead of deferred, since
`kube::connect` is a free function and a temporary second `Connection`
never needs to live on `Runtime`. M11.8 (combined acceptance/full
regression/soak) found and fixed a real pre-existing bug in
`accept-m9.py`/`soak-m9.py`'s own Argo CD rollback-revision source during
the M1-M10 regression sweep (`.status.sync.revision` can mirror an
unresolved branch name rather than the resolved commit SHA
`.status.history` records; `:argocd_rollback`'s own denial of an
unrecorded revision was correct app behavior, the test scripts' assumption
was not) -- fixed both scripts to source the last history entry instead,
documented, and the full M9 regression (including its own 240s soak)
re-ran clean afterward. New `scripts/accept-m11.py` (8 sequences,
including a live proof -- a real RBAC-forbidden `ServiceAccount`, the same
pattern M6's own seq11 established -- that Eye/Pulse never convert
partial/unknown evidence into a false "healthy/zero problems" claim)
passed twice clean. A 300-second bounded soak (`soak-m10.py`'s own scoping
precedent -- every M11 view under soak is read-only, so unlike M8/M8B/M9/
M10's soaks there is no mutation cadence to throttle, only I/O: bundle
export and context diff run every 5th cycle) ran 103 cycles with zero
reconnects and zero transient errors, flat RSS/fds/threads. **All of M11
(M11.0-M11.8) ACCEPTED.** Local tag `m11-accepted`, never pushed without
explicit authorization.

M4 is fully ACCEPTED: M4.0, M4.1, all of M4.2 (one-shot exec + interactive shell),
M4.2b (attach), M4.3 (port-forward manager), and M4.4 (combined adversarial
acceptance + full M1-M4 regression + 75-minute soak) all ACCEPTED. Local annotated
`m4-accepted` created, never pushed. Contract: PORT_FORWARD.md, EXEC.md,
[M4_ACCEPTANCE.md](M4_ACCEPTANCE.md).

M4.4 accepted 2026-09-16: ten combined live sequences run in order against
`kind-sauron-test` (logs/ns/ctx/history; multi-source search/filter/pause/delete/
resume; forward/navigate/context/logs/stop; exec startup/cancel/exec; shell/resize/
remote-exit/palette; forward+logs same-name replacement; rapid scope changes;
manager rapid start/stop; 32x9 logs/manager/help; shutdown during logs/forward/
manager) -- no task/listener/terminal/scope leak observed in any case. Plus a full
regression rerun (`accept-m3.py filters`/`sorting`, `accept-m4.py`/`logs`,
`accept-m4-forward.py` full) and a 75-minute soak (912 cycles, 911 log sessions,
one continuously-held forward, RSS 29000→29488 KiB flat, fds 17→18, threads
steady at 4, one self-recovered timing hiccup). One real finding: a terminal-
input-protocol ambiguity (byte-adjacent Escape + next key merging into an unbound
`Alt+<key>` event in crossterm's parser) made one spot in `accept-m4.py` flaky;
root-caused with a temporary key trace (added, used, fully removed), fixed with a
small delay in the script, not the app -- bounded/self-recoverable, same category
as the already-documented phantom-Enter limitation. Full suite (76 unit + 14 fake
HTTP) green before and after the soak.

M4.3 (port-forward manager) ACCEPTED, implemented from clean 7af4926. Latest M4.3
checks: 76 unit + 14 fake HTTP = 90 passed, fmt/check/clippy clean.
Live `python3 scripts/accept-m4-forward.py` passed real HTTP, navigation/context/logs,
explicit/conflicting/auto ports, four forwards/eight clients, individual stop, 12 cycles,
five immediate start/cancel cycles, deletion/new UID, 32x9 and listener/stty cleanup.
`--policy-only` proved readonly survives reload and still denies all operational actions.
Dedicated kind identity checked and node Ready. Clean baseline verified at annotated
`m3-accepted`, commit `9eac329f5581da45251d81c20a278e53531d4d1a`. Full locked
fmt/check/clippy/test at M4.2b passed 72 unit + 11 fake HTTP = 83; one opt-in live test
ignored. No tracked credentials found. M5 is fully ACCEPTED against real
metrics-server v0.8.1 on `kind-sauron-test`: evidence/freshness foundation
(M5.0), the Metrics API collector (M5.1), static requests/limits/QoS accounting
plus live usage/percentages in the table/filter/sort pipeline (M5.2),
deterministic Pod/workload/Node/storage health with cited evidence (M5.3, see
[HEALTH.md](HEALTH.md)), Explain 2.0 reusing that health evidence plus
verified-ownership workload→Pod correlation and descriptive metrics (M5.4, see
[EXPLAIN.md](EXPLAIN.md)), a bounded UID-keyed Timeline with correct
relist-diff semantics (M5.5, see [TIMELINE.md](TIMELINE.md)), and a 12-sequence
combined adversarial pass plus a 75-minute soak with zero failures (M5.6).
Local annotated `m5-accepted` created, never pushed. `CPU`/`MEM` are
default-visible columns; `CPU/R`/`MEM/R`/`CPU/L`/`MEM/L`/`QOS`/`CPU/C`/`MEM/C`/
`CPU/%R`/`MEM/%R`/`CPU/%L`/`MEM/%L` are wide-only. M6.0 is now in progress
(current continuation above). See [M5_ACCEPTANCE.md](M5_ACCEPTANCE.md).
M4.2b attach accepted 2026-09-16: confirmed it needed no new terminal mechanics,
only call-site plumbing reusing M4.2's guard/forwarding loop exactly. Found and
fixed a real hang on the first live test: attaching to an already-running
process that never reads stdin left the loop waiting forever for output that
would never stop (Ctrl-D, which works for a real shell, does nothing here).
Fixed with a local-only `Ctrl-]` detach key that never signals the remote --
confirmed live via `kubectl get pod`/`logs` that detaching left the container
running, completely unaffected. Live-tested: rejection of a non-interactive
container, real output from an already-running process, the detach key, the
target Pod force-deleted mid-session, and a final quit with exact stty state
preserved. See docs/EXEC.md and HANDBOOK for the full writeup.
M4.2 interactive shell accepted 2026-09-16, built in the requested order (terminal
guard designed and live-verified first, shell built on top second): live-tested
against `kind-sauron-test` across all nine requested endings (clean exit,
nonexistent shell path, Pod force-deleted mid-session, cluster network
disconnected mid-session, repeated resize, Ctrl-C forwarded to remote, Ctrl-D,
eight repeated round-trips, final quit) -- exact `stty` state preserved in every
case. Found and fixed a real crash on the first live shell-exit test
(`Terminal::clear()`'s internal cursor-position query racing the session's own
stdin reader and timing out; fixed via `Terminal::resize()` instead, which needs
no cursor query for a Fullscreen viewport) and a real, byte-traced bug (an
uncancellable `tokio::io::Stdin` read can occasionally swallow one
subsequently-typed command right after a session ends; mitigated to a safe
no-op/retry, documented as a bounded limitation rather than claimed fully fixed)
-- see docs/EXEC.md and HANDBOOK for both root-cause writeups.
M4.2 one-shot exec accepted 2026-09-16: `--readonly` CLI flag wired for the first
time (was previously parsed and never read -- see HANDBOOK); new `kube::exec`
live-verified against `kind-sauron-test`: readonly denial before any connection,
`--readonly` override forcing denial against a `readonly = false` config, explicit
container success, unknown/missing container errors, a real nonexistent-executable
failure, non-zero exit code, Pod delete+recreate correctly clearing the stale
selection, Refresh restarting under a fresh identity, 32x9, exact stty restoration.
Found and fixed a real command-grammar bug along the way: `:exec`'s argv containing
an absolute path (e.g. `/bin/sh`) collided with the unrelated `resource / filter`
boundary syntax and silently lost the command -- see docs/EXEC.md and HANDBOOK.
Attach was subsequently implemented and accepted in M4.2b (above).
M4.1 accepted 2026-09-16: `python3 scripts/accept-m4.py logs` PASS twice in a row
against `m4-fixtures` (container choice/init/ephemeral/multi-source/pause/search/
filter/clear/restart/bounded eviction/same-name replacement/32x9/terminal). Two
test-harness timing/assertion bugs found and fixed along the way (fixture-count
drift, an ephemeral-container startup race) -- neither was an application bug; see
HANDBOOK journal. Reproducible fixtures: `bash scripts/test-cluster.sh m4-fixtures`;
`python3 scripts/accept-m4.py logs`. The latter deletes/recreates only named
m4-sessions and adds its ephemeral fixture, always through guarded test-cluster.sh.
Never substitute a production config.
`python3 scripts/accept-m4.py` rebuilds before launching a unique tmux session and
validates follow/previous, ownership/cancel/rapid scope changes, palette race replay,
32x9, zero sessions after close and exact stty restoration. PASS 2026-09-16.
The isolated kind container was absent. Recreated via `bash scripts/bootstrap-test-cluster.sh`
with explicit `.test-cluster/config`, loopback API and cached v1.33.1 image; node Ready
and script identity checks pass. Previous dedicated kubeconfig backed up privately under
ignored `.test-cluster/previous.*`. Fixtures restored and identity check passes.
The following M3 details are historical evidence, not the current M4 test-cluster state.

## M3 accepted checkpoint and historical evidence

M2 is ACCEPTED: annotated local `m2-accepted` at `675567d940d0cdb7ad8e5c7ae2e95d4d3de5e435`,
verified before M3; worktree initially clean. All 6 M3 items are ACCEPTED (filters,
typed stable sorting, shared document viewer, Events, CRD printer columns, combined
adversarial live acceptance). **M3 is ACCEPTED** — see item 6 below and local
annotated `m3-accepted`.
Acceptance ledger/order: [M3_ACCEPTANCE.md](M3_ACCEPTANCE.md). M3 was entirely read-only.
At the filter/document checkpoint fmt/check/clippy were clean, 49 unit + 4 fake-HTTP tests passed. Fake HTTP needed
loopback permission outside sandbox. `cargo build --locked` followed by
`python3 scripts/accept-m3.py filters` PASS, including exact replay of the stale-error
regex repair bug and rapid scope changes. Full evidence in HANDBOOK journal.
New filter language: FILTERS.md. `po` with colliding Portal CRD is now ambiguous by
current AGENTS policy; use `pods` or `v1/pods`. Historical M2 alias behavior below is
superseded. M3-wide acceptance and its tag were subsequently completed (item 6).
`python3 scripts/accept-m3.py sorting` passed after full checks and fresh build. This
flow applies/patches/deletes ONLY named
`m3-sort-*` ConfigMaps in isolated `sauron-fixtures`, through `test-cluster.sh` identity
verification on every operation. It recreates the deleted fixture, never touches production.

Item 3 (shared document viewer, `src/app/document.rs`) is done: one model backs Yaml/
Describe/Explain/Events/help/logs, with grapheme-aware wrap/layout, bounded search,
horizontal scroll (wrap off only), a `Freshness` status line, and Refresh reusing the
existing UID-pin check so a deleted or same-name-replaced object shows a clear
"NOT CURRENT" error rather than stale or wrong content. Opening the palette from a
document now restores it afterward instead of discarding it. Picked up with 12/49
tests failing on "Unsupported key chord" (the new `ScrollLeft`/`ScrollRight` keys used
`left`/`right`, which `parse_key` didn't recognize, breaking `Keymap::compile` and thus
every test constructing a `State`) — fixed by name. Live testing then found a second
bug: a per-action validation error was written to the same field a real transport error
uses, so it never cleared on a later successful key press, masking the document's own
status line — fixed by routing it through the existing `input_error` field instead.
Full live checklist (nav, horizontal scroll, search incl. no-match, update/delete/
same-name-replace-then-refresh, palette-over-document, 32x9, fullscreen, logs, clean
exit) passed by hand against `kind-sauron-test`; case-by-case evidence in
`M3_ACCEPTANCE.md`. Document contract: [DOCUMENTS.md](DOCUMENTS.md). 49/49 tests
passing.

Item 4 (Events) is done: a Warning-only toggle (`W` / `:toggle_warnings`, scoped to
`document::Source.warning_only`, re-fetches through the normal Refresh/UID-pin path)
and `involvedObject.fieldPath` display (previously read nowhere) added to the existing
UID-correlated Events view. Three new fake-HTTP tests (403 with secret redaction still
intact, `continue`-token PARTIAL notice, mixed Normal/Warning + toggle + timestamp
fallback). Investigated but did NOT confirm a suspected null-timestamp bug: traced
`k8s-openapi`'s `Event` `Serialize` impl directly and found every optional field is
omitted (never emitted as JSON `null`) when unset, so the theorized failure mode
cannot occur via the real API round-trip — hardened the fallback chain defensively
anyway (free, strictly not worse) but recorded honestly as a non-bug, not a fixed live
one. Live: mixed real events with fieldPath and the toggle; a CRD with zero events;
refresh; delete-then-refresh (NOT CURRENT); context switch + back/forward (correctly
returns to the table, no leaked Events/toggle state); same-name Pod recreation with a
new UID showing fresh events, never stale ones. Related-object navigation explicitly
deferred (fits the ledger's own escape clause: every Event already correlates to the
one selected object via the server-side UID filter). A fake-timeout test for
`events()` was not added — acknowledged gap, not silently skipped; the mechanism is
the same per-call timeout wrapper already exercised elsewhere. 49 unit + 7 fake-HTTP
tests passing.

Item 5 (CRD printer columns) is done. Researched server Table conversion first, per
the plan: `kube`/`k8s-openapi` have no typed support for it, and more fundamentally
it's a one-shot snapshot with no JSONPath a live object could be re-evaluated
against, so it can't drive a continuously-live table without polling or a second
mechanism anyway. Implemented CRD `additionalPrinterColumns` directly instead (new
`src/kube/printer.rs`): one bounded GET per resource view (skipped entirely for
empty-group/core resources), a deliberately narrow JSONPath-subset parser (plain
dotted fields + simple numeric array indices; anything else is omitted, not guessed
at), reusing the filter language's own `Field`/`Scalar` machinery to evaluate each
column live against every watched object. Spawned alongside the watch, epoch-gated
like every other async result. Merges additively into existing curated/generic
columns; priority-1+ columns reuse the existing Wide toggle, no new key. Extended the
`eyes.testing.sauron.local` fixture (whose schema had unused numeric/bool fields
already) with a full type spread including a missing field and a wide-only column.
Live: numeric/bool/missing/priority/update(via kubectl patch)/CRD-removal-while-
viewing/narrow-terminal/rapid-resource-switch all verified against `kind-sauron-test`;
cross-validated against `kubectl get` showing the identical columns from the same
CRD. No live bug found this pass. 53 unit + 10 fake-HTTP tests passing. Full
case-by-case evidence in `M3_ACCEPTANCE.md`.

Item 6 (combined adversarial live acceptance) is done. Ran genuinely combined
sequences against `kind-sauron-test`/`kind-sauron-test-b`: CRD printer columns →
typed filter → sort → YAML → search → back → Warning-only Events → context/
namespace switch → back/forward → refresh; invalid-regex-repair across a context
switch; server+local selectors surviving Refresh/all-namespaces/concrete-namespace;
document update+refresh+delete showing an explicit NOT CURRENT/404 rather than a
crash; CRD same-name replacement with no leftover cells; rapid resource switching
with no stale columns/rows; a 32x9 terminal through breadcrumb/palette/document
transitions; filter+sort+history+Refresh interleaved. Found and fixed one real bug
under rapid context/namespace/resource churn with zero settle time: `navigate()`
and `switch_namespace()` both errored "Not connected" during the brief window
right after a context switch (before its `Payload::Connected` lands), and the
palette's reopen-on-error preserved that stale text, silently absorbing every
subsequent keystroke as an edit to it instead of a fresh command — confirmed live
by capturing the buffer mid-burst as three commands glued into one garbled string.
Fixed by queuing (`navigate()`, reusing the existing `self.pending` mechanism) or
ignoring the transient error (`switch_namespace()`, since `Payload::Connected`'s
`rewatch()` fallback already re-applies the namespace set beforehand) instead of
erroring. Two new regression tests. Full case-by-case evidence and the bug
write-up are in `M3_ACCEPTANCE.md`. 57/57 tests passing (55 unit incl. 2 new
regressions, 10 fake-HTTP, 1 live-cluster test correctly ignored).

**M3 is ACCEPTED.** Tagged locally as `m3-accepted` (same pattern as `m1-accepted`/
`m2-accepted`, not pushed anywhere).

## Historical M1 checkpoint

M1 core is implemented AND interactively accepted against the isolated cluster: native
connection/discovery/watch, staged UID-aware store, Pod and generic tables, command
namespace/generic-resource switching, filter AST, sorting, redacted YAML, contextual
descriptions, UID-related Events/Explain, and Pod logs. The live test cluster and fixture
resources exist. No full Sofka parity claim.

Tagged locally as `m1-accepted` (not pushed anywhere) as the checkpoint to diff/reset
against once M2 work starts: context picker, fuller namespace navigation, generic
resources/CRD polish, aliases, history/breadcrumbs, command palette.

Completed checks (all green as of 2026-09-15):

- `cargo fmt --check` clean.
- `cargo check --all-targets` clean.
- `cargo clippy --all-targets -- -D warnings` clean (0 lints; the original 8 are fixed).
- `cargo test --all-targets`: 23/23 passing — 20 unit tests, 3 fake-HTTP integration tests
  in `tests/watch_transport.rs` (paged list+watch relist, bounded-channel cancellation,
  403 error surfacing without leaking credentials, partial discovery on a forbidden group).
  `tests/cluster.rs` (live acceptance) now passes for real against `kind-sauron-test`.
- Fixed two `prepare()` memoization bugs, both in the HANDBOOK journal:
  1. the clock was part of the cache key unconditionally, forcing a resort every tick
     regardless of data/filter/sort changes — fixed by ticking only when the filter has an
     `age` comparison, the only predicate whose membership can change from time alone.
  2. that fix exposed that `store.revision` resets to 0 on every reconnect/refresh and can
     alias a previous watch's revision, silently skipping a required rebuild and leaving
     the table permanently empty after Refresh — fixed by adding `epoch` (never resets) to
     the key alongside `revision`.
  Covered by 4 regression tests total.
- `benches/pipeline.rs` runs and prints real (debug-profile, single-sample) numbers over
  100/1,000/5,000 synthetic objects; no release-profile or repeated-run data yet.
- Local `sauron-test` fixture node Ready; healthy Pod Running; failure fixtures present.

Interactive TUI acceptance against the live isolated cluster is now done and recorded
(2026-09-15): live watch table, namespace switching, generic/CRD resource resolution via
shortname, text filter, redacted YAML, Explain, Events, Pod log follow, narrow-terminal
resize, and clean terminal restore on quit were all directly observed over tmux against
`kind-sauron-test`. `bash scripts/test-cluster.sh test` now passes for real (previously
only unit/fake-HTTP evidence existed). Full details and the second bug found while doing
this (`prepare()`'s cache key aliased revision numbers across reconnects, leaving the
table permanently empty after a Refresh) are in the HANDBOOK journal.

Also observed (2026-09-15, same session): Pod `--previous` logs, explicit-container log
selection (`:logs worker`), `:ctx` switching (tested by temporarily adding a second
context to the isolated kubeconfig, removed immediately after), all-namespaces mode
(`0`, 13 real Pods across 3 namespaces with a NAMESPACE column), and a ~150s continuous
watch soak (AGE advancing correctly, ~25MB RSS, ~2% CPU, no crash, clean terminal restore).
Found and fixed a third bug this way: `open_logs` never reset `state.status`, so a new
log stream could display stale "Log stream ended" text from a previous, unrelated action
while actively streaming live lines. Fixed in `src/app/mod.rs`.

Remaining gap: the soak was ~150s, not multi-hour/overnight; only one namespace/context
combination was exercised per feature. Update this checkpoint after every verification.
Never re-mark ACCEPTED from unit tests alone — only from a directly observed session
against the isolated cluster.

## M2 checkpoint

Order and adversarial case list for M2 are in the HANDBOOK ("M2 plan and discipline").
Item 1 (real context picker + namespace-per-context memory) is done: `:ctx` opens a
navigable list instead of static text, and switching context restores that context's
last-viewed namespace (including all-namespaces) instead of always resetting to the
kubeconfig default. Stress-tested against rapid repeated context switching (three
rounds of 8 back-to-back switches, ten rounds of reopen-and-switch, no settling time) —
no stale-epoch leak. `scripts/test-cluster.sh fixtures` now also creates a second
context alias (`kind-sauron-test-b`) on the same isolated cluster so this is
reproducible for future M2 work, not a one-off. 25/25 tests passing (2 new).

Item 2 (full namespace navigation) is also done: `n` / bare `:ns` open a real picker
fetching the actual namespace list from the cluster (bounded to 500), `<all>` always
first, a session-local MRU of recently-visited namespaces (capped 5, reset on context
switch) next, then the rest alphabetically. `:ns NAME`/`:ns *`/`0` still switch directly.
Stress-tested `namespace A → all → namespace B → all` and rapid repeated `:ns` switching
(three rounds, five back-to-back commands each, no settling time) — always converged
correctly. Found and fixed a fourth bug this way: `Mode::Loading` had no dedicated key
handling and fell through to table-action dispatch, so a key pressed while the namespace
list (or any fetch) was in flight could fire an unrelated action against a stale
selection. Fixed by giving `Mode::Loading` its own arm where only Esc acts. Full account,
including the false-negative re-test caused by testing against a stale tmux pane instead
of a freshly restarted process, is in the HANDBOOK journal. 25/25 tests passing.

Item 3 (generic resources/CRDs/aliases) is also done. `Runtime::watch()` no longer
re-resolves `state.query.resource` on every Refresh/namespace switch — it reuses the
already-resolved `Resource` (canonical GVK); only a context switch (`rewatch()`) or an
explicit new navigation re-resolves by name. `Catalog::resolve` no longer silently
prefers a core match or picks a shortname arbitrarily: any genuine cross-group
ambiguity (plural, kind, or shortname) now errors with the `plural.group` options
listed, and the 12 built-in aliases are matched case-insensitively (found live: they
weren't, letting `PO` fall through to real ambiguity that `po` never hit — see
HANDBOOK). `scripts/test-cluster.sh fixtures` now also applies
`tests/fixtures/ambiguous.yaml`/`ambiguous-instances.yaml`: a CRD shortname colliding
with a built-in, two CRDs sharing a plural/kind in different groups, and a
cluster-scoped CRD. Verified live: `:po`/`:pods` identical, ambiguous `:widgets`
rejected with both options listed, `plural.group` disambiguates, cluster-scoped
`:probes` shows no NAMESPACE column and "n/a (cluster-scoped)", the shortname-collision
warning is visible via `:info`, deleting a CRD mid-watch or navigating to one already
deleted both produce a clean error (not a crash), and rapid resource switching
(`pods`/`eyes`/`probes`/`widgets.a.sauron.test`, zero settle time, three rounds) always
converged correctly. Also fixed the header/table-title UI to show the resolved
canonical name instead of the raw typed alias. 29/29 tests passing (6 new).
`pods → CRD → deployments → back → forward` restoring resource+namespace+context+
selection is explicitly deferred to item 4 — there is no resource-navigation history
yet. A suspected crash chased at length during this item turned out to be intentional
`Esc`-at-root-quits behavior (see AGENTS.md), not a bug; no code change from it.

Item 4 (navigation history/breadcrumbs) is also done. `state::HistoryEntry` stores
semantic intent only — context, namespace, canonical resource name, filter, sort,
selected UID — never store/rows data. Two `VecDeque` back/forward stacks on `Runtime`,
capped at a fixed `HISTORY_LIMIT = 100` from the start. Pushed only at `Command::Resource`,
`switch_namespace`, `switch_context` (Refresh/sort/filter/document-view never push, by
construction, not a special case). New `[`/`]` keys walk the stacks and re-resolve/
rewatch, never reviving old rows. Header replaced with a compact breadcrumb: `ctx:X ›
ns:Y › resource` (no `ns:` segment for cluster-scoped), always the canonical name.

Verified live: `pods → eyes → deployments → back → back → forward → forward` walks
correctly; back after a namespace or context change restores correctly; `<all>` vs a
concrete namespace round-trips; opening/closing a document doesn't touch history;
several Refreshes then one Back returns directly to the prior view; going back to a
CRD deleted in the meantime errors cleanly, not a crash; 15 rapid `[`/`]` presses past
the real stack depth stop cleanly at the boundary; a narrow-terminal breadcrumb clips
instead of panicking. Found and fixed a real bug: restoring history updated the
canonical `state.resource` but not the text field `state.query.resource` that a later
context switch re-resolves — a Back to `pods` followed by a context switch landed on
the previous resource (`eyes`) instead of `pods` on the new context. Fixed in
`finish_history`. Also spent time on what looked like a same-name-different-UID
selection bug; it was confusion from an extremely long single test session, not a real
bug — a clean isolated re-run with temporary instrumentation confirmed `rebuild()`
already clears a stale-UID selection correctly. 31/31 tests passing (2 new).

Item 5 (command palette unified on the action registry) is also done. Removed two
separate, drifting name->behavior tables (`parse`'s hardcoded action match, which only
covered 7 of ~30 registered actions, and the `COMMANDS` autocomplete list) in favor of
one lookup against `registry()` for both. Found a real name collision this surfaced:
`sort`/`logs`/`previous_logs` were each both a zero-arg key action and an
argument-taking command sharing the same name with different behavior — unified so
bare `:name` is always the action, an explicit argument takes the special path. Also
replaced a hardcoded hint-bar string in `src/ui/mod.rs` (independent of the actual
keymap) with `hint_bar()` built from `Keymap::primary_key`, verified live against a
real config override. Added a dispatch-time mode gate so a palette-typed action
respects the same mode-scoping a key binding already has, instead of silently
no-op-ing. Verified live: rapid palette open/close, fuzzy search, Tab-complete,
identical result via key vs. palette, "select a row first" on a selection-requiring
action with none, scope-change-then-reopen, narrow-terminal clipping. One non-bug
documented rather than fixed: opening the palette from a document always returns to
the table first (same existing pattern as Esc/Back), so a document-scoped action can
never actually be typed by name while "in" a document. Two new regression tests
assert every registered action is reachable by name and every suggested name is
registered. 34/34 tests passing.

Item 6 (whole-of-M2 combined acceptance) is done. Ran items 1-5 together against
`kind-sauron-test`/`kind-sauron-test-b`: context→namespace→CRD→filter→back→context→
forward; palette open + rapid scope change + immediate action; key remap agreeing
across hint bar/help/key/name execution; ambiguous alias→qualified→context switch→
back; `<all>`→namespace→picker→context→back/forward; a deleted CRD sitting in history
then navigated back into (clean 404); context switch from the picker during an active
watch; three rounds of combined `:ns`+back+`:ctx`+back+forward at zero settle time; a
32x9 terminal with picker/palette/long-breadcrumb; Refresh interleaved with complex
navigation, confirmed not to pollute history. No new bugs found — items 1-5 compose
correctly under combined adversarial use. 34/34 tests passing.

**M2 is ACCEPTED.** Tagged locally as `m2-accepted` (same pattern as `m1-accepted`,
not pushed anywhere). The following milestone is now defined in M3_ACCEPTANCE.md.

## Tools

Rust/cargo 1.95 from system distribution. Matching rustfmt and Clippy installed in
`~/.local/opt/sauron-rust-tools`, with links in `~/.local/bin` already on PATH.
User authorized installing required development tools. sudo requires a password; user-local
installation worked without privileged access. Docker, kind, kubectl and tmux are available.

## Validation commands

```sh
cargo fmt --check
cargo check --locked --all-targets
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked --all-targets
bash scripts/test-cluster.sh check
bash scripts/test-cluster.sh test
```

Fixture setup is explicit and restricted by the script:

```sh
bash scripts/test-cluster.sh fixtures
```

Run the application against the isolated cluster:

```sh
cargo run -- --kubeconfig /home/ximbi/SAURON/.test-cluster/config \
  --context kind-sauron-test pods -n sauron-fixtures --readonly
cargo run -- --kubeconfig /home/ximbi/SAURON/.test-cluster/config \
  --context kind-sauron-test pods -n sauron-fixtures --snapshot
cargo run -- info --offline
```

The test kubeconfig is private local state. Never commit it or print its raw contents.
The kind cluster remains running for continued development; no automatic teardown runs.
