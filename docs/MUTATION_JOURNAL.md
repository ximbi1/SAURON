# Mutation journal

`src/mutation/journal.rs`. SAURON's own local evidence trail for every
mutation attempt — not a Kubernetes Event replacement, not an audit log
that persists across machines, and not exposed to any network.

## Format

Append-only JSONL at `$XDG_CONFIG_HOME/sauron/mutations.jsonl` (or
`~/.config/sauron/mutations.jsonl`), one `Record` per line, explicit
`schema_version` field (currently `1`) so a future format change can be
detected rather than silently misparsed.

Each `Record` carries:

- `timestamp`, `request_id`, `phase`
- exact target identity: `context`, `cluster`, `resource`, `namespace`,
  `name`, `uid`
- `effect` (`Debug` string of `MutationEffect`)
- `summary` — a redacted, field-path-only description (e.g.
  `metadata.annotations["m7-proof"]`), never a value
- `payload_sha256` — a fingerprint, never the payload itself
- `policy_decision` / `policy_reasons` — the exact structured policy result
- `outcome` / `detail` — the exact structured `MutationOutcome`, and a
  bounded classification string, never a raw server response body

Phases (matching `kube::mutation`'s pipeline): `IntentCreated`,
`PolicyEvaluated`, `Previewed`, `PreflightStarted`, `PreflightResult`,
`ConfirmationSatisfied`, `CommitStarted`, `CommitResult`. A dry run is
recorded under `PreflightResult`, a real mutation under `CommitResult` —
never the same phase, so an audit trail can always tell them apart even
though both may report the same `MutationOutcome::Committed`.

## What it never records

Never Secret `data`/`stringData`, never tokens, never kubeconfig
credentials, never `Authorization` headers, never exec stdin, never a full
sensitive HTTP body. The `Record` type has no field capable of holding any
of that by construction — `summary` and `detail` are the only free-text
fields, and both are documented and enforced as redacted-only content by
every caller. This is checked by a unit test
(`journal::tests::never_carries_secret_style_fields_by_construction`).

## Durability and bounds

- `append` fails loudly on any I/O or serialization error. Callers on the
  pre-commit path treat that failure as **fail-closed**: the mutation is
  never sent if the pre-commit journal write did not succeed.
- `recent(limit)` reads the whole file, skips any line that fails to
  parse (a malformed prior line never crashes SAURON), and returns at most
  `limit` (hard-capped at 500) most-recent records in file order.
- Individual fields are bounded (`summary`/`detail` truncated at 2048
  bytes with a `...<truncated>` marker; `policy_reasons` capped at 32
  entries) so one pathological record can never make the journal or its
  viewer unbounded.
- A missing journal file reads as empty, never an error — the `:mutations`
  view on a fresh installation just shows "No mutation journal records yet."

## Failure semantics, precisely

- **Pre-commit journal write fails** → the mutation is never sent. The
  gateway returns `Denied`; zero HTTP requests occur.
- **Post-commit journal write fails after a real server success** → the
  gateway returns `CommittedButJournalIncomplete`, never `Denied` and never
  silently treated as if the mutation didn't happen. The server-side effect
  already exists; the journal is incomplete, and the caller must know that
  distinction to reconcile later (e.g. via a fresh GET), not be lied to.

## Viewing

`:mutations` (key `m`, table mode) renders the bounded recent tail via
`mutation::view::journal_report` — a fully local, synchronous, zero-network
read. There is no live-tailing, no filtering, and no pagination UI yet; M7
only needs to prove the journal exists, is bounded, and is honest. Richer
journal browsing is M8+ scope if ever needed.
