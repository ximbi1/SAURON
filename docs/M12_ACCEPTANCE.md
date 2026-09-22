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
| M12.1 | Plugin execution foundation: subprocess trust/process-group/bounds, reusing `app::session::Sessions` | PLANNED |
| M12.2 | Plugin protocol (smallest safe shape) + command registry wiring | PLANNED |
| M12.3 | Providers — investigated; accepted or evidence-backed DEFERRED | PLANNED |
| M12.4 | Headless maturity — hardening pass (schema version, exit-code/output contract confirmed) | PLANNED |
| M12.5 | Packaging — CI build matrix defined (4 platforms); Linux x86_64 built+proven locally | PLANNED |
| M12.6 | Checksums / license inventory / release manifest | PLANNED |
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
