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

Implemented, awaiting visual/TUI acceptance: native connection/discovery/watch, staged
UID-aware store, Pod and generic tables, command namespace/context switching, filter AST,
sorting, redacted YAML, contextual descriptions, UID-related Events/Explain, timeline and
Pod logs. The live test cluster and fixture resources exist. No full Sofka parity claim.

Completed checks (all green as of 2026-09-15, first commit):

- `cargo fmt --check` clean.
- `cargo check --all-targets` clean.
- `cargo clippy --all-targets -- -D warnings` clean (0 lints; the original 8 are fixed).
- `cargo test --all-targets`: 22/22 passing — 19 unit tests, 3 fake-HTTP integration tests
  in `tests/watch_transport.rs` (paged list+watch relist, bounded-channel cancellation,
  403 error surfacing without leaking credentials, partial discovery on a forbidden group).
  The live `tests/cluster.rs` test is `#[ignore]`d and has NOT been run against
  `kind-sauron-test` yet — that is a separate, still-pending step (`scripts/test-cluster.sh
  test`).
- Fixed a `prepare()` memoization bug where the clock was part of the cache key
  unconditionally, forcing a resort every tick regardless of data/filter/sort changes.
  Now only an `age`-based filter comparison ticks the cache, since that is the only thing
  whose membership can change from time alone; AGE display/sort stay correct without it.
  See HANDBOOK journal for the full analysis; covered by 3 new regression tests.
- `benches/pipeline.rs` runs and prints real (debug-profile, single-sample) numbers over
  100/1,000/5,000 synthetic objects; no release-profile or repeated-run data yet.
- Local `sauron-test` fixture node Ready; healthy Pod Running; failure fixtures present.

Explicitly NOT yet done, do not mark ACCEPTED: no interactive TUI session against the live
isolated cluster has been observed and recorded (watch/recovery, namespace/context
switching, terminal restore on resize/exit, Pod logs follow/previous with an explicit
container). Green unit/fake-HTTP tests are necessary but not sufficient evidence for that.

Next work: run `bash scripts/test-cluster.sh fixtures` then `bash scripts/test-cluster.sh
test` against the isolated cluster; drive the real TUI interactively and record what was
actually observed; only then update this checkpoint to ACCEPTED for the observed slices.
Update this checkpoint after every verification. No live application acceptance yet.

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
