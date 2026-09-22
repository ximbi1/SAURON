# M11 — Eye/Pulse/evidence bundle/blast radius/context diff: acceptance ledger

Status: **PLANNED — NOT STARTED**. This document is M11.0: the scope-freeze
contract written before any M11 implementation code, exactly like
`docs/M10_ACCEPTANCE.md` was for M10 and `docs/M8B_ACCEPTANCE.md`/
`docs/M9_ACCEPTANCE.md` before their own milestones. It records what
reconnaissance found in the existing codebase, which primitives every later
slice must reuse rather than fork, the amended slice ledger, and the exact
conditions for `m11-accepted`. Nothing in this file authorizes touching code,
cluster, or CI — only the slices marked ACCEPTED in the Journal, once this
contract exists, authorize implementation work.

`docs/M9B_A_ACCEPTANCE.md` and `docs/M9B_B_ACCEPTANCE.md` remain separate,
PLANNED — NOT STARTED ledgers for deferred Helm mutation work; M11 does not
touch, schedule, or depend on them.

## Baseline

M1-M10 are ACCEPTED. Baseline commit `59f1110`, local annotated tag
`m10-accepted`, never pushed. M9.6 (Helm rollback/uninstall) remains
explicitly DEFERRED. Worktree was clean at that tag.

## Purpose

M1-M10 built a Kubernetes/operator console: observation (watch/list) →
typed evidence (M5 health/metrics/Explain/Timeline, M6 relationships) →
guarded action (M7/M8/M8B mutation, M9 Flux/Argo/Helm) → journal → bulk/
workspace/bookmark/config UX (M10). M11 is explicitly a **composition**
milestone: it does not introduce a new resource-watching architecture, a new
health/relationship/mutation engine, or a new persisted truth store. It
builds five thin, evidence-preserving lenses over what M1-M10 already
compute:

- **Eye** — priority-ordered attention view of the *currently watched* table.
- **Pulse** — refreshed operational tile summary of the same current scope.
- **Evidence bundle** — a local, redacted, bounded export of one target's
  already-computed evidence (health/Explain/Events/Timeline/relationships/
  metrics), for taking outside the terminal (a ticket, an incident channel).
- **Blast radius** — a read-only safety lens over the same M6 relationship
  graph SAUR-ON already collects for Adjacent/Xray, adding current health per
  node and explicit provenance labels, invoked before or alongside a mutation
  preview. It never executes anything itself.
- **Context diff** (scoped down — see "Amendment to the proposed ledger"
  below) — an explicitly lower-priority, on-demand, bounded two-scope
  comparison.

## The core rule: AGGREGATION != NEW INTERPRETATION

Every M11 slice must be traceable to an already-accepted evidence primitive.
If a slice needs a second health engine, a second relationship engine, a
second mutation-safety engine, a second config model, or a second evidence
vocabulary, implementation stops and the fork is escalated rather than built
silently. Reconnaissance (below) found this is achievable for every proposed
slice — M11 is smaller in net-new architecture than M10 was.

## Reconnaissance: what already exists and is reused verbatim

This section is the actual architectural finding of M11.0, not aspiration.

- **`src/evidence.rs`** (pre-existing, M5-era, unmodified by M11): the
  literal "shared evidence vocabulary" the M11 kickoff asked M11.1 to build.
  `Unknown` (12-variant reason enum: `Unavailable`/`Forbidden`/`NotFound`/
  `NotReported`/`Stale`/`Unsupported`/`Partial`/`TargetReplaced`/
  `TransportError`/`Malformed`/`TimedOut`/`ZeroDenominator`), `Origin`,
  `Observation` (receipt-tick + source-time freshness, TTL-checked, immune to
  backwards wall-clock), `Evidence<T>` (`Result<T, Unknown>` + `Observation`),
  `Coverage` (`omitted`/`malformed`/`truncated`, `.partial()`). Every M11
  slice reuses these types directly rather than inventing parallel ones.
- **`resources::health::{Health, Severity}`** (M5.3, unmodified): every
  `resources::Object` already carries `pub health: Health` computed once at
  construction (`health::derive(&value)`), a pure function with no I/O.
  `Severity` is `Healthy < Unknown < Warning < Critical` (derived `Ord`).
  This is Eye's entire priority-ordering mechanism: sorting the *existing*
  `Severity` in reverse gives exactly the desired attention order (Critical
  first, Unknown ranked above Healthy since uncertainty deserves attention,
  Healthy last) with **zero new ordering logic**.
