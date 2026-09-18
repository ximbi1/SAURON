# SAURON

**One eye over the entire cluster.**

SAURON is a from-scratch, keyboard-first terminal console for Kubernetes,
written in Rust. It is built around one governing principle:

> **UNKNOWN ≠ ZERO ≠ HEALTHY.**
> Absence of evidence is never converted into a fabricated value, and a
> cluster state SAURON cannot justify is never called healthy by default.

It helps operators see what exists in a cluster, understand what needs
attention, inspect the evidence behind that judgment, and — where policy
allows — act on it. It is read-only by default and never mutates anything
without an explicit, policy-gated action.

## Status

M1 through M6 are **accepted** (local annotated tags `m1-accepted` through
`m6-accepted`, verified live against an isolated `kind` cluster, never
against production). M7 (guarded mutations) is not started.

| Milestone | Scope | Status |
| --- | --- | --- |
| M1 | Read-only foundation: kubeconfig, discovery, live resource table | ACCEPTED |
| M2 | Context/namespace switching, generic resource commands, history | ACCEPTED |
| M3 | Typed filters/sort, documents, Events, CRD printer columns | ACCEPTED |
| M4 | Interactive sessions: logs, exec, shell, attach, port-forward | ACCEPTED |
| M5 | Evidence-driven metrics, deterministic health, Explain 2.0, Timeline | ACCEPTED |
| M6 | Bounded relationship graph, `:adjacent`, `:xray` | ACCEPTED |
| M7 | Guarded mutations | not started |

See [`HANDBOOK.md`](HANDBOOK.md) for the full engineering record and
[`docs/`](docs/) for per-feature contracts and acceptance ledgers.

## What it does today

- **Live resource table** over the real Kubernetes watch API — any built-in
  or CRD resource, canonical GVK identity, no server Table negotiation
  guesswork.
- **Typed filters and sort** with three-valued (Strong Kleene) logic:
  `cpu>100m`, `age>1h`, `label.team=platform` — unknown values are excluded
  from a filter and always sort last, never coerced to zero or false.
- **Documents**: redacted YAML, describe, Events (UID-scoped, capped), and
  **Explain** — a deterministic evidence report, not a diagnosis.
- **Interactive sessions**: one-shot `exec`, an interactive `shell` and
  `attach` over a real terminal-handoff guard, and a native background
  **port-forward** manager — all denied outright under read-only policy.
- **Metrics** (optional): a bounded Kubernetes Metrics API collector.
  Missing/forbidden/stale samples render as explicit `UNKNOWN`, never `0`.
  CPU/memory usage, requests/limits accounting, and usage-as-percent are all
  typed, filterable, and sortable columns.
- **Deterministic health**: a pure function over known fields — no AI, no
  hidden scoring — with a strict precedence (deletion > terminal failure >
  container failure > scheduling/init > readiness > progressing > healthy >
  unknown) and every non-healthy result citing the exact fields that
  produced it.
- **Explain 2.0**: reuses that same health evidence directly (never a
  second, parallel diagnosis), adds bounded verified-ownership child-Pod
  correlation for workloads, and descriptive (never causal) metrics.
- **Timeline**: a bounded, UID-scoped, session-local history of meaningful
  state transitions — never Events, never an audit log, and a watch
  reconnect never invents a transition it did not actually observe.
- **Relationships** (`:adjacent`/`a`): a bounded graph built only from
  verified sources — `ownerReferences`, explicit typed references (Pod
  node/ConfigMap/Secret/ServiceAccount/PVC, PVC↔PV↔StorageClass), Service/Pod
  selector matches, and status-backed references (EndpointSlice targetRef,
  PV claimRef) — grouped as OWNED BY / OWNS / SELECTED BY / REFERENCES /
  REFERENCED BY. A selector match is never shown as ownership, and an IP
  address alone never creates an edge. `Enter` navigates to the selected
  related object by its exact canonical identity, never by name.
- **Xray** (`:xray`/`x`): the same verified relationships, traversed 1-3
  bounded hops from the selected object, reusing the identical deterministic
  health per node — never a second, parallel "graph health," and a related
  object is never presented as the cause of a problem.

## Safety model

- Read-only by default. `--readonly` is a hard override that survives
  config reload and cannot be lifted by any cluster/context config layer.
- Every mutating-adjacent capability (exec, shell, attach, port-forward)
  checks policy before any network connection is attempted.
- Secrets are redacted in every view, including embedded
  `kubectl.kubernetes.io/last-applied-configuration` annotations.
- All development and live acceptance testing runs exclusively against an
  isolated `kind` cluster (see `scripts/test-cluster.sh`), never against a
  real/production cluster.

## Building and running

```sh
cargo build --release
./target/release/sauron                       # pods in the current namespace
./target/release/sauron deployments -A         # all namespaces
./target/release/sauron --context my-ctx pods -n my-ns -l app=web
./target/release/sauron --readonly             # hard-disable exec/shell/attach/forward
```

Keyboard is the primary interface: `?` opens the effective keybinding
reference at any time, `:` opens the command palette. Requires network
access to a real (or `kind`) Kubernetes API server and a working
kubeconfig.

## Development

```sh
cargo fmt --check
cargo check --all-targets --locked
cargo clippy --all-targets --locked -- -D warnings
cargo test --all-targets --locked
```

Live acceptance scripts (`scripts/accept-*.py`, `scripts/soak-*.py`) drive
the real TUI over `tmux` against the isolated test cluster described in
[`docs/RUNBOOK.md`](docs/RUNBOOK.md) — they never touch a production
kubeconfig, verified by identity checks in `scripts/test-cluster.sh` before
any fixture write.

## Documentation map

- [`HANDBOOK.md`](HANDBOOK.md) — architecture, invariants, decisions, and
  the full milestone journal.
- [`docs/RUNBOOK.md`](docs/RUNBOOK.md) — current checkpoint and live test
  procedures.
- `docs/M3_ACCEPTANCE.md`, `docs/M4_ACCEPTANCE.md`, `docs/M5_ACCEPTANCE.md`,
  `docs/M6_ACCEPTANCE.md` — per-milestone acceptance ledgers with recorded
  live evidence (M1/M2 acceptance is recorded directly in `HANDBOOK.md`'s
  journal).
- `docs/ARCHITECTURE.md` — module layout and runtime invariants.
- `docs/METRICS.md`, `docs/HEALTH.md`, `docs/EXPLAIN.md`,
  `docs/TIMELINE.md`, `docs/PORT_FORWARD.md`, `docs/EXEC.md`,
  `docs/SESSIONS.md`, `docs/FILTERS.md`, `docs/SORTING.md`,
  `docs/DOCUMENTS.md`, `docs/LOGS.md`, `docs/RELATIONSHIPS.md` — per-feature
  contracts.
- `docs/SOFKA_PARITY.md` / `docs/RESEARCH.md` — competitive research
  baseline; research is explicitly not a claim of shipped parity.

## Non-goals

No hosted control plane, no telemetry uploads, no credential broker, no
automatic remediation, no required AI, no embedded external security
scanners.
