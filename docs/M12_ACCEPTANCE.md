# M12 — Plugins/providers/headless maturity/packaging/performance: acceptance ledger

Status: **PLANNED — NOT STARTED**. This document is M12.0: the scope-freeze
contract written before any M12 implementation code, exactly like
`docs/M11_ACCEPTANCE.md` was for M11. It records what reconnaissance found,
the amended slice ledger, the trust/protocol/output/packaging/performance
contracts, and the exact conditions for `m12-accepted`. Nothing in this
file authorizes touching code, cluster, CI, or publication — only the
slices marked ACCEPTED in the Journal, once this contract exists,
authorize implementation work.

`docs/M9B_A_ACCEPTANCE.md`/`docs/M9B_B_ACCEPTANCE.md` remain separate,
untouched, PLANNED — NOT STARTED ledgers; M12 does not depend on them.

## Baseline

M1-M11 are ACCEPTED. Local annotated tags `m1-accepted` through
`m11-accepted`; `main` and all milestone tags are pushed to
`origin` (`git@github.com:ximbi1/SAURON.git`) through `m11-accepted` as of
this document's own start. Worktree was clean at `m11-accepted`.

## Purpose

M12 is the final milestone of the current roadmap. Unlike M11 (a pure
composition milestone over existing evidence), M12 has two genuinely new
architectural surfaces — plugin subprocess execution and packaging/release
artifacts — plus maturation of two mostly-already-built surfaces (headless
output, performance methodology) and one still-open investigation
(providers). The core discipline carried over from M11.0: reuse an
accepted primitive wherever one exists; only build new architecture where
reconnaissance proves nothing already covers it, and say so explicitly.

## Reconnaissance: what already exists and is reused verbatim

- **`app::session::{Sessions, Kind, Scope, Outcome, State}`**
  (`src/app/session.rs`, M4-era, unmodified): the exact bounded
  (`MAX_ACTIVE=8`), cancellable, `Drop`-safe (cancels + aborts every child
  task the instant the owner drops — the literal mechanism "no orphan
  process after quit" needs) task-ownership primitive already used for
  Logs/Exec/PortForward sessions. Plugin execution (M12.1) adds
  `Kind::Plugin` and reuses `Sessions::spawn`/`stop`/`join_next`/`shutdown`
  verbatim — no second task-supervision system.
- **`src/kube/logs.rs`'s 16 KiB per-line clip convention** (`MAX_SOURCES`,
  clipped-line marking): the established bounded-output-capture pattern
  plugin stdout/stderr capture reuses, not a new bound invented from
  scratch.
- **`config::{Config, Settings}` + `Config::save`'s atomic write**
  (M10.8, unmodified): the single persisted config surface with
  `deny_unknown_fields`, fail-safe malformed reload, and the only atomic
  (temp-file/`0600`/`rename`) local write this project has. Plugin trust
  config (which commands are approved, with what argv/env policy) extends
  `Config`/`Settings` the same way workspaces/bookmarks did — no second
  config file.
- **`safety::redact`/`safety::text`** (M1-era, unmodified): the one
  global redaction/terminal-control-stripping functions. Any object
  projection handed to a plugin or provider reuses these, never a new
  masking heuristic.
- **`benches/pipeline.rs`** (M4-era, unmodified): a real, working
  reproducible-methodology benchmark (object count sweep,
  build/filter/render timing, median-of-5, debug/release profile
  labeled in its own output). M12.7 extends this harness (more
  measurements) rather than building a second one.
- **`main.rs`'s headless CLI** (`--check`, `--snapshot`, `info`,
  `info --offline`, JSON/YAML/text output, bounded `--timeout`,
  `PARTIAL DISCOVERY`/`FILTER UNKNOWN` explicit stderr diagnostics,
  `std::process::exit(1)` on error): already P0 **TESTED** in
  `docs/SOFKA_PARITY.md` ("No TTY needed"). M12.4 hardens this (explicit
  JSON schema version, confirms exit-code/no-ANSI/no-prompt guarantees
  already hold) rather than rebuilding it.
- **`brand::{NAME, VERSION, BINARY}`** (existing): already the single
  source of version/name metadata `Cli`'s own `--version` and the
  `info --offline` banner use; packaging artifact naming and the license/
  checksum manifest reuse these, not a second version constant.

### Genuine new architecture (not composition)

Confirmed via `grep -rn "std::process::Command\|tokio::process::Command" src/`
returning **zero matches**: this codebase has never spawned a local child
process before. `kube::exec`/`kube::shell` proxy through the Kubernetes
exec WebSocket API, not a local subprocess. Plugin execution (M12.1) is
therefore genuinely new architecture, not composition — expected and
appropriate for this specific slice, unlike M11's evidence lenses. It
still reuses `Sessions` for lifecycle/cancellation, so it is "new" only in
the narrow sense of "first local subprocess boundary," not in task
ownership.

Packaging (M12.5) and license/checksum tooling (M12.6) are also genuinely
new (no CI release job, no cross-compilation setup, no license-inventory
tooling exist yet) — confirmed via `.github/workflows/ci.yml` (a single
`ubuntu-latest` job: fmt/check/clippy/test, no release/build-matrix job)
and an empty search for any existing SBOM/license/checksum script.

### Reconnaissance-driven ledger amendment (evidence-backed, not a fork)