- **`graph::{Identity, Provenance, Edge, Bound, Graph}`** (M6, unmodified):
  `Provenance` is already `OwnerReference | ExplicitReference |
  SelectorMatch | StatusReference` — the *exact* category set the M11
  kickoff's Blast Radius section asked for ("verified ownership/dependency
  relationship, explicit reference, selector-derived relationship,
  status-reported relationship"). `Bound` (`Nodes | Edges | Depth |
  Evidence`) is already the "bound hit visible" vocabulary. Blast radius
  reuses this struct-for-struct; it adds no new relationship category.
- **`kube::relationships::report::{xray, adjacent}`** (M6, unmodified): the
  bounded, cancellable, epoch/scope-safe async collectors that already
  populate a `Graph`-backed `Report` for a given target, invoked from
  `Runtime::start_adjacent`. Blast radius calls the same `xray` collector
  (or a thin variant with different default limits) rather than writing a
  second traversal.
- **`adjacent::Target { line, resource, namespace, name, uid }`** (M6,
  unmodified): the existing "rendered line → navigable object identity"
  mechanism already used by `Document.adjacent` to let Adjacent/Xray reports
  jump to a related object on Enter/Follow. Eye reuses this struct verbatim
  for its own priority rows — no new navigation mechanism.
- **`kube::evidence::{document, events, owned_children}`** (M5, unmodified):
  the bounded, timed-out, UID-reverifying single-target evidence collector
  behind Explain/Events/Describe. The evidence bundle (M11.4) calls this
  exact function for its Explain/Events sections rather than re-deriving
  evidence.
- **`resources::store::Store::histories: BTreeMap<uid, VecDeque<Change>>`**
  (M5.5, unmodified): Timeline's session-local bounded change ring, already
  keyed by UID. The evidence bundle's "bounded Timeline" section reads this
  directly (`Runtime::timeline_text`'s own pattern), no new history buffer.
- **`safety::redact`/`safety::text`/`safety::api_error`** (M1-era,
  unmodified): the one global redaction function (Secret bodies, token/
  password/credential/apikey/private-key-shaped keys, `last-applied-
  configuration`, `managedFields`) and the one terminal-control-stripping
  text sanitizer. The evidence bundle reuses `redact` on every JSON value it
  embeds; it does not write a second masking heuristic.
- **`mutation::view::{policy_report, workflow_report, journal_report,
  bulk_report}`** (M7-M10, unmodified) and **`Runtime::open_static`/
  `open_mutations_view`** (M7, unmodified): the exact "pure Rust function
  turns already-in-memory Runtime state into a text report, `open_static`
  renders it as a `Document`, zero new network" pattern already used for
  `:mutations`/`:policy`. Eye and Pulse are new instances of this exact
  pattern, not a new UI subsystem.
- **`Config::save`** (M10.8, unmodified): the only existing atomic local
  write (temp file, `0600`, `rename()`). The evidence bundle's export reuses
  this *pattern* (not the literal function — it writes a directory of files,
  not one TOML file) for the same crash-safety and permission guarantees.
- **`command::registry()`/`Action`/`Command`/`command_names()`** (M1-M10,
  unmodified): the single central keymap/command registry every `:xray`/
  `:policy`/`:mutations`/`:workspace_save` entry already goes through. New
  M11 commands are new tuples in the same table, nothing else.

### Genuine architectural unknowns identified (not yet resolved by code)

These are implementation-level design questions for their own slice, not
forks requiring escalation — recorded here so the per-slice work has a
starting point instead of re-deriving it from scratch:

1. Whether Eye/Pulse render as `Mode::Document` (reusing `Document.adjacent`
   verbatim, simplest, matches Adjacent/Xray/Policy/Mutations exactly) or
   need a richer live-updating structured mode. **Decision (this
   reconnaissance): `Mode::Document`.** Both are inherently "recompute from
   already-live `state.rows`/`state.store`" operations with no independent
   network lifecycle of their own, so they need no `Source`/session
   machinery beyond what `open_static` already provides — a fresh render on
   demand (open, and an explicit `r`efresh reusing the existing document
   refresh key) is sufficient and matches `:mutations`'s own precedent
   exactly, rather than inventing a live-ticking widget class.
2. Whether Pulse needs any *new* bounded network call at all, or is 100%
   derived from state already loaded by the current watch. **Decision:**
   ship Pulse v1 as 100% derived (zero new requests, matches "idle rendering
   sends zero requests" and "no request storm" test requirements trivially).
   If reconnaissance during M11.3 finds a genuinely valuable bounded
   additional source (e.g., a capped Warning-Events-in-scope count), add it
   as an explicit, budgeted, cancellable addendum reusing
   `kube::evidence::events`'s exact pattern — never a second watch.
3. Exact bundle file format/manifest shape (M11.4) — deferred to that
   slice's own design, but constrained: local directory, one JSON manifest
   plus one text/YAML file per evidence section (mirrors `kube::evidence`'s
   own per-action text outputs), no single monolithic blob that would hide
   partial-export labeling.
4. Blast radius's exact default traversal depth/bound — deferred to that
   slice, but must reuse `graph::Limits`'s existing default shape
   (`nodes: 128, edges: 256, depth: 2`) as the starting point, not invent new
   numbers without reason.

## Amendment to the proposed ledger (evidence-backed, not a fork)

The M11 kickoff proposed ledger order M11.1→M11.6 as: foundation, Eye,
Pulse, bundle, context diff, blast radius. Reconnaissance of
`docs/SOFKA_PARITY.md`'s own parity ledger (this repository's existing,
already-accepted priority classification, reconciled as recently as
2026-09-21) shows:

| Row | Priority | Status |
| --- | --- | --- |
| `Overview \| Pulse refreshed health tiles` | P1 | DESIGNED |
| `SAURON \| Eye problem-priority overview` | P1 | DESIGNED |
| `SAURON \| Blast radius, safety lens, change preview` | P1 | DESIGNED |
| `Diagnostics \| Redacted incident bundle...` | P1 | DESIGNED |
| `SAURON \| Context diff, navigation replay, explainable score` | **P3** | **DEFERRED** |
| `Fleet \| ... cross-context ... multi-cluster Eye` | **P3** | **DEFERRED** |

`docs/ROADMAP.md`'s own M11 acceptance line ("Partial overview is explicit;
sensitive fixture/data redaction is proven; local export permission/
overwrite tests") also does not name context diff or blast radius as
specific acceptance gates — only the overview (Eye/Pulse) partial-evidence
guarantee and the bundle's redaction/export guarantees are load-bearing
there. Blast radius and context diff are named in-scope but not singled out
in that one-line contract the way bundle/overview are.

This is exactly the kind of "reconnaissance proves a different division is
cleaner" case the M11 kickoff pre-authorized amending, not a stop-and-ask
fork (it is not opaque scoring, not a controversial identity rule, not a new
sensitive-data disclosure, not a predicted-impact claim, not a new database,
not an unsafe write, not an invariant weakening, and it does not contradict
the roadmap's own one-line acceptance text). **Amended ledger below reorders
blast radius ahead of context diff** to match the repository's own P1 vs P3
classification, and treats context diff as **candidate for the same
evidence-backed DEFERRED treatment M9.6 received** if a genuine architectural
reason emerges during that slice (its own SOFKA_PARITY row already notes
"scoring optional" — i.e., it was never fully specified even at RESEARCH
time). It is attempted, not pre-emptively dropped — M9.6's own precedent was
to investigate first and defer only with evidence, never to skip
investigation.

## Proposed slice ledger (amended)

| Slice | Scope | Status |
| --- | --- | --- |
| M11.0 | Acceptance contract / architecture freeze (this document) | ACCEPTED |
| M11.1 | Shared evidence snapshot foundation (formalize what already exists; the one new piece: a pure priority-ordering helper) | ACCEPTED |
| M11.2 | Eye — problem-priority current-context overview | ACCEPTED |
| M11.3 | Pulse — bounded refreshed operational overview | ACCEPTED |
| M11.4 | Evidence bundle — redacted local incident export | PLANNED |
| M11.5 | Blast radius — evidence-backed safety lens (reordered ahead of context diff; P1 in SOFKA_PARITY) | PLANNED |
| M11.6 | Context diff — read-only comparison (P3 in SOFKA_PARITY; attempt with evidence-backed scope, DEFER only if investigation shows a genuine architectural blocker, mirroring M9.6) | PLANNED |
| M11.7 | Cross-feature navigation / UX integration (command registry wiring for all of the above) | PLANNED |
| M11.8 | Combined acceptance / full M1-M10 regression / soak / docs / tag | PLANNED |

## Explicit non-goals for M11

- No new continuous multi-kind watch architecture. Eye/Pulse operate over
  the currently watched single resource kind's already-loaded
  `state.rows`/`state.store`; they do not fan out to watch every kind in a
  namespace/cluster.
- No opaque/ML/numeric "risk score." Eye's ordering is the existing,
  explainable `Severity` `Ord`, nothing invented.
- No raw Secret body, kubeconfig, credential-plugin output, TLS private key,
  or unmasked Helm value ever leaves the evidence bundle. `safety::redact`
  is applied to every embedded JSON value; masked Helm values stay masked.
- No raw logs in the bundle unless a future slice defines a separate,
  explicit, narrow safe-log contract first (mirrors M9.5's own Helm-Secret
  exception precedent) — v1 bundle omits logs entirely.
- No causal language, predicted outage, predicted reschedule target, or
  "affected user count" claim anywhere in blast radius output. Every row is
  labeled by its `graph::Provenance` (or a clearly-marked "policy/
  operational risk" note), never collapsed into one undifferentiated
  "affected" bucket.
- No mutation is ever executed from Eye, Pulse, the bundle exporter, blast
  radius, or context diff. All five are strictly read-only/local-export;
  any actual mutation continues to go through the unmodified M7/M8/M8B/M10
  gateway from the normal table/preview flow.
- No second persisted evidence/config store. If M11 needs a config field
  (e.g., a default bundle export directory), it extends the existing
  `Config`/`Settings` model with the existing atomic-write/fail-safe-reload
  semantics — no new file format.
- No cross-cluster identity claim. Within one connection, `Object.uid`
  remains sole identity authority (already true, unchanged). If context diff
  ships, cross-context comparison uses an explicitly labeled *comparison
  key* (`resource + namespace + name`), never claimed as identity.
- Fleet-wide/multi-cluster Eye (SOFKA_PARITY's own P3 `Fleet` row) is
  explicitly out of scope for M11 — a single-context Eye, matching the
  roadmap's own "current-context overview" framing.

## Invariants (inherited from M1-M10, unmodified, must survive M11)

- `UNKNOWN != ZERO != HEALTHY`
- `RELATIONSHIP != CAUSE`
- `VERIFIED REFERENCE != SELECTOR-DERIVED RELATIONSHIP`
- `COMMIT != OBSERVED EFFECT`
- `REQUEST ACCEPTED != DESIRED EFFECT OBSERVED`
- `UID != NAME`
- `LOOKS CORRECT != PROVEN CORRECT`
- No name-prefix heuristics. No silent scope widening. No unbounded cluster
  fanout. No hidden mutation retry. No API requests from rendering. No
  runtime shell-out to kubectl/helm/flux/argocd. No Secret body disclosure
  outside the one already-accepted M9.5 Helm-Secret reader. Production
  remains read-only.

## Safety boundary

Identical to every prior milestone. Production cluster: reads only, forever.
All new live/write-adjacent testing (there should be very little — M11 is
almost entirely read/export) happens against the two existing guarded
isolated kind clusters:

- `sauron-test` / `kind-sauron-test` / `.test-cluster/config` /
  `scripts/test-cluster.sh` (general regression + all M11 live evidence).
- `sauron-m9` / `kind-sauron-m9` / `.test-cluster-m9/config` /
  `scripts/test-cluster-m9.sh` (only if a slice needs to prove GitOps/Helm
  evidence composition, e.g. the bundle including Flux/Argo/Helm metadata
  for a target that has it).

No default kubeconfig for fixture writes, ever. Never print/commit
kubeconfig or credential material. Never push/publish. Local annotated tag
`m11-accepted` only, mirroring every prior milestone's own discipline.

## Per-slice contracts

### M11.1 — Shared evidence snapshot foundation

Reconnaissance already found the vocabulary exists (`evidence::*`,
`resources::health::*`, `graph::*`). The actual new work in this slice:

- A small pure module (`src/eye.rs`'s own foundation, or a shared
  `priority` helper if Eye and Pulse both need it) that takes a scope
  (already-live `&[SharedObject]` / `&Store`) and produces a
  deterministically-ordered, provenance-preserving view — no network, no
  mutable global state, cancellation via the existing `epoch`/`request`
  pair `State` already carries (a late/stale computation is simply never
  rendered, matching `prepare()`'s own cache-key discipline).
- Confirm via unit tests that composing existing `Unknown`/`Coverage`/
  `Severity`/`Provenance` is sufficient for every M11.2-M11.6 evidence class
  identified above — if a slice later needs a distinction none of these
  express, that is itself evidence for a very small, explicitly-justified
  addition (e.g., a new `Unknown` variant), never a parallel enum.

Tests: scope/epoch change discards a stale computed view; same-name/new-UID
never merges two identities into one priority row; one forbidden source
among several does not collapse the whole view to empty/healthy; ordering is
deterministic for a fixed input; zero I/O from the pure functions themselves
(enforced by them taking no connection/client parameter at all).

### M11.2 — Eye

Purpose: within the currently watched resource kind/namespace/context, order
`state.rows` by attention-worthiness and let the operator jump straight to
any of them.

Reuses, verbatim: `Object.health.severity`/`.evidence` for ordering and the
"why"; `adjacent::Target`-shaped navigation; `Document`/`open_static` for
rendering; `state.filter_unknown`/`state.synced`/`state.store.incomplete`
for the explicit partial/stale signal (Eye must say so, never render a
silently-empty "all clear").

Ordering: `Reverse(Severity)` (Critical, Warning, Unknown, then Healthy),
ties broken by namespace/name for determinism — the exact same stable-sort
discipline `resources::sort` already establishes elsewhere. No numeric
score. Each row's "reason" is `Health.evidence.join("; ")` when non-empty,
or an explicit `"Unknown: <cause>"`/`"No evidence -- Healthy"` line when it
is.

Tests: broken Pod/workload sorts above healthy ones; `Unknown` is visually
and positionally distinct from `Healthy`, never silently merged; a
namespace/RBAC-partial store (`state.store.incomplete` or a forbidden LIST)
is shown as an explicit partial banner, not a falsely-complete "all healthy";
same-name/new-UID replacement does not carry over a stale priority row;
bound/truncation from the underlying store is visible; selecting a row
navigates to the exact UID via the existing table selection mechanism (never
by name); 32x9 renders without corruption; effective (resolved) keymap
help lists `:eye`.

### M11.3 — Pulse

Purpose: a refreshed tile summary of the same current scope — counts, not
rows.

v1 is 100% derived from already-loaded state (`state.rows`, `state.store`,
`state.metrics`) — zero new network requests, so "bounded asynchronous
refreshed" is trivially satisfied by the existing watch/render loop; opening
`:pulse` (or pressing refresh inside it) simply recomputes the same pure
tiles function against current state. Every tile distinguishes `0` from
`Unknown`/`unavailable`/`partial`/`truncated` — reusing `Severity`/`Unknown`/
`Coverage` directly, never collapsing a missing source into a zero count.

Tests: zero known-problems is visually distinct from "problems unknown
because the list is RBAC-partial"; metrics-unavailable tile is distinct from
metrics-all-healthy; rapid context/namespace/resource switching does not
show a stale tile set (same epoch/request discipline as the rest of the
app); refresh is cancellable and cheap; narrow terminal renders without
corruption; idle rendering (no user input) sends zero Kubernetes requests
(provable the same way M5's "no request storm" tests already prove it for
metrics).

### M11.4 — Evidence bundle

Purpose: a local, bounded, redacted export of one selected target's
already-computed evidence, for taking outside the terminal.

Contract (frozen before implementation, per the M11 kickoff's own
requirement):

**Included** — canonical target identity (kind/namespace/name/UID);
collection timestamp; `Health` (status/severity/evidence); an
Explain-equivalent text section (reusing `kube::evidence::document` with
`Action::Explain`); related Events (reusing `kube::evidence::events`,
bounded/capped exactly as Explain already bounds them); a bounded Timeline
section (`Store::histories` for that UID, same bound Timeline's own view
already uses); a bounded relationship summary (`kube::relationships::report`
result, provenance-labeled, same bound as Adjacent/Xray); metrics with
explicit freshness (`state.metrics`, `Evidence<f64>`-shaped, `Unknown` when
stale/absent — never a bare number with no freshness marker); GitOps/Helm
metadata already exposed by the accepted M9 readers, if the target has any
(Flux/Argo status, masked Helm release view) — never re-fetched with looser
rules than the M9 reader already enforces; SAUR-ON build/version metadata
(no credentials). Every section is independently labeled `OK`/`PARTIAL`/
`UNAVAILABLE`.

**Excluded (v1, no exception without a new explicit contract)** — Secret
body/data, kubeconfig, any credential-plugin output, TLS private keys,
Pod/container environment variables (they may contain secrets and this
project's redaction is heuristic, not a guarantee for arbitrary env var
names), unmasked Helm values/notes, raw Pod logs, raw `mutation::journal`
records beyond a redacted, target-scoped, already-safe summary (the journal
itself is append-only-local and already excludes secret bodies, but a bundle
must not become a second unredacted export path for it).

Export mechanics: local directory only, one JSON manifest (deterministic
file inventory + per-section OK/PARTIAL/UNAVAILABLE + any bound hits) plus
one file per evidence section; every write goes through a new
`bundle::export` helper reusing `Config::save`'s exact
atomic-temp-file/`0600`/`rename()` pattern per file, `0700` on the bundle
directory; no silent overwrite — an existing destination is refused unless
the caller explicitly confirms overwrite (mirrors `bulk_delete`'s own
"explicit confirmation, never silent" precedent, adapted to a filesystem
decision instead of a cluster one); no path traversal (destination and every
generated filename validated); bounded total size (the same per-section caps
above already bound it; the manifest records total bytes written); a failed
write mid-export cleans up its own partial output rather than leaving a
half-written bundle that looks complete.

Tests (must include live evidence, not unit-only, since fixture-value
absence is the acceptance-critical claim): a Secret-referencing target's
bundle contains the reference (name) but never the Secret's `data`/
`stringData`; a live sensitive fixture value (a real Secret's plaintext,
matching M9.5's own `hunter2`-style fixture-value-absence proof) is searched
for in the exported bytes and confirmed absent; a masked Helm release stays
masked in the bundle; a denied/forbidden Events or metrics source produces
an explicit `PARTIAL`/`UNAVAILABLE` section, never a silently-empty one;
same-UID-replaced target is rejected (mirrors every other UID-reverification
path); destination-exists-without-overwrite is refused; destination-exists-
with-explicit-overwrite succeeds; file permissions are `0600`/directory
`0700`; an interrupted write (simulated) leaves no half-written file at the
final path; a crafted target name cannot escape the destination directory;
manifest inventory is deterministic for a fixed input.

### M11.5 — Blast radius

Purpose: before (or alongside) a mutation preview, show what has a verified
or labeled relationship to the target, grouped by exactly how that
relationship is known, each with its own current `Health` — a safety lens,
never a causal or predictive claim.

Reuses, verbatim: `kube::relationships::report::xray` (or a thin variant
with its own `graph::Limits`, starting from `graph::Limits::default()`) for
the bounded collection; `graph::{Provenance, Bound}` for the category/bound
labels; `resources::health::Health` for each node's *current* state (never a
predicted future state); `adjacent::Target`-shaped navigation into any
listed node.

Output groups, each explicitly labeled and never merged into one "affected"
bucket: **directly targeted** (the object itself); **verified
ownership/dependency** (`Provenance::OwnerReference`); **explicit reference**
(`Provenance::ExplicitReference`); **selector-derived** (labeled as
inference — `Provenance::SelectorMatch` — never presented as verified);
**status-reported** (`Provenance::StatusReference`); an explicit **"policy/
operational risk" note** for cases evidence supports without a graph edge
(e.g., cordoning a Node relates to the Pods currently scheduled there per
`spec.nodeName`, which is a status fact, not an owner/selector edge — this
gets its own explicitly-labeled category, not folded into ownership).
Language is constrained: "structurally related", "currently scheduled on",
"selector-matches" — never "will fail", "will be down", "N users affected",
or any other outcome claim the evidence does not support.

Invoked read-only from the table (a new `Action::BlastRadius`, available on
any selected row, independent of whether a mutation is actually being
previewed — giving the operator this safety lens *before* choosing an
action) — it does not hook into the mutation state machine and cannot
authorize or trigger anything; an actual mutation continues through the
unchanged M7/M8/M8B/M10 gateway from its own normal entry points.

Tests: an owner chain (Deployment→ReplicaSet→Pod) is grouped as verified
ownership, not merged with selector matches; an explicit Secret/ConfigMap
reference from a Pod spec is its own category; a Service→Pod selector match
is explicitly labeled selector-derived/inference, never presented as
verified; a Node→Pod relationship (cordon/drain candidate) appears under the
explicit status-reported/operational-risk category, not ownership; cycle
safety (reuses `graph::Graph`'s own existing cycle handling — no new
traversal logic to re-test at this layer beyond confirming composition);
bound hit is visible; partial RBAC on one hop produces an explicit partial
note for that branch, not a silently-shrunk graph; same-name/new-UID
replacement is rejected the same way every other UID-checked view already
rejects it; no output string anywhere claims certainty of future impact
(grep-able in the acceptance script, matching the M9-era
"absence-of-a-forbidden-substring" test-writing lesson already learned this
session — a *positive* assertion of the correct labeled text, not just
absence of a banned word); zero writes; not reachable from any confirmation
step of an actual mutation (proving it cannot become a bypass path).

### M11.6 — Context diff

Purpose: read-only, bounded, on-demand comparison between two explicitly
selected contexts (same object kind/namespace/name looked up independently
in each), never a background cross-cluster watcher.

Given its SOFKA_PARITY P3/DEFERRED classification and its own
"scoring optional" note (i.e., it was never even fully specified at
RESEARCH time), this slice starts with an investigation, not an
implementation assumption: confirm whether a minimal, evidence-backed
version is cleanly buildable from already-accepted primitives (two
independent `kube::evidence`-style GETs, one per context, diffed on a small
explicit projection: `Health`, image refs, replicas, resource requests/
limits, GitOps revision/status where the M9 readers already expose it) with
no new identity concept. If that minimal version is genuinely
straightforward, implement and accept it at that reduced scope (documented
here, not silently assumed). If investigation surfaces a real architectural
blocker — most plausibly, a temptation to treat cross-cluster UID or name
matching as identity, which the M11 kickoff explicitly forbids — this slice
is DEFERRED with the same evidence-backed write-up discipline M9.6 used,
never silently dropped and never a stop-and-ask escalation for something
already pre-classified P3 by this repository's own ledger.

Identity semantics (binding if implemented): within one connection/context,
`Object.uid` remains sole identity authority, unchanged. Across two
contexts, comparison uses an explicit **comparison key**
(`resource + namespace + name`), labeled as exactly that — never presented
as, or confused with, identity. Diff categories: only-left, only-right,
comparable-but-different, equivalent-under-the-declared-projection,
unknown/partial. No mutation. Sensitive fields redacted the same way the
bundle redacts them.

Tests (if implemented): same logical name across two contexts is compared,
never assumed to be "the same object"; same-name/new-UID within one side is
rejected the same way every other UID-checked view rejects it; an
object present on only one side is labeled only-left/only-right, not
silently omitted; RBAC-incomplete on one side produces an explicit partial
comparison, not a false equivalent/absent; a CRD missing in one context
is explicit; ordering deterministic; bounded; a context switch mid-collection
discards the stale in-flight comparison; no credential leakage; zero writes.

### M11.7 — Cross-feature UX

New `Action`/`Command` registry entries only — no new navigation mechanism.
Candidate names (subject to the existing collision-checked
`Keymap::compile` conflict detection, which will refuse to build if two
actions collide in the same mode — the actual authority on whether a name/
key is available, not a manual audit here): `:eye`, `:pulse`, `:bundle`,
`:blast_radius`, `:context_diff` (if M11.6 ships). Help overlay must show
effective (resolved/overridden) bindings, matching M10.6's own established
contract — no hardcoded-default help text. Every M11 view remains reachable
by command-palette name alone, with no keybinding required (matching M10.7's
own "usable without any custom keybindings" precedent).

### M11.8 — Combined acceptance / full regression / soak / docs / tag

Purpose: close out M11 exactly like M10.9 closed out M10.

Scope: full M1-M10 regression via every existing `accept-m*.py` script,
unmodified (including `accept-m10.py` and `soak-m10.py`'s own precedent of
checking cluster health/recreating the isolated kind cluster(s) first if
drift is found — do not silently skip a live cluster check because the
prior milestone already proved it once). Dedicated `scripts/accept-m11.py`
covering every M11.2-M11.7 acceptance item above, run twice clean. Sensitive
bundle redaction proven from actual exported bytes against a live sensitive
fixture (not just a unit-level mock). Export permission/overwrite semantics
proven live. Eye/Pulse partial-evidence proven live (a real RBAC-limited or
forbidden scope, not just a fake-HTTP substitute, for at least the
acceptance-critical claim). Blast radius provenance/bound/causal-language
proven live. Context diff proven live if implemented, or its DEFER write-up
finalized with evidence if not. Bounded soak (M11's own new dimension per
the kickoff: Eye/Pulse refresh churn, bounded blast-radius/relationship
analysis, periodic bundle export not every cycle since file I/O would
distort the run, an M4 forward or other accepted long-lived operation held
alive if practical, selection/navigation churn from M10) with RSS/fd/thread/
reconnect/transient-error/bound-hit/export-count observations, "observed
stability only" honesty, never a leak-freedom claim. Docs reconciled:
`HANDBOOK.md`, `docs/RUNBOOK.md`, `README.md`, this file, `docs/ROADMAP.md`
checkpoint line, `docs/SOFKA_PARITY.md` rows for every slice actually
shipped. M9B-A/M9B-B confirmed still PLANNED — NOT STARTED and untouched.
Worktree clean. Local annotated tag `m11-accepted`, never pushed without
explicit authorization.

## Bug discipline (unchanged from M10, restated because M10 proved why it
matters)

For every live failure: reproduce → classify app/harness/environment →
root-cause → fix the correct layer → add a regression test when reasonable
→ `fmt`/`check`/`clippy`/`test` → rebuild → replay the exact failing live
flow → document in this file's Journal → continue. Never "fix" the app to
paper over a stale test binary (M10.9's own live example). Never "fix" the
harness to hide a real app bug (M10.9's `startup_warning` finding). Never
dismiss environment drift without evidence (M10.9's kind-cluster x509
finding) — but also never accept an unexplained live failure as
"environment" without the same `docker exec .../systemctl status kubelet`-
grade evidence M10.9 actually gathered.

## Bugs / limitations (placeholder)

None yet — implementation has not started. Updated per slice, exactly like
every prior milestone's own section.

## Journal

- 2026-09-22: M11.0 (acceptance contract / architecture freeze) written.
  Reconnaissance covered `HANDBOOK.md`, `docs/RUNBOOK.md`, `README.md`,
  `docs/ROADMAP.md`, `docs/SOFKA_PARITY.md`, `docs/M10_ACCEPTANCE.md`,
  `docs/EXPLAIN.md`, `docs/HEALTH.md`, `docs/TIMELINE.md`,
  `docs/MUTATION_POLICY.md`, and source: `src/evidence.rs`,
  `src/resources/health.rs`, `src/resources/mod.rs` (`Object.health`),
  `src/graph.rs`/`src/graph/references.rs`, `src/kube/evidence.rs`,
  `src/kube/relationships.rs` (via `Runtime::start_adjacent`),
  `src/adjacent.rs`/`src/xray.rs`, `src/safety.rs`, `src/config.rs`
  (`Config::save`'s atomic-write pattern), `src/app/document.rs`
  (`Document.adjacent`), `src/app/state.rs` (`Mode`/`Picker`),
  `src/app/mod.rs` (`open_static`/`open_mutations_view`/`open_document`/
  `start_adjacent`), `src/command/mod.rs` (`registry()`/`Action`/`Command`/
  `command_names()`). Key finding, restated above with file/line evidence
  throughout: every evidence primitive M11 needs already exists and is
  ACCEPTED (`evidence::*` is *literally* the "shared evidence vocabulary"
  the kickoff asked M11.1 to build; `graph::Provenance` is *literally* the
  category set Blast Radius asked for), so M11's actual net-new surface is
  substantially smaller than a first read of the kickoff's own slice
  descriptions would suggest — this is the intended outcome of the
  "aggregation != new interpretation" principle, not a shortcut around it.
  Amended the ledger to move Blast Radius (P1 in `docs/SOFKA_PARITY.md`)
  ahead of Context Diff (P3/DEFERRED in the same ledger, "scoring optional"
  even at RESEARCH time) — documented above under "Amendment to the
  proposed ledger" with the evidence and the explicit reasoning for why this
  is not a stop-and-ask fork. **Next: M11.1** (shared evidence snapshot
  foundation), continuing directly in this session.

- 2026-09-22: M11.1 (shared evidence snapshot foundation) implemented.
  Reconnaissance already confirmed the vocabulary (`evidence::*`,
  `resources::health::{Health,Severity}`) exists and needs no change, so the
  only new code is `src/resources/priority.rs`: `by_attention(&[SharedObject])
  -> Vec<SharedObject>` (stable sort by `Reverse(Severity)`, ties left in
  caller order exactly like `resources::sort::rows`'s own documented
  convention of trusting the store's canonical order) and
  `severity_counts(&[SharedObject]) -> SeverityCounts` (four exhaustive
  buckets, `total()`/`problems()` helpers) for Pulse's own tiles. No new
  struct wraps `Object`/`Health`; both functions take a plain slice and
  return owned `Arc` clones, so there is nothing here that can itself go
  stale — the caller (`State::rows` after `prepare()`) already owns the
  epoch/scope-safety M11.1's own contract asked for. 6 new unit tests:
  fixed-input ordering (Critical, Warning, Unknown, Healthy), a broader
  every-problem-before-every-healthy invariant check (not just the one fixed
  case), stable-tie-order regression, exhaustive severity-count
  reconstruction of `rows.len()`, empty-scope zero-counts (never a default
  "healthy" claim), and same-name/different-UID never merging into one row
  (this project's `UID != NAME` invariant, directly regression-tested at
  this new layer rather than only trusted-by-composition). 394 unit tests
  total (up from 388), fmt/clippy clean.

- 2026-09-22: M11.2 (Eye) implemented. New `src/eye.rs::report(rows,
  resource, caveats) -> (String, Vec<adjacent::Target>)` -- a pure function,
  reusing `resources::priority::by_attention`/`severity_counts` for
  ordering/summary and `adjacent::Target` verbatim for navigation (zero new
  navigation mechanism: `Document.navigate`'s existing Up/Down-steps-through-
  targets/Follow-jumps-to-UID code already handles any `Document` with a
  non-empty `.adjacent`, unchanged). New `ScopeCaveat` enum
  (`NotYetSynced`/`Incomplete`/`UnknownFieldsExcluded(n)`) is Eye's own
  explicit "this scope may not be the whole truth" signal, deliberately kept
  separate from `evidence::Unknown` (that enum is about one *value* being
  unknown; this is about the *row set itself* being possibly incomplete) --
  reconnaissance's own "a new enum only with explicit justification" bar,
  met here in a comment, not silently added. New `Action::Eye`/`:eye`
  registry entry (table mode, key `e`), `Runtime::open_eye` builds the
  `Document` synchronously from `state.rows`/`state.resource`/the three
  caveat sources already on `State` -- no async task, no `Payload::Document`
  round trip, unlike Explain/Adjacent/Xray, because there is nothing to
  fetch. 8 new unit/app tests: fixed-order and broader
  every-problem-before-healthy invariant checks, Unknown-never-rendered-as-
  healthy, caveats always stated explicitly (including the empty case,
  stated as explicitly as the non-empty one), same-name/new-UID
  non-collapse, navigable-target UID correctness, real-`Runtime`
  `Action::Eye` wiring proving every row becomes a navigable target, and a
  synchronous-completion proof (`rt.tasks.is_empty()` after the action,
  unlike every async document action). **Live evidence** against
  `kind-sauron-test`/`sauron-fixtures` (14 real Pods: CrashLoopBackOff,
  ImagePullBackOff ×3, a genuinely `Failed` Job Pod, an `Unschedulable`
  Pod, 7 Healthy): `:eye` correctly ordered all 6 Critical rows first, the
  1 Warning next, 7 Healthy last, each with its real evidence line
  (container waiting reason, restart count, scheduling message); `Down`
  then `Enter` (Follow) navigated the table cursor to the exact UID of the
  2nd priority row (`m5-job-failing-9ppwq`), not by name; 32x9 rendered
  without corruption; quit restored the terminal cleanly. 402 unit + 76
  fake-HTTP total, fmt/clippy clean.

- 2026-09-22: M11.3 (Pulse) implemented. New `src/pulse.rs::report(rows,
  caveats, metrics_summary) -> String` -- a pure function reusing
  `resources::priority::severity_counts` for the HEALTH tile,
  `app::metrics::Cache::summary()` (already-existing, unchanged) verbatim
  for the METRICS tile, and `eye::ScopeCaveat::text` (widened from private
  to `pub` so both views share one caveat vocabulary instead of two) for the
  SCOPE tile. Deliberately scoped narrower than `:info`: `:info` already
  covers connection/task/session/queue plumbing, so Pulse covers only what
  `:info` does not -- the current resource scope's own health/metrics
  evidence -- rather than duplicating it. v1 makes zero new Kubernetes
  requests: `Runtime::open_pulse` builds the report synchronously from
  `state.rows`/`state.metrics` exactly like `open_eye`, no async task, no
  new bounded collector added (reconnaissance's own M11.3 "decision 2" this
  slice confirmed rather than revisited -- no genuinely new bounded source
  was needed to make Pulse useful). New `Action::Pulse`/`:pulse` registry
  entry (table mode, key `o`), never requires a selected row. 5 new unit/app
  tests: zero-rows-is-explicit (never an implicit "all healthy" claim),
  metrics-unavailable tile textually distinct from metrics-sampled,
  exhaustive severity counts, caveats-never-silently-dropped (including the
  absence case stated as explicitly as presence), and a real-`Runtime`
  wiring test proving synchronous completion (`rt.tasks.is_empty()`) with no
  selection required. **Live evidence** against
  `kind-sauron-test`/`sauron-fixtures` (same 14-Pod scope as M11.2's own
  live check): `:pulse` correctly showed 6 Critical/1 Warning/0 Unknown/7
  Healthy, a real `Metrics PARTIAL: 6/6 fresh; omitted=8 malformed=0` tile
  (the bounded metrics collector had only pinned/sampled 6 of the 14 rows at
  that point -- shown as PARTIAL, never silently rounded up to "all
  sampled"), and "No caveats" for a fully synced scope; 32x9 rendered
  without corruption; quit restored the terminal cleanly. 407 unit + 76
  fake-HTTP total, fmt/clippy clean.

## Final acceptance checklist

- [ ] Every M11.1-M11.8 slice implemented and individually ACCEPTED in this
      document's own Journal (or, for context diff only, explicitly
      evidence-backed DEFERRED, mirroring M9.6's precedent — never silently
      dropped).
- [ ] No second health/relationship/mutation-safety/config/evidence engine
      exists anywhere in the M11 diff — every new module composes an
      already-accepted primitive, traceable to this document's
      reconnaissance section.
- [ ] Eye/Pulse never convert partial/unknown evidence into healthy/zero;
      proven live against a real RBAC-limited or forbidden scope, not just a
      fake-HTTP substitute.
- [ ] Eye's ordering is the existing, explainable `Severity` `Ord` — no
      opaque/numeric score anywhere.
- [ ] Blast radius output labels every relationship by its actual
      `graph::Provenance` (or an explicit "policy/operational risk" note)
      and never uses causal/predictive language — proven by a positive text
      assertion in the acceptance script, not just absence-of-banned-word.
- [ ] Blast radius executes no mutation and is not reachable as a bypass of
      the normal M7/M8/M8B/M10 confirmation flow.
- [ ] Evidence bundle: a live sensitive fixture value is confirmed absent
      from the actual exported bytes (not a unit-level mock claim).
- [ ] Evidence bundle: overwrite is refused by default and only proceeds on
      explicit confirmation; permissions (`0600` files, `0700` directory);
      no path traversal; bounded size; deterministic manifest.
- [ ] Context diff (if implemented) makes comparison-key vs. identity
      semantics explicit in its own output text, not just in code comments.
- [ ] All M11 work stays bounded and cancellable; no unbounded fanout; no
      new continuous watch architecture.
- [ ] Production sees zero writes across the entire milestone.
- [ ] 32x9 works for every new view. Terminal restoration works.
- [ ] Full M1-M10 regression (`accept-m*.py`, unmodified) passes, including
      recreating either isolated kind cluster if drift is found (matching
      M10.9's own established remediation, not silently skipped).
- [ ] Combined M11 acceptance (`accept-m11.py`) run twice clean.
- [ ] M11 soak completes with recorded observations under the "observed
      stability only" honesty standard — no leak-freedom claims.
- [ ] Docs reconciled: this file, `HANDBOOK.md`, `docs/RUNBOOK.md`,
      `README.md`, `docs/ROADMAP.md` checkpoint line, `docs/SOFKA_PARITY.md`
      rows for every slice actually shipped.
- [ ] `docs/M9B_A_ACCEPTANCE.md`/`docs/M9B_B_ACCEPTANCE.md` confirmed still
      PLANNED — NOT STARTED and untouched by any M11 change.
- [ ] Worktree clean.
- [ ] Local annotated tag `m11-accepted` created — never pushed without
      explicit authorization.
