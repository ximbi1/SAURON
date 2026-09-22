# M12 — Plugins/providers/headless maturity/packaging/performance: acceptance ledger

Status: **ACCEPTED (M12.0-M12.8; M12.3 providers explicitly DEFERRED,
evidence-backed)**. Local annotated tag `m12-accepted`; never pushed. This
document began as M12.0: the scope-freeze contract written before any M12
implementation code, exactly like `docs/M11_ACCEPTANCE.md` was for M11. It
now also records the full Journal of every slice's own implementation and
live acceptance evidence, the amended slice ledger, the trust/protocol/
output/packaging/performance contracts, and the M12.8 combined-acceptance
record that satisfied the "exact conditions for `m12-accepted`" this
document itself originally set out.

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
| M12.7 | Performance — extend `benches/pipeline.rs`; startup + large-object-count campaign | ACCEPTED |
| M12.8 | Combined acceptance / full M1-M11 regression / soak / docs / final tag | ACCEPTED |

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

## Bugs / limitations

- **Found after `m12-accepted` was tagged and the repo made public**, via
  the real `ci.yml` GitHub Actions run (`ubuntu-latest`, a genuinely fresh
  runner with no prior `~/.config/sauron`) — a class of environment this
  session's own dev machine could never exercise, since every prior local
  test run had already caused `~/.config/sauron` to exist (created by
  earlier M10.8 workspace/bookmark saves). Classified as an **app bug**,
  not a CI/harness bug: a fresh machine with no prior SAUR-ON config is a
  completely ordinary real-world first-run scenario, not a CI quirk.
  `plugin::run()` used `config::directory()` (`~/.config/sauron`) as the
  spawned child's `current_dir` unconditionally; that directory is only
  ever created lazily by `Config::save()`, so on a machine where nothing
  has been saved yet, `Command::spawn()` failed with `ENOENT`, misreported
  as `"cannot start plugin: No such file or directory (os error 2)"` as if
  the executable itself were missing. Broke 10 tests in CI (all passed
  locally, for the reason above). Fixed by calling
  `std::fs::create_dir_all` on that directory before using it as
  `current_dir` — the exact same convention `Config::save()` itself
  already uses. Verified by reproducing the failure locally first
  (`XDG_CONFIG_HOME` pointed at a fresh empty temp directory, confirmed
  the same 10 tests fail with the fix reverted), then confirming the fix:
  full `cargo test --locked --all-targets` (521 tests) plus `cargo fmt
  --check` and `cargo clippy --locked --all-targets -- -D warnings`, all
  clean, all with `XDG_CONFIG_HOME` pointed at a fresh directory to match
  CI's own environment exactly. `docs/M12_ACCEPTANCE.md`'s own M12.8
  Journal entry (below) is left unmodified — this fix lands as its own
  dated entry and commit, after the tag, per this project's own "never
  amend an already-tagged milestone; land a fix as a new entry" precedent.

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
  `--output json|yaml`'s document object gained a new `"schemaVersion": 1`
  key -- every existing key/shape is otherwise byte-for-byte unchanged, so
  any consumer that already ignores unknown keys keeps working verbatim;
  `scripts/accept-m3.py`'s own live `snapshot()` helper (which only ever
  reads specific keys like `result['items']`) needed no change, confirmed
  by inspection rather than assumed. (Corrected in M12.8: this entry
  originally claimed `schemaVersion` was serialized as the object's first
  key. `scripts/accept-m12.py`'s own live check caught that this is false
  -- `serde_json::Value` has no `preserve_order` feature enabled in this
  project, so object keys serialize alphabetically for both `--output
  json` and `--output yaml`. Enabling `preserve_order` was considered and
  rejected: it would make `mutation::workflow::payload_hash`'s canonical-
  JSON fingerprint depend on source key order instead of being
  alphabetically stable, a real risk to a safety-critical mutation-
  confirmation path, for a purely cosmetic gain. Fixed the claim, not the
  ordering -- `src/main.rs`'s own comment now states the accurate
  guarantee: presence and value, never byte position.)

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