`docs/SOFKA_PARITY.md`'s own priority ledger (reconciled through M11) shows:

| Row | Priority | Status |
| --- | --- | --- |
| `Headless \| --check, --snapshot, info, info --offline` | P0 | **TESTED** |
| `Performance \| Published startup/filter/view/RSS methodology` | P0 | **TESTED (subset)** — "startup/large-cluster campaign remains" |
| `Distribution \| Linux/macOS x86_64/aarch64, Cargo/Homebrew/Nix` | P2 | DEFERRED |
| `Providers \| Prometheus/VictoriaMetrics rightsize preview` | P3 | DEFERRED |
| `Providers \| VictoriaLogs autodiscovery/history/tail/field detection` | P3 | DEFERRED |
| `Plugins \| ` (all 5 rows: inline commands, package commands, activity panel, catalog install/update/rollback, bundled/external tool integration) | P3 | **DEFERRED (5/5)** |

`docs/ROADMAP.md`'s own M12 acceptance line — "Process limits/trust; four
platform builds; checksums/license inventory; reproducible performance
campaign" — requires the plugin **trust/process boundary**, packaging, and
performance explicitly, but does **not** name a full plugin protocol/
catalog or providers as acceptance-gating, mirroring exactly the pattern
M11.0 already found for Context Diff (named in the milestone's title/scope,
not singled out in the one-line acceptance contract).

