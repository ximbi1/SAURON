# SAURON development runbook

Updated 2026-09-15. Canonical architecture/status: [HANDBOOK](../HANDBOOK.md).

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

## Current checkpoint

M1 core is implemented AND interactively accepted against the isolated cluster: native
connection/discovery/watch, staged UID-aware store, Pod and generic tables, command
namespace/generic-resource switching, filter AST, sorting, redacted YAML, contextual
descriptions, UID-related Events/Explain, and Pod logs. The live test cluster and fixture
resources exist. No full Sofka parity claim.

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

NOT yet observed: Pod `--previous` logs, explicit-container log selection, `:ctx`
switching to a second real context (the isolated kubeconfig only has one), all-namespaces
mode, and long-running/multi-hour watch stability.

Next work: exercise the remaining untested slices above; then move to M2/M3 acceptance.
Update this checkpoint after every verification. Never re-mark ACCEPTED from unit tests
alone — only from a directly observed session against the isolated cluster.

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