- 2026-09-22: M12.7 (performance) implemented as an extension of
  `benches/pipeline.rs`'s own already-established shape, not a rebuild.
  Closed the two gaps this document's own performance methodology section
  and `docs/SOFKA_PARITY.md` both named: cold-startup time and a
  larger-object-count tier. Cold startup is measured by spawning the real
  packaged binary (`env!("CARGO_BIN_EXE_sauron")`) running `info
  --offline` end-to-end via `Instant`, median of 5 reps — not wall-clock
  `time(1)`, matching the existing filter/render measurement's own
  methodology. The object-count sweep gained a `20_000` tier (this
  project's own `max_objects` default, closing the "large object count"
  gap while staying honest about being bounded by this one machine's
  memory, not a claim about arbitrary cluster scale). The bench now also
  prints its own environment header before any measurement: `rustc
  --version`, `uname -a`, and `/proc/cpuinfo` model name + logical core
  count via `std::thread::available_parallelism()` — so every run's
  numbers carry the hardware/toolchain context needed to judge them,
  rather than being bare numbers.

  **Run live** (release profile) on this development machine (12th Gen
  Intel Core i7-1250U, 12 logical cores, rustc 1.95.0, Linux
  7.0.12+kali-amd64):

  ```
  cold_startup_median_ms,1.580
  objects,build_ms,filter_sort_median_ms,render_median_ms,estimated_json_bytes
  100,2.739,0.044,0.391,37990
  1000,15.632,0.386,1.285,380890
  5000,78.335,2.952,5.262,1908890
  20000,270.227,11.321,24.147,7648890
  ```

  No comparative claim is made against any other tool. No optimization
  was made as a result of this campaign — every number above is a
  first-and-only measurement, not a before/after; per this document's own
  performance methodology, an optimization is only recorded here if a
  measured bottleneck actually motivates one, and none of these numbers
  (sub-second render even at 20k synthetic objects, ~1.6ms cold start)
  crossed a threshold that warranted one this slice. `cargo fmt`,
  `cargo clippy --all-targets`, and the full test suite (445 unit + 76
  fake-HTTP) all stayed clean after the bench change.