This is the same class of "reconnaissance proves a cleaner division, amend
and document rather than silently follow the kickoff's suggested shape"
case M11.0 already established as pre-authorized (not opaque scoring, not
a controversial identity rule, not new sensitive-data disclosure, not a
predicted-impact claim, not weakening an invariant, not contradicting the
roadmap's own acceptance line). **Amended scope**:

- **M12.1 (plugin execution/trust foundation) ships in full** — it is the
  literal "process limits/trust" the roadmap requires.
- **M12.2 (plugin protocol) ships at the smallest safe shape**: one
  approved external command, invoked with an explicit argv and a bounded,
  redacted JSON projection on stdin, a bounded JSON/text result on stdout —
  never a catalog, marketplace, install/update/rollback, or bundled
  third-party tool integration (all five of those are the P3 DEFERRED rows
  above, confirmed out of scope with evidence, not silently dropped).
- **M12.3 (providers) is investigated, not assumed** — given every
  provider row is P3 DEFERRED and providers are not named in the roadmap's
  own acceptance line, the working assumption entering that slice is
  "defer with evidence unless investigation finds a genuinely small,
  clean win," mirroring the exact discipline M11.6 already used for
  context diff (which turned out feasible) and M9.6 already used for Helm
  rollback (which turned out infeasible) — the outcome is decided by
  investigation, not by this paragraph.
- **M12.4 (headless) is a hardening pass**, not a rebuild — P0 already
  TESTED.
- **M12.5 (packaging)**: per explicit user decision (asked directly,
  since this repository/machine has no rustup, no cross-compilation
  linker, and no macOS hardware — a real "cannot honestly prove without
  available infrastructure" fork, stop-condition #5): **define a real CI
  build matrix (GitHub Actions, native runners per platform) and document
  it fully, but do not push/trigger it this session.** Linux x86_64 gets a
  genuine local build + full acceptance; the other three platforms are
  honestly documented as CI-defined and not yet executed. No four-platform
  build claim is made without CI execution evidence — the acceptance
  checklist reflects this exactly.
- **M12.6 (checksums/license inventory) ships in full** for whatever
  artifacts M12.5 actually produces locally (Linux x86_64), with the
  manifest/tooling structured to cover all four once CI executes.
- **M12.7 (performance) extends `benches/pipeline.rs`** rather than
  building a second harness, closing the two gaps
  `docs/SOFKA_PARITY.md` itself names: startup measurement and a
  larger-object-count campaign.

## Amended slice ledger

| Slice | Scope | Status |
| --- | --- | --- |
| M12.0 | Acceptance contract / architecture freeze (this document) | ACCEPTED |
| M12.1 | Plugin execution foundation: subprocess trust/process-group/bounds, reusing `app::session::Sessions` | ACCEPTED |
| M12.2 | Plugin protocol (smallest safe shape) + command registry wiring | ACCEPTED |
| M12.3 | Providers — investigated; evidence-backed DEFERRED | DEFERRED |
| M12.4 | Headless maturity — hardening pass (schema version, exit-code/output contract confirmed) | ACCEPTED |
| M12.5 | Packaging — CI build matrix defined (4 platforms); Linux x86_64 built+proven locally | ACCEPTED |
| M12.6 | Checksums / license inventory / release manifest | ACCEPTED |
| M12.7 | Performance — extend `benches/pipeline.rs`; startup + large-object-count campaign | PLANNED |
| M12.8 | Combined acceptance / full M1-M11 regression / soak / docs / final tag | PLANNED |

## Explicit non-goals for M12

- No plugin catalog, marketplace, install/update/rollback, or bundled
  third-party tool (Popeye/Trivy) integration — all five confirmed P3
  DEFERRED in `docs/SOFKA_PARITY.md`, not silently dropped.
- No plugin mutation capability. Plugins are read-only augmentation in
  M12; if a plugin ever needs to request a mutation, that requires a
  typed intent routed through the unmodified M7/M8/M8B/M10 gateway, which
  is out of scope here — deferred, not designed half-safely.
- No credential of any kind (kubeconfig path/content, bearer token,
  exec-plugin credential output, raw Secret values) is ever passed to a
  plugin or provider by default.
- No implicit shell (`sh -c`, `bash -c`, `eval`, shell interpolation)
  anywhere in plugin execution. Explicit executable + argv only.
- No four-platform build *claim* without CI execution evidence (see the
  ledger amendment above) — packaging accepts at the honestly-proven
  scope.
- No publication, release, or CI trigger without explicit authorization
  beyond what was already given for `main`/tags.
- No provider evidence is ever presented as if it were native Kubernetes
  truth — `PROVIDER EVIDENCE != KUBERNETES TRUTH`, provenance-labeled
  always, augmenting native Health/Metrics/Explain/Timeline, never
  replacing them.
- No comparative ("faster than X") performance claim — only measured,
  reproducible, methodology-documented numbers for this codebase.
- No second task-supervision system, config file, redaction function, or
  benchmark harness — every M12 slice composes an existing one.

## Invariants (inherited from M1-M11, unmodified, must survive M12)

`UNKNOWN != ZERO != HEALTHY` · `RELATIONSHIP != CAUSE` ·
`VERIFIED REFERENCE != SELECTOR-DERIVED RELATIONSHIP` ·
`COMMIT != OBSERVED EFFECT` · `REQUEST ACCEPTED != DESIRED EFFECT OBSERVED`
· `UID != NAME` · `LOOKS CORRECT != PROVEN CORRECT` ·
`AGGREGATION != NEW INTERPRETATION` · `RELATIONSHIP != GUARANTEED IMPACT`.
New for M12, stated once here and enforced identically to the above:
`EXTENSIBILITY != UNBOUNDED TRUST` · `DISTRIBUTABLE != PROVEN PORTABLE` ·
`FAST ONCE != REPRODUCIBLY FAST` · `HEADLESS != HIDDEN UI AUTOMATION` ·
`PROVIDER EVIDENCE != KUBERNETES TRUTH`.

No name-prefix heuristics. No silent scope widening. No unbounded fanout.
No hidden retries. No API requests from rendering. No runtime shell-out to
kubectl/helm/flux/argocd. No credential leakage. No Secret body disclosure
outside the one already-accepted M9.5 Helm-Secret reader. Production
remains read-only, forever.

## Safety boundary

Identical to every prior milestone. Production cluster: reads only,
forever. All new live/write-adjacent testing happens against the two
existing guarded isolated kind clusters (`sauron-test`/`kind-sauron-test`,
`sauron-m9`/`kind-sauron-m9`). No default kubeconfig for fixture writes.
Never print/commit kubeconfig or credential material. Never push/publish
without explicit authorization (already given, narrowly, for `main` and
milestone tags through `m11-accepted` — that authorization does not
extend to a new CI-triggering push for M12's own packaging matrix; see the
ledger amendment above). Local annotated tag `m12-accepted` only.

## Trust model (M12.1/M12.2)

A plugin is: an explicit, user-configured, **approved** executable +
fixed argv template, never an ad-hoc shell string. Config shape (extends
`Settings`, same file, same atomic write, `deny_unknown_fields`):

```
[plugins.<name>]
executable = "/absolute/or/PATH-resolved/binary"
args = ["fixed", "argv", "entries"]
trust = "approved"   # or "disabled" -- never a bare boolean
timeout_secs = 10     # bounded, explicit
```

Trust states: `approved` (may run) or `disabled` (configured but inert;
the default for anything not explicitly `approved`). There is no
`untrusted-but-runs-with-a-warning` state — a plugin either has been
explicitly approved by editing the config file (the same trust gesture
`readonly = false` already requires for mutations) or it does not run at
all. No discovery-by-directory-scan that auto-executes anything.

Environment: the child process gets an **explicit allowlist only**
(`PATH`, `HOME`, `LANG` — never the parent's full environment, never
`KUBECONFIG`, never any token/credential variable). No kubeconfig path,
content, bearer token, exec-plugin credential output, or raw Secret value
is ever passed via argv, env, or stdin by default. Working directory is
fixed (the SAUR-ON config directory or an explicit configured path), never
the shell's own CWD.

## Plugin protocol (M12.2, smallest safe shape)

stdin: one bounded JSON object — canonical target identity
(kind/namespace/name/UID), the redacted `Health` projection
(status/severity/evidence), and nothing else by default (no raw object
body, no Secret-adjacent field, no credential). stdout: bounded (same 16
KiB-per-line clip convention `kube::logs` already uses) text or a small
JSON result; anything else is a `Failed` outcome, never crashes SAUR-ON.
stderr: captured, bounded, shown as diagnostics, never parsed as a
result. Exit code: non-zero is an explicit failure outcome, never
silently retried.

## Provider trust/network model (M12.3, pending investigation outcome)

If M12.3 ships rather than defers: opt-in only, explicit endpoint (no
auto-discovery), bounded request with its own timeout, cancellable,
explicit freshness/provenance on every value, partial/unavailable stays
explicit (never silently zero), no credential sharing with the
Kubernetes connection, no provider result ever overwrites native
Health/Metrics/Explain/Timeline — only augments, clearly labeled.

## Headless output contract (M12.4)

JSON/YAML output (`--output json|yaml`) gains an explicit
`"schemaVersion": 1` field (new, additive, non-breaking for any consumer
that already ignores unknown keys — the existing document shape is
otherwise unchanged). Confirmed-not-rebuilt guarantees (already true,
verified not asserted): stdout carries only data, stderr carries
diagnostics (`PARTIAL DISCOVERY`/`FILTER UNKNOWN`), no ANSI escapes in
headless mode (`crossterm`/`ratatui` are never initialized on the
`--check`/`--snapshot`/`info` paths), no interactive prompt, bounded
`--timeout`, deterministic row ordering (the same stable sort every table
render already uses), explicit non-zero exit via `std::process::exit(1)`
on any error.

## Packaging matrix (M12.5)

| Platform | Rust target | Build proof this milestone |
| --- | --- | --- |
| Linux x86_64 | `x86_64-unknown-linux-gnu` | Local build, full acceptance (native machine) |
| Linux aarch64 | `aarch64-unknown-linux-gnu` | CI-defined (GitHub Actions `ubuntu-latest` + cross linker) — not executed |
| macOS x86_64 | `x86_64-apple-darwin` | CI-defined (GitHub Actions `macos-latest`, Rosetta/native) — not executed |
| macOS aarch64 | `aarch64-apple-darwin` | CI-defined (GitHub Actions `macos-latest` native) — not executed |

Artifact naming: `sauron-<version>-<target>.tar.gz` (or `.zip` where a
platform convention prefers it), containing the binary, a `LICENSE` copy,
and a short `INSTALL.md`. Version embedded from `brand::VERSION`
(`Cargo.toml`'s own `[package].version`, single source of truth, no
duplicate version string). No release is published; artifacts stay local/
CI-only per the safety boundary above.

## Performance methodology (M12.7)

Extends `benches/pipeline.rs`'s existing, already-documented shape
(profile label, object-count sweep, median-of-N, "synthetic objects, no
Kubernetes I/O" honesty). New measurements close the two gaps
`docs/SOFKA_PARITY.md` itself names: process cold-startup time (`info
--offline`, measured end-to-end via `Instant`, not wall-clock `time(1)`,
for determinism across repetitions) and a larger object-count tier
(closing the "large-cluster campaign" gap — bounded by this machine's own
memory, recorded explicitly, never presented as a claim about arbitrary
cluster scale). Recorded alongside every run: hardware (`/proc/cpuinfo`
model, core count), OS/kernel (`uname -a`), Rust version (`rustc
--version`), build profile (debug/release, already labeled by the
harness), repetitions and statistic used (median-of-N, already
established). No comparative claim against any other tool. Any
optimization made as a result of a measured bottleneck records its own
before/after numbers in the Journal — no optimization without a measured
reason.

## Bug discipline (unchanged from M10/M11, restated because both proved why it matters)

For every failure: reproduce → classify app/harness/environment/
platform/toolchain → root-cause → fix the correct layer → add a
regression test when reasonable → `fmt`/`check`/`clippy`/`test` →
rebuild → replay the exact failing flow → document in this file's
Journal → continue. Never patch the app to compensate for a stale
binary, a CI runner quirk, a broken cross toolchain, a provider outage,
or a packaging-script mistake. Never patch the harness to hide a real
SAUR-ON defect. M10's `startup_warning` finding and M11's
`accept-m9.py` rollback-revision finding are both restated here as the
concrete precedent this discipline is built from.

## Bugs / limitations (placeholder)

None yet — implementation has not started. Updated per slice.

## Journal

- 2026-09-22: M12.0 (acceptance contract / architecture freeze) written.
  Reconnaissance covered `HANDBOOK.md`, `docs/RUNBOOK.md`, `README.md`,
  `docs/ROADMAP.md`, `docs/SOFKA_PARITY.md`, `docs/M11_ACCEPTANCE.md`,
  `docs/RESEARCH.md`, `Cargo.toml`, `.github/workflows/ci.yml`,
  `benches/pipeline.rs`, `src/main.rs` (headless CLI), `src/app/session.rs`
  (`Sessions`/`Kind`/task ownership), `src/kube/logs.rs` (bounded-capture
  convention), `src/config.rs` (`Settings`/`Config`/atomic save),
  `src/safety.rs` (redaction), and a direct grep confirming zero prior
  local-subprocess execution anywhere in `src/`. Confirmed this machine has
  no `rustup`, no cross-compilation linker, and no macOS hardware — asked
  the user directly how to handle the four-platform build requirement
  (stop-condition #5, a genuine "cannot honestly prove without available
  infrastructure" fork) and got an explicit decision: define the CI matrix,
  do not execute it this session. Amended the ledger (M12.2 scoped to the
  smallest safe plugin shape, M12.3 providers entering as an investigation
  with a defer-leaning prior, both evidence-backed from
  `docs/SOFKA_PARITY.md`'s own P3 rows and `docs/ROADMAP.md`'s own
  acceptance line, mirroring M11.0's own precedent for this exact class of
  reconciliation). **Next: M12.1** (plugin execution foundation),
  continuing directly in this session.

- 2026-09-22: M12.1 (plugin execution foundation) implemented. New
  `src/plugin.rs`: `Trust` (`Approved`/`Disabled`, default `Disabled` --
  no "untrusted but runs with a warning" state), `PluginConfig`
  (executable + fixed argv + trust + bounded `timeout_secs`), and
  `run(config, input, cancel) -> Status` (`Completed{exit_code, stdout,
  stderr, truncated flags}` / `TimedOut` / `Cancelled` / `Failed(reason)`).
  `tokio::process::Command`: `env_clear()` + an explicit 3-variable
  allowlist (`PATH`/`HOME`/`LANG`, never the parent's full environment,
  never `KUBECONFIG`/tokens/credentials), fixed `current_dir`
  (`config::directory()`, never the shell's own CWD), `kill_on_drop(true)`,
  and (unix) `process_group(0)` so the child becomes its own process-group
  leader. Cancellation/timeout both call a new `kill_process_group` (added
  `libc` as a unix-only dependency for the one raw `kill(-pid, SIGKILL)`
  syscall neither `std` nor `tokio` expose safely) targeting the whole
  group, not just the direct child -- proven live by a test whose child
  itself forks a grandchild `sleep 30` (`sh -c "sleep 30 & wait"`): a 1s
  timeout kills both, confirmed by wall-clock (`elapsed() < 5s`, never
  blocking on the 30s grandchild). stdout/stderr capture reuses
  `kube::logs`'s own 16 KiB-per-line clip bound exactly, plus a new
  500-line total cap independent of line length, both explicitly marked
  `truncated` rather than silently dropping data. Added `tokio`'s
  `process` feature (previously absent -- confirmed via
  `grep -rn "std::process::Command\|tokio::process::Command" src/`
  returning zero matches before this slice, the literal "genuinely new
  architecture" claim from this document's own reconnaissance section).

  `Settings` gained `pub plugins: BTreeMap<String, PluginConfig>`
  (same `deny_unknown_fields`/fail-safe-reload discipline `keys`/`theme`
  already established), and `Config::resolve()`'s own hand-built base
  `toml::Value` was extended to include it -- a real gap caught before any
  test ran: `resolve()` lists `Settings`'s fields explicitly rather than
  deriving them, so a new field silently would not have flowed through at
  all without this one-line addition (found by reading `resolve()`'s own
  implementation, not by a failing test, a "catch it in review" outcome
  the M10.8 `runtime()` test-helper finding already precedented in this
  project). Two new config tests prove base-level plugin config resolves
  correctly and that a context layer can approve a plugin the base left
  `Disabled` -- and that approval is per-context, never globally implied.

  `app::session::Kind` gained `Plugin` (reusing `Sessions` verbatim, no
  second task-supervision system). A new end-to-end test
  (`dropping_sessions_leaves_no_real_child_process_running`) spawns a real
  `plugin::run(/bin/sleep 30)` through `Sessions::spawn` exactly the way
  `Runtime` will, confirms via `pgrep` that the real OS process is running,
  drops the owning `Sessions`, and confirms via `pgrep` again that it is
  gone -- proving "no orphan process after quit" against the actual
  process table, not just a cancellation-token flag, and proving it holds
  even under `Sessions::drop`'s own `abort_all()` path (which does not run
  `plugin::run`'s cooperative-cancel branch at all -- `kill_on_drop` on
  the owned `Child` is what actually fires here, a stronger and more
  literal proof than the unit-level cooperative-cancellation test alone).

  9 new unit tests in `plugin.rs` (disabled trust never spawns; missing
  executable is an explicit failure, never a panic; stdout/exit-code/
  stdin-transport all correct on a real `cat`; non-zero exit reported
  correctly; process-group timeout kill including a grandchild; explicit
  cancellation; stdout bounded at 500 lines with `truncated` set; a real
  `KUBECONFIG`/fake-token environment variable pair set in the test
  process is confirmed absent from `/usr/bin/env`'s own child output;
  stdout/stderr captured and kept separate). 439 unit + 76 fake-HTTP
  total, fmt/clippy clean. No live-cluster evidence needed for this slice
  (pure OS-process behavior, no Kubernetes I/O) -- live/interactive
  acceptance is M12.2's own job once there is a command surface to drive.

- 2026-09-22: M12.2 (plugin protocol, smallest safe shape) implemented.
  `plugin::projection(&Object) -> serde_json::Value`: `protocolVersion: 1`
  plus canonical target identity (kind/namespace/name/UID) and the
  redacted `Health` projection (status/severity/evidence) -- never
  `object.value` itself, so there is no redaction to trust here, only
  identity/health fields that never had anything sensitive to begin with.
  New `Command::Plugin(String)`/`:plugin NAME` grammar (no default
  keybinding, matching `:bundle`/`:context_diff`'s own precedent for
  argument-taking commands) and `Runtime::open_plugin`: fails fast
  (`Result::Err`, nothing spawned) for an unknown name or a plugin whose
  `trust` is not exactly `Approved` -- proven by three new app-level tests
  asserting `rt.sessions.active_count() == 0` after each rejection.
  `app::session::Kind::Plugin` reuses `Sessions::spawn` exactly like
  Logs/Exec (bounded `MAX_ACTIVE=8`, `Drop`-safe), with the result
  delivered through the *existing* `Payload::Document` event -- no new
  payload variant, matching the bundle/context-diff precedent from M11.

  **Real bug found and fixed live** (the actual reason this slice took
  more than wiring): quitting the real app while a plugin that itself
  forked a background grandchild (`sh -c "sleep 60 & wait"`) was running
  left that grandchild alive afterward, even though M12.1's own unit test
  for "timeout kills a grandchild" had already passed. Root-caused by
  reading `Sessions::drop` closely: it calls `record.cancel.cancel()` and
  then *immediately* `self.tasks.abort_all()`, with no `.await` between
  them -- so the plugin task is aborted before it is ever polled again to
  observe the cancellation flag and run `run()`'s own
  `tokio::select!` cancellation branch (including its `kill_process_group`
  call). When a tokio task is aborted, only in-scope `Drop` impls fire;
  `tokio::process::Child`'s own `kill_on_drop` reaches only the direct
  child, never a process-group grandchild. Fixed by adding a
  `GroupKillGuard` whose own `Drop` unconditionally kills the whole
  process group, held for `run()`'s entire scope so it fires on every
  exit path (normal completion, cooperative cancel/timeout, *and* abrupt
  external abort) rather than relying on any single `tokio::select!`
  branch ever being polled. A new regression test
  (`aborting_the_task_still_kills_the_whole_process_group_not_just_the_
  direct_child`) reproduces the exact failure mode directly -- spawns
  `run()` in its own `tokio::spawn`, confirms the grandchild is really
  running via `pgrep`, calls `handle.abort()` (exactly what
  `JoinSet::abort_all()` does per task, not a cooperative
  `CancellationToken::cancel()`), and confirms via `pgrep` again that
  nothing survives. This test would have failed before the fix and passes
  after it; the earlier cooperative-cancel unit test alone was
  insufficient to catch this because it never exercises the abort path at
  all. Live-replayed the exact original failing scenario against the real
  `kind-sauron-test` cluster afterward (a real long-running plugin quit
  mid-execution): zero orphaned processes, confirmed via `ps aux` before
  and after.

  **Live evidence** against `kind-sauron-test`/`sauron-fixtures` with a
  real scratch plugin config: `:plugin smoke` (an approved shell script)
  ran end-to-end and correctly received the target's kind in its stdin
  projection, with a deliberately-set ambient `KUBECONFIG` environment
  variable confirmed absent from the child's own `env` output; `:plugin
  locked` (configured `trust = "disabled"`) was refused with an explicit,
  visible error and no process spawned; `:plugin` is discoverable via the
  command palette (`:plug` → suggests `plugin`) and appears in `?`'s
  effective help.

  4 new unit tests in `plugin.rs` (the abort-path regression above, plus
  `projection`'s own redaction-boundary test), 4 new app-level tests
  (unknown plugin, disabled plugin, no-selection, and a full
  approved-plugin-runs-end-to-end proof reaping the real session record).
  445 unit + 76 fake-HTTP total, fmt/clippy clean.

- 2026-09-22: M12.3 (providers) investigated per this document's own
  pre-authorized decision path and **DEFERRED with evidence**, mirroring
  M9.6's precedent exactly (investigate first, defer only with evidence,
  never silently). Findings:
  - `docs/RESEARCH.md`'s own `docs/providers.md` reference was never
    actually written (confirmed: no such file exists in `docs/`) --
    unlike M11's Eye/Pulse/blast-radius, which reused already-accepted
    M1-M10 primitives, providers have no existing design, no existing
    fixture, and no existing HTTP client scaffolding to compose from.
  - `Cargo.toml` has no standalone HTTP client dependency (`http`/
    `http-body-util` exist only as `kube`'s own transitive plumbing) --
    a real provider adapter would need a new dependency and a genuinely
    new network/trust boundary, the same class of "new architecture, not
    composition" M12.1's plugin execution already was, except at the
    project's own lowest priority tier.
  - Neither `sauron-test` nor `sauron-m9` (the only two isolated kind
    clusters this project is authorized to write fixtures to) has a
    Prometheus/VictoriaMetrics/VictoriaLogs instance deployed. Proving a
    provider adapter live (per this project's own "never accept on unit
    tests alone when the failure mode is live" discipline) would require
    deploying and maintaining a new class of fixture before any adapter
    code could even be tested against something real.
  - `docs/SOFKA_PARITY.md` marks **both** provider rows P3 DEFERRED (the
    project's own lowest priority tier, "Not yet delivered"), and
    `docs/ROADMAP.md`'s own M12 acceptance line ("Process limits/trust;
    four platform builds; checksums/license inventory; reproducible
    performance campaign") does not name providers at all -- confirmed
    already in M12.0's own reconnaissance, reconfirmed here rather than
    silently trusted.

  Conclusion: unlike M11.6 (context diff), where investigation found a
  small, cleanly composable win, M12.3 finds the opposite -- a genuinely
  large new integration surface with zero existing scaffolding, at the
  lowest priority tier, not required by the roadmap's own acceptance
  contract, and untestable live without first building new cluster
  fixtures this session has no evidence-backed need to build. **DEFERRED**,
  not implemented, not half-built. `PROVIDER EVIDENCE != KUBERNETES TRUTH`
  and the rest of this document's own provider trust-model section remain
  as a frozen contract for whichever future milestone takes this on.

- 2026-09-22: M12.4 (headless maturity) implemented as the hardening pass
  this document's own contract promised, not a rebuild -- `--check`/
  `--snapshot`/`info`/`info --offline` were already P0 **TESTED** in
  `docs/SOFKA_PARITY.md` before this slice started. One additive change:
  `--output json|yaml`'s document object gained `"schemaVersion": 1` as
  its first key -- every existing key/shape is otherwise byte-for-byte
  unchanged, so any consumer that already ignores unknown keys keeps
  working verbatim; `scripts/accept-m3.py`'s own live `snapshot()` helper
  (which only ever reads specific keys like `result['items']`) needed no
  change, confirmed by inspection rather than assumed.

  Every other headless guarantee was **verified live, not merely
  asserted**: `--snapshot --output json < /dev/null` against the real
  `kind-sauron-test` cluster exits `0`, needs no TTY (stdin genuinely
  closed), writes zero ANSI escape bytes to stdout (`grep -c $'\x1b'`
  found none), and correctly puts `PARTIAL DISCOVERY` on stderr while data
  stays on stdout; a request for a nonexistent resource kind exits `1`
  with a clear message; `info --offline` makes zero Kubernetes requests
  and needs no TTY either.

  **A real test-flake found and fixed during this slice's own `cargo
  test` run** (test-harness bug, not an app bug -- classified before
  touching anything, matching this project's own bug discipline):
  `app::session::tests::dropping_sessions_leaves_no_real_child_process_
  running` (written in M12.1) failed intermittently once M12.2 added its
  own `plugin.rs` tests that spawn an identically-shaped `sleep 30`
  fixture -- `cargo test` runs tests in parallel by default, so one
  test's own `pgrep -f "sleep 30"` could match the *other* test's
  concurrently-running process. Root-caused by re-running the failing
  test in isolation (passed every time) versus the full suite (flaked),
  confirming it was a fixture-uniqueness problem, not a real ordering bug
  in `Sessions`/`GroupKillGuard` themselves. Fixed by giving the session
  test's own fixture a unique sleep duration derived from the test
  process's own PID, exactly the same fix already applied to `plugin.rs`'s
  own abort-path regression test earlier in this milestone. Confirmed
  stable across 4 consecutive full-suite runs afterward.

  No new Rust-level unit test was added for the `schemaVersion` field
  itself: `main.rs` is the binary entrypoint, not library code, and this
  project has no existing pattern for unit-testing it directly (headless
  behavior is proven live, via `scripts/accept-m3.py`'s own
  `snapshot()` calls and this slice's own live checks above) -- adding a
  main.rs-testing harness for one additive field would be more new
  architecture than the change itself, not proportionate to a hardening
  pass. 445 unit + 76 fake-HTTP total, fmt/clippy clean.

- 2026-09-22: M12.5 (packaging) implemented per the user's own explicit
  decision on the packaging strategy question: define the CI build matrix,
  do not execute it, and prove the one platform this machine can honestly
  build (Linux x86_64) with a real local build and full acceptance. Before
  packaging could produce a complete archive, a real gap surfaced: the
  project had no `LICENSE` file and no `license` field in `Cargo.toml` --
  asked the user directly rather than presuming a legal choice; the user
  chose dual `MIT OR Apache-2.0`. Added `LICENSE-MIT` and `LICENSE-APACHE`
  at the repo root and `license = "MIT OR Apache-2.0"` in `Cargo.toml`.

  Added `scripts/package.sh` (stages the binary + both LICENSE files +
  `docs/INSTALL.md` into `target/package/sauron-<version>-<target>/`, then
  produces a deterministic `.tar.gz` via `tar --sort=name
  --mtime='1970-01-01 00:00:00Z' --owner=0 --group=0 --numeric-owner` in
  `target/dist/` -- no embedded timestamps or host-specific owner/group IDs,
  so repeated packaging of byte-identical inputs is byte-identical output).
  Added `docs/INSTALL.md` (platform table, extraction, checksum
  verification, PATH install, from-source build, `info --offline`/
  `--check` as the two safe first commands to run). Added
  `.github/workflows/release.yml`: `workflow_dispatch`-only trigger (never
  fires on push/PR), a 4-target build matrix (`x86_64-unknown-linux-gnu`,
  `aarch64-unknown-linux-gnu` via a cross linker, `x86_64-apple-darwin`,
  `aarch64-apple-darwin`), packaging + native-only basic-acceptance steps,
  and a `manifest` job that runs `scripts/checksums.py` (below) over every
  uploaded archive. The workflow's own header comment states plainly that
  it has never been executed -- this machine has no rustup cross-target
  toolchain and no macOS hardware, and triggering CI beyond what was
  already authorized for `main`/tags needs its own explicit go-ahead,
  neither of which this slice has.

  **Proven live, not merely asserted**, for Linux x86_64: `cargo build
  --release` produced `target/release/sauron` (17 MiB); `bash
  scripts/package.sh x86_64-unknown-linux-gnu target/release/sauron`
  produced `target/dist/sauron-0.1.0-x86_64-unknown-linux-gnu.tar.gz`
  containing exactly `sauron`, `LICENSE-MIT`, `LICENSE-APACHE`,
  `INSTALL.md`; extracted into a scratch directory, confirmed permissions
  (`sauron` 0755, the three docs 0644); ran the extracted binary's
  `--version` (`sauron 0.1.0`) and `info --offline` (prints config
  directory, read-only enforcement, theme, max-objects, cache budget,
  makes no Kubernetes request) -- the same two checks the CI workflow's
  own "Basic acceptance" step performs.

- 2026-09-22: M12.6 (checksums / license inventory) implemented.
  `scripts/checksums.py` (already written in M12.5's own slice) run
  against the real `target/dist` archive: produced `SHA256SUMS.txt` in
  the exact `sha256sum -c` format and `manifest.json`
  (`schemaVersion: 1`, one entry per archive: file/bytes/sha256, no
  embedded timestamp in the entries themselves so two runs over an
  unchanged archive produce byte-identical entries). **Verified live,
  both directions**: `sha256sum -c SHA256SUMS.txt` against the real
  archive reports it matches; a copy of the same archive with a single
  byte flipped (via `dd`, offset 100) against the *original* archive's
  own recorded checksum is correctly reported as **not** matching --
  proving the check actually detects tampering rather than trivially
  passing.

  Added `scripts/license_inventory.py`, generating a license inventory
  from `cargo license --json`'s own output against the locked dependency
  graph -- never hand-classified. Any dependency with an empty/missing
  license field is surfaced explicitly (a dedicated "Unknown/unparseable
  license" section), never silently dropped, mirroring this project's own
  established "UNKNOWN != ZERO" discipline. Writes
  `docs/LICENSE_INVENTORY.md` (human-readable table) and
  `target/dist/license-inventory.json` (machine-readable, same
  `schemaVersion: 1` shape as `checksums.py`'s own manifest). Run live
  against this project's real `Cargo.lock`: **305 dependencies, 0 with an
  unknown/missing license**, all permissive (`Apache-2.0 OR MIT` dominant
  at 201 occurrences; the remainder MIT/BSD/ISC/Zlib/Unlicense/
  BSL-1.0/CDLA-Permissive-2.0/Unicode-3.0 in various OR combinations; one
  `Apache-2.0 OR LGPL-2.1-or-later OR MIT` entry where the LGPL term is
  only one of three alternatives, not a forced obligation) -- no forced
  copyleft, no legal red flag, confirmed by inspection of the actual
  output rather than assumed from the crate ecosystem's usual reputation.

## Final acceptance checklist

- [ ] Every M12.1-M12.8 slice implemented and individually ACCEPTED in this
      document's own Journal (or, for providers only, explicitly
      evidence-backed DEFERRED, mirroring M9.6/M11.6's precedent — never
      silently dropped).
- [ ] No second task-supervision system, config file, redaction function,
      or benchmark harness exists anywhere in the M12 diff.
- [ ] Plugin execution: explicit argv only, no implicit shell; process-
      group termination proven live; timeout proven live; bounded stdout/
      stderr proven live; no orphan process survives quit/cancellation.
- [ ] Plugin trust: default is `disabled`; only explicitly `approved`
      plugins run; no credential (kubeconfig/token/Secret) reaches a
      plugin by default, proven by an explicit test inspecting the actual
      child environment.
- [ ] Plugin protocol: malformed output, oversized output, non-zero exit,
      and a missing executable all produce an explicit failure outcome,
      never a crash and never a silent retry.
- [ ] Providers (if shipped): partial/unavailable/stale stays explicit;
      native evidence remains usable when a provider fails; no credential
      sent to an unapproved endpoint; provider evidence is always
      provenance-labeled, never presented as native Kubernetes truth.
- [ ] Headless output: explicit `schemaVersion`; no ANSI; no prompt;
      deterministic ordering; correct non-zero exit codes; confirmed live
      with no TTY attached.
- [ ] Packaging: Linux x86_64 built and locally accepted; the CI matrix
      for the other three platforms is defined and documented, never
      falsely claimed as executed/proven this session.
- [ ] Checksums verify for every artifact actually produced; one modified
      byte fails verification; every produced artifact appears in the
      manifest.
- [ ] License inventory exists for the actual dependency graph; any
      unknown/unparseable license is explicit, never silently dropped.
- [ ] Performance: methodology documented (hardware/OS/Rust
      version/profile/repetitions); startup and large-object-count
      measurements recorded; no comparative claim against another tool;
      any optimization made records its own before/after numbers.
- [ ] Production sees zero writes across the entire milestone.
- [ ] All work stays bounded and cancellable.
- [ ] Full M1-M11 regression (`accept-m*.py`, unmodified) passes,
      including recreating either isolated kind cluster if drift is
      found.
- [ ] Combined M12 acceptance (`accept-m12.py`) run twice clean.
- [ ] M12 soak completes with recorded observations under the "observed
      stability only" honesty standard — no leak-freedom claims — and
      confirms no orphan child process remains afterward.
- [ ] Docs reconciled: this file, `HANDBOOK.md`, `docs/RUNBOOK.md`,
      `README.md`, `docs/ROADMAP.md` checkpoint line, `docs/SOFKA_PARITY.md`
      rows for every slice actually shipped, packaging/install docs.
- [ ] `docs/M9B_A_ACCEPTANCE.md`/`docs/M9B_B_ACCEPTANCE.md` confirmed still
      untouched.
- [ ] Worktree clean.
- [ ] Local annotated tag `m12-accepted` created — nothing pushed/
      published without explicit authorization beyond what was already
      given.
