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

Next: M2 item 4 (navigation history/breadcrumbs — `pods → CRD → deployments → back →
forward`), item 5 (command palette centralized on the action registry), item 6 (M2
interactive acceptance trying to break it).

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