- 2026-09-22: M12.8 (combined acceptance) implemented, mirroring M10.9's
  and M11.8's own combined-acceptance precedent exactly.

  **Full M1-M11 regression** (every existing `accept-m*.py`, unmodified)
  passed. Two real issues found and fixed along the way, both correctly
  classified before touching anything:
  - `accept-m3.py` needs an explicit `filters`/`sorting` argument (not a
    bug -- a harness usage detail, confirmed by reading the script; both
    modes pass).
  - `accept-m9.py`'s own internal re-run of `accept-m4.py` hit a
    transient `previous_logs` fixture flake ("unable to retrieve
    container logs" from containerd, a log-retention race on the
    `crashloop` fixture pod, not a SAUR-ON defect) -- confirmed by
    re-running `accept-m4.py` standalone immediately after, which passed
    clean.
  - `accept-m9.py`/`accept-m11.py` both failed once on `blast_radius`'s
    own owner-chain grouping: the `healthy` Deployment fixture had
    accumulated 9 stale zero-replica ReplicaSets across this long
    session's own repeated fixture applies (`revisionHistoryLimit=10`
    never pruned them), inflating the candidate scan and pushing the
    safety disclaimer text below the visible pane in a narrower terminal
    -- genuine fixture drift, not an app bug (`blast_radius` correctly
    reported `PARTIAL EVIDENCE` when it hit a real bound). Fixed by
    deleting the stale ReplicaSets on the isolated `kind-sauron-test`
    cluster; both scripts passed clean on retry.

  **New `scripts/accept-m12.py`** (7 sequences, modeled on
  `accept-m11.py`'s own tmux/expect/kubectl pattern via `accept-m4.py`'s
  helpers): a real approved plugin (`/usr/bin/env`) runs end-to-end with
  real captured stdout and exit code 0; a live no-credential-leak proof
  (the launched shell exports a real `KUBECONFIG` path and a real
  fixture secret value; the plugin's own captured environment output is
  grepped and neither appears -- `ENV_ALLOWLIST` proven live, not just
  unit-tested); a live timeout (`timeout_secs=1` against a ~2.5s sleep)
  confirmed `TimedOut` and a real `pgrep` sweep confirming the process is
  actually gone; a live mid-run cancellation (Escape during a 30s sleep)
  confirmed via the same real `pgrep` sweep -- the exact scenario that
  surfaced the `GroupKillGuard` bug in M12.2, re-verified still fixed
  under this fresh harness; headless `--output json` schema/ANSI/exit
  checks; and the M12.5 packaged archive re-verified to extract and run.
  Run **twice, back to back, zero flakes both times**.

  A **real bug was found and fixed** while writing `accept-m12.py`'s own
  headless check: the original assertion (`schemaVersion` is the JSON
  object's first key) failed on a real invocation. Root-caused: this
  project's `serde_json` dependency has no `preserve_order` feature
  enabled, so `serde_json::Value`'s object type serializes keys
  alphabetically for both `--output json` and `--output yaml` -- meaning
  M12.4's own journal entry's claim ("gained `schemaVersion: 1` as its
  first key") was factually wrong, though the field's presence and value
  were always correct (a documentation/comment-accuracy defect, not a
  functional one; no consumer that reads JSON structurally was ever
  affected). Considered and **rejected** enabling `preserve_order`
  globally: `mutation::workflow::payload_hash`'s own "stable fingerprint"
  comment depends on `serde_json::to_string(value)` being deterministic
  regardless of the source object's own key order (today guaranteed by
  alphabetical canonicalization); switching to insertion-order
  serialization would make that fingerprint depend on the *source* JSON's
  own key order instead, a real risk to a safety-critical
  mutation-confirmation binding, for a purely cosmetic gain. Fixed the
  claim, not the ordering: `src/main.rs`'s own comment and this
  document's own M12.4 entry now state the accurate guarantee (presence
  and value, never byte position); `accept-m12.py`'s own check was
  updated to match. `cargo fmt`/`clippy --all-targets`/`test` all stayed
  clean after the fix; full regression re-confirmed unaffected (a
  comment-only production change).

  **New `scripts/soak-m12.py`** (modeled on `soak-m11.py`'s own shape):
  180 bounded seconds, 64 cycles of Eye/Pulse/blast-radius churn plus a
  plugin run every cycle (a fast-completing `echo`), a timeout cycle
  every 3rd iteration and a cancellation cycle every 4th, a real headless
  `--output json` invocation every 6th cycle, and an `:info` active-
  session-count check every 10th cycle (stayed at `0` throughout, as
  expected between cycles). RSS/fd/thread counts sampled every cycle
  stayed exactly flat (`49944 KiB` / `15` fds / `4` threads, start to
  finish) across 21 timeouts and 16 cancellations. Zero reconnects, zero
  transient errors. **A real harness bug was found and fixed** in the
  soak script's own shutdown sequence (not an app bug): since `sauron` is
  the tmux pane's own direct command (not wrapped in a shell), sending
  `C-c` to gracefully quit the app already ends the pane/session/server
  on its own, so a subsequent unconditional `tmux kill-server` call
  legitimately errors (server already gone) -- fixed by tolerating that
  exit code (matching `soak-m11.py`'s own precedent of a tolerant
  shutdown). After shutdown, an explicit `pgrep` sweep for both this
  run's unique plugin-fixture markers found **zero** orphan processes.

  **A second real test-flake was found and fixed** while running the
  final pre-commit `cargo test` pass (test-harness bug, not an app bug,
  classified before touching anything): `app::session::tests::
  dropping_sessions_leaves_no_real_child_process_running` and `plugin::
  tests::aborting_the_task_still_kills_the_whole_process_group_not_just_
  the_direct_child` both compute their own "unique" sleep-duration marker
  as `format!("30.{}", std::process::id() % 1000)` -- an identical
  formula. `std::process::id()` is the SAME value for every test running
  inside one `cargo test` binary, so the two tests always produce the
  exact same marker string and, whenever `cargo test`'s default
  parallelism runs them concurrently, one test's own `pgrep -f` sees the
  *other* test's still-running (or just-dropped) sleep process, causing a
  spurious pass or fail depending on timing. This is the same class of
  flake fixed twice earlier in M12 (M12.2, M12.4) — a per-process marker
  is not sufficient when two call sites share the exact same formula
  within one binary; only distinct static markers guarantee no collision
  regardless of PID. Fixed by giving `app::session`'s own test a
  different reserved base ("35." instead of "30."), leaving
  `plugin.rs`'s own tests on "30.". Confirmed stable across 4 consecutive
  full-suite runs afterward (445 unit + 76 fake-HTTP, all green every
  time).

  **Docs reconciled**: `HANDBOOK.md` (Feature status table, Current
  milestone/continuation instructions, a new Recorded M12 checks
  paragraph matching the M9/M10/M11 precedent), `docs/RUNBOOK.md`
  (Current checkpoint, architecture summary, the stale "M12 is next and
  has not started" line), `README.md` (Status table, a new full M12
  section matching M9/M10/M11's own style, explicit DEFERRED/packaging-
  subset callouts), `docs/ROADMAP.md` (checkpoint line, M12 row),
  `docs/SOFKA_PARITY.md` (Plugins rows split into what M12 actually
  delivered -- TESTED -- versus what remains DEFERRED; Distribution row
  moved to TESTED (subset); Performance and Headless rows updated with
  M12.7/M12.4 evidence; Providers rows left untouched, still DEFERRED,
  never silently upgraded). `docs/M9B_A_ACCEPTANCE.md`/
  `docs/M9B_B_ACCEPTANCE.md` confirmed untouched (`git diff --stat`
  empty).

  Production received zero writes across the entire milestone -- every
  live check ran against `kind-sauron-test` (and `kind-sauron-m9` via
  `accept-m9.py`'s own regression). Worktree is clean except for this
  milestone's own intended changes. This is the final milestone of the
  current roadmap.

- 2026-09-22: post-tag fix, landed after `m12-accepted` and after the repo
  was made public in preparation for a `crates.io` publish + GitHub
  Release. The real `ci.yml` workflow ran for the first time on a genuine
  `ubuntu-latest` runner (this session's own dev machine could never
  surface this: `~/.config/sauron` already existed locally from earlier
  M10.8 testing, so `plugin::run()`'s `current_dir` call never hit a
  missing directory here). Full root cause, fix, and verification are in
  this document's own "Bugs / limitations" section above. Also in this
  same pass: `Cargo.toml`'s `[package].name` was split from the actual
  binary/lib name ("sauron" is already registered on crates.io by an
  unrelated project) -- `[package].name = "saur-on"` is now only the
  crates.io publish identifier; `[lib]`/`[[bin]]` explicitly pin the real
  crate/binary name back to `"sauron"`, and `src/brand.rs`'s `BINARY`
  constant became a literal instead of `env!("CARGO_PKG_NAME")` so the
  config directory, `--version` output, and user agent are all
  unaffected. `repository`/`readme`/`keywords`/`categories` metadata was
  added for the eventual `cargo publish`. `publish = false` is
  deliberately still set; removing it is its own explicit future step.

## Final acceptance checklist

- [x] Every M12.1-M12.8 slice implemented and individually ACCEPTED in this
      document's own Journal (or, for providers only, explicitly
      evidence-backed DEFERRED, mirroring M9.6/M11.6's precedent — never
      silently dropped).
- [x] No second task-supervision system, config file, redaction function,
      or benchmark harness exists anywhere in the M12 diff.
- [x] Plugin execution: explicit argv only, no implicit shell; process-
      group termination proven live; timeout proven live; bounded stdout/
      stderr proven live; no orphan process survives quit/cancellation.
- [x] Plugin trust: default is `disabled`; only explicitly `approved`
      plugins run; no credential (kubeconfig/token/Secret) reaches a
      plugin by default, proven by an explicit test inspecting the actual
      child environment.
- [x] Plugin protocol: malformed output, oversized output, non-zero exit,
      and a missing executable all produce an explicit failure outcome,
      never a crash and never a silent retry.
- [x] Providers (if shipped): partial/unavailable/stale stays explicit;
      native evidence remains usable when a provider fails; no credential
      sent to an unapproved endpoint; provider evidence is always
      provenance-labeled, never presented as native Kubernetes truth.
- [x] Headless output: explicit `schemaVersion`; no ANSI; no prompt;
      deterministic ordering; correct non-zero exit codes; confirmed live
      with no TTY attached.
- [x] Packaging: Linux x86_64 built and locally accepted; the CI matrix
      for the other three platforms is defined and documented, never
      falsely claimed as executed/proven this session.
- [x] Checksums verify for every artifact actually produced; one modified
      byte fails verification; every produced artifact appears in the
      manifest.
- [x] License inventory exists for the actual dependency graph; any
      unknown/unparseable license is explicit, never silently dropped.
- [x] Performance: methodology documented (hardware/OS/Rust
      version/profile/repetitions); startup and large-object-count
      measurements recorded; no comparative claim against another tool;
      any optimization made records its own before/after numbers.
- [x] Production sees zero writes across the entire milestone.
- [x] All work stays bounded and cancellable.
- [x] Full M1-M11 regression (`accept-m*.py`, unmodified) passes,
      including recreating either isolated kind cluster if drift is
      found.
- [x] Combined M12 acceptance (`accept-m12.py`) run twice clean.
- [x] M12 soak completes with recorded observations under the "observed
      stability only" honesty standard — no leak-freedom claims — and
      confirms no orphan child process remains afterward.
- [x] Docs reconciled: this file, `HANDBOOK.md`, `docs/RUNBOOK.md`,
      `README.md`, `docs/ROADMAP.md` checkpoint line, `docs/SOFKA_PARITY.md`
      rows for every slice actually shipped, packaging/install docs.
- [x] `docs/M9B_A_ACCEPTANCE.md`/`docs/M9B_B_ACCEPTANCE.md` confirmed still
      untouched.
- [x] Worktree clean.
- [x] Local annotated tag `m12-accepted` created — nothing pushed/
      published without explicit authorization beyond what was already
      given.
