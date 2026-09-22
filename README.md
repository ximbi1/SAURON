# SAURON

**One eye over the entire cluster.**

SAURON is a from-scratch, keyboard-first terminal console for Kubernetes,
written in Rust.

It is built around one governing principle:

> **UNKNOWN ≠ ZERO ≠ HEALTHY.**
>
> Absence of evidence is never converted into a fabricated value, and a
> cluster state SAURON cannot justify is never called healthy by default.

SAURON helps operators see what exists in a cluster, understand what needs
attention, inspect the evidence behind that judgment, and — where policy
allows — act on it.

It is read-only by default and never performs an operational or mutating
action without passing explicit safety and policy boundaries.

---

## Status

SAURON was built as a 12-milestone project.

M1 through M12 are **ACCEPTED** (M9.6 and M12.3 explicitly deferred, see
below), with local annotated milestone tags (`m1-accepted` through
`m12-accepted`) and live verification against isolated Kubernetes `kind`
clusters.

M12 was the final milestone of the current roadmap.

| Milestone | Scope | Status |
| --- | --- | --- |
| M1 | Read-only foundation: kubeconfig, discovery, live resource table | ACCEPTED |
| M2 | Context/namespace switching, generic resource commands, history | ACCEPTED |
| M3 | Typed filters/sort, documents, Events, CRD printer columns | ACCEPTED |
| M4 | Interactive sessions: logs, exec, shell, attach, port-forward | ACCEPTED |
| M5 | Evidence-driven metrics, deterministic health, Explain 2.0, Timeline | ACCEPTED |
| M6 | Relationship graph, adjacent resources, Xray | ACCEPTED |
| M7 | Central mutation policy, guardrails and operation journal (infrastructure only — no mutation workflow yet) | ACCEPTED |
| M8 | Guarded mutation workflows: Scale, Restart, Delete, Label, Annotate, with post-commit verification | ACCEPTED |
| M8B | Advanced cluster operations: Cordon/Uncordon, Set image, CronJob trigger, Evict, Drain, Force delete | ACCEPTED |
| M9 | Flux, Argo CD and Helm integrations | ACCEPTED (M9.6 Helm rollback/uninstall DEFERRED — see below) |
| M10 | Bulk workflows, workspaces, bookmarks, themes and keymaps | ACCEPTED |
| M11 | Eye, Pulse, evidence bundles, context diff and blast-radius analysis | ACCEPTED |
| M12 | Trust-gated plugin execution, headless hardening, packaging and performance | ACCEPTED (providers explicitly DEFERRED — see below) |

See [`HANDBOOK.md`](HANDBOOK.md) for the full engineering record,
[`docs/M12_ACCEPTANCE.md`](docs/M12_ACCEPTANCE.md) for M12's own detailed
journal, [`docs/INSTALL.md`](docs/INSTALL.md) for installing a packaged
release, and [`docs/`](docs/) for architecture notes, feature contracts
and milestone acceptance ledgers.

**M12.3 (providers: Prometheus/VictoriaMetrics/VictoriaLogs adapters) is
explicitly DEFERRED**, not implemented — investigated and found to be a
genuinely large new integration surface with zero existing scaffolding,
at this project's own lowest priority tier, not named in this milestone's
own acceptance contract. See `docs/M12_ACCEPTANCE.md`'s M12.3 journal
entry.

**M12.5 (packaging) is proven live for Linux x86_64 only.** The other
three platforms (Linux aarch64, macOS x86_64, macOS aarch64) are defined
in `.github/workflows/release.yml` (`workflow_dispatch`-only, never
triggered) but not built or verified this milestone — this development
machine has no cross-linker toolchain and no macOS hardware.

---

## What SAURON does today

### Live Kubernetes resource inspection

SAURON uses the real Kubernetes watch API to maintain a continuously updated
resource view.

It supports built-in Kubernetes resources as well as dynamically discovered
CRDs.

Resource identity is based on canonical Kubernetes API identity and object
UIDs rather than names alone, preventing same-name object replacements from
being silently treated as the same object.

---

### Typed filters

Filters use typed values rather than treating everything as strings.

Examples:

    cpu>100m
    memory>256Mi
    age>1h
    label.team=platform
    qos=Burstable
    cpu/%r>80

SAURON uses three-valued Strong Kleene logic.

A value can be:

    TRUE
    FALSE
    UNKNOWN

Missing information is therefore not silently converted into zero, false or
an empty value.

UNKNOWN values are excluded from positive filters and remain explicitly
counted.

---

### Typed sorting

Sorting shares the same typed value system as filtering.

CPU, memory, quantities, percentages, ages and other supported values are
sorted according to their actual type rather than lexicographically.

UNKNOWN values always sort last.

This remains true in both ascending and descending order.

---

### Generic Kubernetes resources and CRDs

SAURON discovers Kubernetes resources dynamically.

It tracks:

- API group
- version
- resource
- kind
- scope
- shortnames
- supported verbs

CRDs can therefore be inspected without requiring a dedicated Rust type.

SAURON also supports a deliberately safe subset of CRD
`additionalPrinterColumns`, evaluated against live objects.

Unsupported or ambiguous JSONPath expressions are omitted rather than guessed.

---

### Documents

SAURON has a shared document viewer used by several inspection workflows.

Current document types include:

- redacted YAML
- Describe
- Events
- Explain
- Logs
- Help

The viewer supports:

- search
- wrapping
- horizontal scrolling where appropriate
- fullscreen viewing
- bounded content
- freshness information
- UID-pinned refresh
- stale/replaced-object rejection

Refreshing a document never silently switches to a new object that happens to
reuse the same namespace/name.

---

## Metrics

SAURON includes an optional bounded Kubernetes Metrics API collector.

It currently supports live CPU and memory usage for:

- Pods
- containers
- Nodes

It also exposes static Kubernetes resource accounting:

- CPU requests
- memory requests
- CPU limits
- memory limits
- Node capacity / allocatable values
- QoS class
- usage as percentage of requests
- usage as percentage of limits

Examples of fields available to the filter/sort engine include:

    cpu
    memory
    cpu/r
    mem/r
    cpu/l
    mem/l
    cpu/%r
    mem/%r
    cpu/%l
    mem/%l

Missing, forbidden, stale or unavailable metrics never become zero.

They remain UNKNOWN with an explicit reason.

A denominator of zero never becomes `0%` or infinity.

It remains UNKNOWN.

Usage above 100% of a declared request is preserved as-is rather than clamped.

---

## Deterministic health

SAURON does not use AI, fuzzy heuristics or hidden numerical health scores to
decide whether a Kubernetes object is healthy.

Health is a deterministic function over actual Kubernetes fields.

The general precedence model is:

    deletion
      >
    terminal failure
      >
    container failure
      >
    scheduling / init problems
      >
    readiness degradation
      >
    progressing
      >
    healthy
      >
    unknown

Rules are implemented per resource kind rather than forcing every Kubernetes
resource through one generic interpretation.

Current health coverage includes:

- Pods
- Deployments
- StatefulSets
- DaemonSets
- ReplicaSets
- Jobs
- Nodes
- PersistentVolumeClaims
- PersistentVolumes
- Namespaces

Pod health understands evidence including:

- phase
- init containers
- restartable init sidecars
- readiness
- scheduling state
- CrashLoopBackOff
- ImagePullBackOff
- ErrImagePull
- OOMKilled
- restart counts
- container waiting state
- current terminated state
- previous terminated state

Workload health accounts for fields such as:

- metadata.generation
- status.observedGeneration
- desired replicas
- updated replicas
- ready replicas
- available replicas
- unavailable replicas
- StatefulSet revisions
- StatefulSet partitioning
- OnDelete strategy
- DaemonSet rollout state
- Job completion/failure conditions

Node condition polarity is interpreted explicitly.

For example:

    Ready=True

is positive evidence, while:

    MemoryPressure=True
    DiskPressure=True
    PIDPressure=True

are negative evidence.

Missing evidence is not treated as healthy.

Every non-trivial health result carries evidence citing the Kubernetes fields
that produced it.

---

## Explain 2.0

Explain is an evidence report, not a speculative diagnosis engine.

Its guiding model is:

    Explain =
        deterministic health findings
        + fresh evidence
        + bounded correlation

Explain reuses the exact evidence produced by the health engine rather than
implementing a second parallel health interpretation.

It can additionally collect:

- fresh selected-object state
- UID verification
- correlated Events
- current Pod/Node metrics
- directly owned child Pods
- workload-to-Pod evidence

Ownership correlation uses Kubernetes `ownerReferences` with UID validation.

It never guesses ownership based on name prefixes.

For Deployments, the bounded relationship is:

    Deployment
        -> ReplicaSet
        -> Pod

For StatefulSets, DaemonSets, ReplicaSets and Jobs:

    workload
        -> Pod

Failures in individual evidence sources are additive rather than destructive.

For example, a report may contain valid health evidence while clearly stating:

    PARTIAL EVIDENCE: Events forbidden

or:

    PARTIAL EVIDENCE: owned Pods unavailable

Metrics are descriptive evidence only.

High CPU usage is not automatically presented as the cause of a failure.

A lack of failure evidence also does not prove health.

---

## Timeline

SAURON maintains a bounded, session-local Timeline of meaningful Kubernetes
state transitions.

Timeline is:

- UID-scoped
- bounded
- session-local
- based on observed state transitions

Timeline is NOT:

- Kubernetes Events
- an audit log
- a stream of every resourceVersion
- metrics history

Meaningful changes currently include fields such as:

- health state
- Pod phase
- container reasons
- restart totals
- generation
- observedGeneration
- replica state
- image changes
- deletion timestamp transitions

Timeline distinguishes how a change was learned:

    [watch]
    [relist]

A reconnect/relist never fabricates intermediate history.

If SAURON last observed state A and reconnects to discover state D, it records:

    A -> D

It does not invent:

    A -> B -> C -> D

unless those intermediate states were actually observed.

Objects with the same namespace/name but different UIDs receive separate
histories.

---

## Relationships (Adjacent)

SAURON builds a bounded relationship graph around the selected object,
opened with:

    :adjacent
    a

The graph is built only from sources SAURON can verify. It never infers a
relationship from name similarity or IP address alone.

Recognized relationship sources are:

- `ownerReferences`, UID-validated
- explicit typed references (Pod `nodeName`, ConfigMap, Secret,
  ServiceAccount, PersistentVolumeClaim, and PersistentVolume /
  StorageClass for storage objects)
- Service/Pod selector matches, evaluated against real labels
- status-backed references Kubernetes itself reports (EndpointSlice
  `targetRef`, PersistentVolume `claimRef`)

Relationships are grouped by what they actually are, never blended
together:

    OWNED BY
    OWNS
    SELECTED BY
    REFERENCES
    REFERENCED BY

A selector match is never presented as ownership. An IP address alone
never creates an edge — only an explicit, verifiable reference does.

`Enter` navigates to the related object under the cursor using its exact
canonical identity (API version, kind, namespace, name and UID), never by
name alone. Navigation integrates with the existing history stack, so
Adjacent traversal is a normal, reversible move rather than a special case.

---

## Xray

Xray extends Adjacent into a bounded, cycle-safe traversal, opened with:

    :xray
    x

It walks the same verified relationship sources 1 to 3 hops from the
selected object, grouped by hop distance. A visited-identity set guarantees
the traversal terminates even if the underlying graph contains a cycle.

Each node in the traversal shows the same deterministic health used
everywhere else in SAURON. Xray does not compute a second, parallel "graph
health," and a related object is never presented as the cause of a
problem — relationship is not causation.

---

## Guarded mutation workflows (M8)

SAURON ships five guarded, user-facing mutation workflows, each a
first-class semantic operation — never a generic patch console, never
kubectl passthrough:

    :scale N            -- Deployment / StatefulSet / ReplicaSet
    :restart            -- Deployment / StatefulSet / DaemonSet (rolling restart)
    :delete             -- Pod / Deployment / StatefulSet / DaemonSet / ReplicaSet / Job / CronJob / ConfigMap
    :label KEY=VALUE / KEY-      -- set or remove one label
    :annotate KEY=VALUE / KEY-   -- set or remove one annotation

Every one of them walks through the exact same pipeline: intent → policy
→ preview → optional server dry-run → confirmation (a double key-press
for destructive/cluster-critical operations, a single press otherwise) →
the single execution gateway with fresh before-mutation revalidation →
a redacted, append-only local journal → exactly one bounded, cancellable
post-commit verification. Commit outcome and verification outcome are
always shown as two distinct facts (`COMMIT RESULT` / `VERIFICATION`) —
a verification that times out or observes something unexpected never
retroactively turns a successful commit into a failure, and readiness/
health stays a separate concern (see Explain/Timeline above), never
conflated with "the write was accepted."

SAURON has no in-app setting that marks a cluster as verified for
mutation from its name alone; that verification is deliberately kept
external (an explicit `--mutation-test-cluster-verified` flag, set only
by the guarded test-cluster script after independently proving cluster
identity via Docker/API introspection). Production stays hard-denied by
the existing default-readonly posture regardless.

Two read-only views remain available for inspecting the pipeline itself:

    :policy
    u

shows what a hypothetical Modify or Delete on the selected object would do
under the real policy engine — fully local, zero network.

    :mutations
    m

shows the bounded, most-recent local mutation journal (including the
verification record correlated to each commit) — also fully local, zero
network. On a fresh installation it is empty.

See [`docs/M8_ACCEPTANCE.md`](docs/M8_ACCEPTANCE.md),
[`docs/MUTATION_POLICY.md`](docs/MUTATION_POLICY.md) and
[`docs/MUTATION_JOURNAL.md`](docs/MUTATION_JOURNAL.md) for the full
contract. [`docs/M8B_ACCEPTANCE.md`](docs/M8B_ACCEPTANCE.md) (Cordon/
Uncordon, Set image, CronJob trigger, Evict, Drain, Force delete — now
ACCEPTED end to end) and
[`docs/FUTURE_MUTATIONS.md`](docs/FUTURE_MUTATIONS.md) (families
intentionally deferred, e.g. Secret/RBAC mutation — not started, not a
roadmap commitment) capture the extended mutation surface beyond M8's
own first pass.

---

## Events

Events are correlated using Kubernetes object identity rather than loose name
matching.

Current Events behavior includes:

- UID correlation
- Warning-only toggle
- fieldPath display
- bounded result count
- explicit partial-result handling
- timestamp fallback handling
- stale object protection

Events remain separate from Timeline because they represent different sources
of truth.

---

## Logs

SAURON provides native Kubernetes log streaming.

Current support includes:

- regular containers
- init containers
- ephemeral containers
- previous container logs
- multiple visible sources
- follow
- pause/resume
- clear
- search
- filtering
- wrapping
- fullscreen
- bounded memory
- output sanitization

Pause freezes the displayed view while bounded ingestion continues.

Log streams are owned tasks and are cancelled when their session ends.

---

## Exec

SAURON supports one-shot Kubernetes exec commands.

Example:

    :exec container -- /bin/sh -c "echo hello"

Exec is:

- policy gated
- UID pinned
- container aware
- structured
- cancellable
- denied before connection under read-only policy

---

## Interactive shell

SAURON can temporarily hand the terminal to a real Kubernetes exec session.

Interactive shell handling includes:

- terminal mode suspension/restoration
- alternate-screen handling
- resize propagation
- Ctrl-C forwarding
- Ctrl-D
- remote process exit
- Pod deletion during session
- network interruption
- exact terminal restoration after exit

The terminal is returned to SAURON after the remote session finishes.

---

## Attach

SAURON supports attaching to already-running interactive containers.

Attach reuses the same terminal-handoff machinery as interactive shell.

A local:

    Ctrl-]

detaches from the session without terminating or signalling the remote process.

---

## Port-forward manager

SAURON includes a native background Kubernetes Pod port-forward manager.

Examples:

    :forward 8080
    :forward 8081:8080
    :pf
    :pf_stop ID

Current guarantees include:

- loopback-only listeners
- automatic free local-port allocation
- explicit local/remote ports
- multiple simultaneous forwards
- bounded clients per forward
- Pod UID validation
- same-name replacement protection
- forwarding survives foreground navigation
- forwarding survives context/view changes
- explicit lifecycle ownership
- individual stop
- clean shutdown

Port forwarding is Pod-based in the current implementation.

Service resolution is planned for later milestones.

---

## Context and namespace navigation

SAURON supports interactive Kubernetes context and namespace navigation.

View state is epoch-scoped.

Results arriving from an old context or resource scope are discarded even if
network cancellation races with a new request.

This prevents stale asynchronous responses from contaminating the current
view.

---

## History

Navigation history supports bounded back/forward movement between views.

History preserves resource identity, scope and relevant query state.

Selections are UID-aware.

A deleted object or a same-name replacement cannot silently inherit the
selection of the old incarnation.

---

## Safety model

Safety is a core architectural property, not a confirmation dialog added at
the end.

SAURON is read-only by default.

The CLI option:

    --readonly

is a hard override.

It cannot be disabled by cluster-specific configuration and survives config
reloads.

Operational capabilities such as:

- exec
- shell
- attach
- port-forward

must pass policy before establishing their operational connection.

Every mutation (Scale/Restart/Delete/Label/Annotate, M8) passes through
one central pipeline:

    intent
      -> policy
      -> preview
      -> optional server dry-run
      -> confirmation
      -> identity revalidation
      -> Kubernetes API
      -> outcome journal
      -> post-commit verification

This central mutation gateway was built in M7 and is exercised by every
M8 workflow — none of them call the Kubernetes API directly.

---

## Secrets and sensitive data

Secret bodies are redacted before display.

Redaction also covers embedded data in fields such as:

    kubectl.kubernetes.io/last-applied-configuration

Kubeconfig credentials, tokens, certificates and private keys must never be
printed into diagnostics or logs.

SAURON does not claim that arbitrary application logs can never contain
secrets; terminal sanitization and redaction are defense-in-depth mechanisms,
not magical secret detection.

---

## Architecture

SAURON is a single Rust package with a thin CLI and cohesive internal modules.

High-level data flow:

    terminal input
        |
        v
    input / keymap
        |
        v
    command
        |
        v
    application state
        |
        +-------------------------------+
        |                               |
        v                               v
    task supervisor                 rendering
        |
        +-> discovery
        +-> watches
        +-> metrics
        +-> evidence
        +-> logs
        +-> sessions
        |
        v
    bounded messages
        |
        v
    reducer
        |
        v
    object store
        |
        +-> health
        +-> timeline
        +-> typed rows
        |
        v
    Ratatui UI

Important invariants include:

- one application owner mutates state
- network workers send immutable messages
- every view has an epoch
- stale async results are discarded
- UID identifies object incarnation
- namespace/name identifies object slot
- relists are staged before publication
- rendering performs no Kubernetes API calls
- bounded channels prevent unbounded work
- network tasks are explicitly owned
- shutdown cancels and joins/aborts owned work
- Secret contents are redacted before presentation

---

## Current milestone roadmap

### M1 — Read-only foundation

Accepted.

Delivered:

- kubeconfig loading
- Kubernetes client
- discovery
- live watch
- staged relist
- UID-aware object store
- table rendering
- basic projections
- read-only architecture

---

### M2 — Navigation and dynamic resources

Accepted.

Delivered:

- context picker
- namespace picker
- generic resource commands
- dynamic GVR discovery
- aliases and shortnames
- deterministic ambiguity handling
- CRDs
- navigation history
- epoch-scoped async work

---

### M3 — Query and inspection

Accepted.

Delivered:

- typed filters
- Strong Kleene UNKNOWN semantics
- typed sorting
- shared document viewer
- Events
- CRD additionalPrinterColumns subset
- bounded search
- UID-pinned document refresh

---

### M4 — Interactive operations

Accepted.

Delivered:

- advanced logs
- session ownership
- one-shot exec
- interactive shell
- attach
- native Pod port-forward manager
- terminal handoff guard
- combined adversarial acceptance
- 75-minute soak

---

### M5 — Evidence-driven inspection

Accepted.

Delivered:

- shared evidence/UNKNOWN vocabulary
- Kubernetes Metrics API collector
- CPU/memory usage
- request/limit accounting
- QoS
- usage percentages
- deterministic health
- health evidence
- Explain 2.0
- bounded ownership correlation
- descriptive metrics evidence
- UID-scoped Timeline
- relist-aware meaningful transition recording
- combined adversarial acceptance
- 75-minute soak

M5 final verification:

    105 unit tests
    19 fake HTTP tests
    12 combined live adversarial sequences
    combined sequence executed twice
    full M1-M4 regression
    75-minute soak
    1800 soak cycles
    0 failures
    stable RSS
    stable file descriptors
    stable thread count

Local annotated tag:

    m5-accepted

The tag has not been published.

---

### M6 — Relationship graph / Adjacent / Xray

Accepted.

Delivered:

- bounded relationship graph: `ownerReferences` (UID-validated), explicit
  typed references, Service/Pod selector matches, and status-backed
  references (EndpointSlice `targetRef`, PersistentVolume `claimRef`)
- storage relationships: PersistentVolumeClaim / PersistentVolume /
  StorageClass, both directions
- bounded reverse reference scans for ConfigMap, Secret, ServiceAccount
  and PersistentVolumeClaim across built-in workload kinds
- `:adjacent` grouped relationship view (OWNED BY / OWNS / SELECTED BY /
  REFERENCES / REFERENCED BY), with canonical-identity navigation
- `:xray` bounded (1-3 hop), cycle-safe traversal reusing the same
  deterministic health, never a second "graph health"
- explicit provenance on every edge; no name-similarity or IP-based
  inference
- combined adversarial acceptance
- 75-minute soak

M6 final verification:

    128 unit tests
    28 fake HTTP tests
    18 combined live adversarial sequences
    combined sequence executed twice
    full M1-M5 regression
    75-minute soak
    1263 soak cycles
    0 failures
    RSS +0.15% over the full run (allocator noise, not a leak)
    stable file descriptors
    stable thread count

Local annotated tag:

    m6-accepted

The tag has not been published.

M6 remains read-only.

---

### M7 — Central mutation policy

Accepted. Infrastructure only — no user-facing mutation workflow ships in
this milestone; that is M8.

Delivered:

- deterministic mutation identity, effect and risk model, reusing the same
  incarnation-safe scope already used by owned sessions
- central policy engine: fixed gate order, every applicable reason
  accumulated, UNKNOWN never silently means Allow
- confirmation contract bound to the exact intent (context, namespace,
  GVR/GVK, name, UID, effect, payload hash) — invalidated by any change
- local preview, server dry-run, and commit kept as three distinct,
  non-implicit phases
- one central execution gateway: re-evaluates policy at commit time,
  revalidates the target's live UID/resourceVersion immediately before
  mutating, journals before and after, bounded/cancellable
- explicit outcome states, preserving "definitely not committed" vs.
  "commit outcome cannot be proven" as genuinely distinct
- durable, redacted, append-only local mutation journal
- `:policy` and `:mutations` read-only TUI surfaces
- one narrowly-scoped internal proof mutation, live-verified twice against
  the isolated `kind-sauron-test` fixture only
- combined adversarial acceptance and 75-minute soak

M7 final verification:

    152 unit tests
    37 fake HTTP tests
    13 combined live interactive scenarios
    combined sequence executed twice
    one live proof-mutation test executed twice
    full M1-M6 regression
    75-minute soak
    1071 soak cycles
    0 reconnects
    RSS +1.2% over the full run (allocator noise, not a leak)
    stable file descriptors
    stable thread count

Local annotated tag:

    m7-accepted

The tag has not been published.

No M8 mutation workflow bypasses this layer — every one of them
(Scale/Restart/Delete/Label/Annotate) is built strictly on top of it.

---

### M8 — Guarded mutation workflows

**ACCEPTED.**

Shipped: `:scale`, `:restart`, `:delete`, `:label`, `:annotate` — see
[Guarded mutation workflows (M8)](#guarded-mutation-workflows-m8) above
and [`docs/M8_ACCEPTANCE.md`](docs/M8_ACCEPTANCE.md) for full evidence.

Combined acceptance summary:

    scripts/accept-m8.py (14 scenarios), run twice
    full M1-M7 regression
    75-minute soak
    604 soak cycles
    0 reconnects, 0 transient errors
    RSS +2.3% over the full run (allocator noise, not a leak)
    stable file descriptors, stable thread count

Local annotated tag:

    m8-accepted

The tag has not been published.

Cordon/uncordon, set image, CronJob trigger, evict, drain, and force
delete were subsequently implemented and ACCEPTED end to end as M8B —
see [`docs/M8B_ACCEPTANCE.md`](docs/M8B_ACCEPTANCE.md). Bulk actions
remain scoped to M10; editor-based/arbitrary resource changes are
catalogued as intentionally deferred in
[`docs/FUTURE_MUTATIONS.md`](docs/FUTURE_MUTATIONS.md).

---

### M9 — GitOps and Helm

ACCEPTED: M9.0-M9.5 and M9.7. M9.6 explicitly DEFERRED (not a blocker —
see below). Full record in [`docs/M9_ACCEPTANCE.md`](docs/M9_ACCEPTANCE.md).

Read-only inspection, live-verified against a dedicated second `kind`
cluster (`sauron-m9`, real Flux v2.9.5 and Argo CD v3.5.3 — `kind-sauron-test`
stays untouched as the M1-M8B regression environment):

- Flux: Kustomization, HelmRelease, GitRepository, OCIRepository,
  HelmRepository, Bucket — conditions rendered verbatim (never
  collapsed to a single "Ready" flag), the `-1` "never reconciled"
  sentinel shown distinctly from ordinary staleness, suspend state,
  revisions, `dependsOn`, `sourceRef`.
- Argo CD: Application (single/multi-source), ApplicationSet, AppProject
  — sync/health/operation state, managed-resource summary.
- Helm: release decode (chart, revision, status, values, manifest
  identity) via a single explicit, bounded, TOCTOU-safe reader — see
  below.

Guarded mutation actions, through the same M7/M8 policy/preview/
confirm/commit/verify gateway as every other mutation in SAURON:

- Flux: `:flux_suspend`, `:flux_resume`, `:flux_reconcile`.
- Argo CD: `:argocd_sync`, `:argocd_refresh`, `:argocd_rollback` —
  discovered live to be plain Kubernetes API PATCH operations on
  `Application.operation`, needing no separate Argo API server call.

**A deliberate security exception for Helm.** Helm releases are stored
entirely inside a Kubernetes `Secret`'s `data.release` field, but
SAURON's generic object pipeline (`resources::Object::new`) always
redacts Secret bodies — an absolute rule that stays unchanged. Reading
a Helm release therefore needed one narrow, explicit exception: a
single `kube::helm::read_release` function, invoked only on an explicit
`:helm` request against an already-identified release, that performs
one bounded fetch, re-verifies the object's UID and type against that
*fresh* response (never trusting a cached selection), and returns only
a fully sanitized view — no raw Secret body, decoded bytes, or chart
notes ever leave that one function. See M9_ACCEPTANCE.md's own M9.5
write-up for the full contract.

**M9.6 (Helm rollback/uninstall) is explicitly DEFERRED**, not
approximated. Investigation found no maintained Rust crate implements
Helm's own client-side rollback/uninstall logic, Helm v3+ has no
server/API component to target instead (Tiller was removed in Helm v3),
and hand-reimplementing that logic directly against the release
Secret's storage record risks corrupting Helm's own bookkeeping in ways
that could break the real `helm` CLI's ability to operate on a release
afterward. None of shelling out to `helm`, direct storage manipulation,
or a reduced-scope reimplementation were judged acceptable inside M9's
scope. A future "Native Helm Engine" milestone is noted as backlog —
not started, not a roadmap commitment — should anyone later want to
take on Helm-compatible lifecycle reimplementation as its own project.

---

### M10 — Bulk workflows, workspaces, bookmarks, keymaps, themes

ACCEPTED: M10.0-M10.9. Full record in
[`docs/M10_ACCEPTANCE.md`](docs/M10_ACCEPTANCE.md).

- **Bounded multi-select** (`V` select-visible, `i` invert, `C` clear,
  up to 500 targets) driving **bulk mutations** (`bulk_label`,
  `bulk_annotate`, `bulk_scale`, `bulk_restart`, `bulk_delete`,
  `bulk_evict`, `bulk_cordon`/`bulk_uncordon`, `bulk_set_image`,
  `bulk_trigger`) — every target goes through the exact same unmodified
  M7/M8/M8B policy/preview/confirm/commit/verify gateway individually,
  one at a time; there is no bulk-only bypass path, and no target is
  ever authorized merely because another target in the same operation
  was authorized. A bulk preview shows each target's own eligibility
  (`ELIGIBLE`/`EXCLUDED`) before any confirmation keypress.
- **Workspaces** (`workspace_save`/`workspace_open`/`workspace_delete`/
  `workspace_list`): saved navigation intent (context, namespace,
  resource, filter, sort) — never live `Resource` data, never
  mutation-authorization state (verified-cluster flag, confirmations,
  in-flight workflows, credentials). Opening a workspace replays the
  intent through the normal watch/rebuild pipeline; it never restores a
  prior cursor selection.
- **Bookmarks** (`bookmark_save`/`bookmark_open`/`bookmark_delete`/
  `bookmark_list`): a saved reference to one specific object by UID,
  with an explicit status (`Exact`/`Replaced`/`Missing`/
  `NotCurrentlyViewed`) rather than a silent same-name reattachment.
- **Configurable keymaps and themes**, both fail-safe: a malformed
  keymap or theme in `config.toml` never crashes startup — it falls
  back to defaults and shows a visible, actionable warning instead
  (naming both colliding actions for a keymap conflict; the unknown
  theme name for a theme). A non-color channel (a distinct glyph per
  severity) means the `mono` theme still distinguishes every health/
  safety state without relying on color at all.
- **Config persistence**: `Config` gained its first real write path —
  workspaces/bookmarks are saved atomically (temp file, `0600`
  permissions, then `rename()`) to the same `config.toml` a pre-M10
  file already used, with a `version` field for future migrations. A
  pre-M10 config file with none of these fields loads unchanged.

Live-verified against the M1-M8B regression cluster (`kind-sauron-test`)
with a dedicated `sauron-m10` namespace and fixtures: bulk label/delete
committed and individually verified via `kubectl`; workspace/bookmark
round trips through the real TUI; config persistence proven across a
real process restart against the same `--config` path; malformed
keymap/theme configs proven not to crash a real startup; combined
acceptance run twice clean; full M1-M9 regression reconfirmed green; a
300-second bounded soak (178 cycles of selection/workspace/bookmark
churn, zero reconnects, zero transient errors, flat RSS/fd/thread).

---

### M11 — Eye / Pulse / evidence bundle / blast radius / context diff

ACCEPTED: M11.0-M11.8 (M11.6 context diff at a reduced scope — see below).
Full record in [`docs/M11_ACCEPTANCE.md`](docs/M11_ACCEPTANCE.md).

M11 is a composition milestone: it builds five thin, evidence-preserving
lenses over what M1-M10 already compute, reusing accepted primitives
directly rather than building parallel ones. No AI diagnosis, no numeric
score, no second health/relationship/mutation-safety engine.

- **Eye** (`:eye`) — a priority-ordered view of the currently watched
  scope: `resources::priority::by_attention` sorts by the existing,
  explainable `Severity` (Critical, Warning, Unknown, then Healthy — no
  opaque scoring), and each row's "why" is `Health.evidence` verbatim.
  Zero new Kubernetes requests — a pure re-render of already-loaded state,
  reusing `adjacent::Target` for navigation so Follow jumps to the exact
  UID. An explicit `CAVEAT` line covers a not-yet-synced, bound-hit, or
  RBAC-partial scope — proven live against a real RBAC-forbidden
  `ServiceAccount`: Eye never renders a false "0 problems, all healthy."
- **Pulse** (`:pulse`) — refreshed HEALTH/METRICS/SCOPE tiles for the same
  scope, deliberately narrower than `:info` (which already covers
  connection/task plumbing). Also zero new requests.
- **Evidence bundle** (`:bundle PATH [--force]`) — a local, bounded,
  redacted export of one target's health/Explain/Events/relationships/
  Timeline/metrics to a directory plus a manifest, reusing the exact same
  collectors Explain/Events/Adjacent already use. Atomic per-file writes
  (`0600`, directory `0700`), no silent overwrite. Live-proven: exporting
  a real Secret fixture with a known plaintext sentinel value produces a
  bundle whose bytes, grepped in full, never contain that sentinel.
- **Blast radius** (`:blast_radius`) — a read-only safety lens over the
  exact same bounded relationship graph Xray already collects, grouped by
  `graph::Provenance` with safety-oriented labels (`VERIFIED OWNERSHIP/
  DEPENDENCY`, `EXPLICIT REFERENCE`, `SELECTOR-DERIVED (INFERENCE)`,
  `STATUS-REPORTED`). Never causal or predictive language — a fixed
  disclaimer states the invariant in the report itself. No mutation, not
  reachable as a confirmation bypass.
- **Context diff** (`:context_diff CONTEXT`) — investigated per this
  project's own M9.6 "investigate before deferring" precedent, then
  accepted at a reduced scope rather than deferred: a bounded, on-demand
  comparison of one target's kind/namespace/name against another named
  context, on a temporary second connection `Runtime` never stores. The
  comparison key is stated in the report's own text as explicitly NOT
  identity.

Live-verified against `kind-sauron-test`/`sauron-fixtures`:
`scripts/accept-m11.py` (8 sequences) passed twice clean; a 300-second
bounded soak (`scripts/soak-m11.py`) ran 103 cycles with zero reconnects
and zero transient errors, flat RSS/fd/thread.

---

### M12 — Extensibility and distribution

ACCEPTED: M12.0-M12.8 (M12.3 providers explicitly DEFERRED — see below).
Full record in [`docs/M12_ACCEPTANCE.md`](docs/M12_ACCEPTANCE.md). The
final milestone of the current roadmap.

- **Plugins** (`:plugin NAME`) — the first local-subprocess boundary this
  codebase has ever had. A plugin is an explicit, user-approved executable
  with a fixed argv template — never an ad-hoc shell string; no `sh -c`/
  `eval`/shell interpolation. `Trust::{Approved, Disabled}` defaults to
  `Disabled`; there is deliberately no "warn but run" state, mirroring
  `readonly = false`'s own explicit-approval gesture. A child process gets
  an explicit environment allowlist (`PATH`, `HOME`, `LANG` only) — no
  kubeconfig path, bearer token, or Secret value ever reaches it, proven
  live by approving a plugin that dumps its own environment and grepping
  its real captured output for a live fixture secret. Output is bounded
  (16 KiB/line, 500 lines total, matching `kube::logs`'s own clip
  convention); a configurable timeout and cancellation both kill the
  plugin's entire process group, not just the direct child — closing a
  real bug found live during development (`Sessions::drop`'s task-`abort()`
  path never reached the running future's own cooperative-cancel branch,
  so a `sh -c "... & wait"` grandchild could survive app quit; fixed with
  a dedicated `GroupKillGuard`). Task ownership/cancellation fully reuses
  `app::session::Sessions` verbatim.
- **Providers** — investigated, evidence-backed **DEFERRED**, not
  implemented. Zero existing scaffolding, no HTTP client dependency
  beyond `kube`'s own transitive plumbing, no Prometheus/VictoriaMetrics/
  VictoriaLogs instance in either isolated test cluster, and this
  project's own lowest priority tier. See `docs/M12_ACCEPTANCE.md`'s
  M12.3 journal entry.
- **Headless maturity** — `--output json|yaml` gained a `schemaVersion`
  key (additive only). Live-proven: no TTY needed, zero ANSI bytes,
  correct exit codes, `PARTIAL DISCOVERY` on stderr while data stays on
  stdout.
- **Packaging** — dual-licensed `MIT OR Apache-2.0`. `scripts/package.sh`
  produces a deterministic archive (binary + both LICENSE files +
  `docs/INSTALL.md`); proven live end-to-end for Linux x86_64 only —
  built, packaged, extracted, and run (`--version`, `info --offline`).
  The other three platforms (Linux aarch64, macOS x86_64/aarch64) are
  defined in `.github/workflows/release.yml` (`workflow_dispatch`-only,
  never triggered) but not built this milestone — no cross-linker
  toolchain or macOS hardware on the development machine.
- **Checksums / license inventory** — `scripts/checksums.py` produces
  `SHA256SUMS.txt`/`manifest.json`; tamper detection proven live in both
  directions. `scripts/license_inventory.py` audits all 305 dependencies
  via `cargo license`: 0 with a missing license, all permissive, no
  forced copyleft. See [`docs/LICENSE_INVENTORY.md`](docs/LICENSE_INVENTORY.md).
- **Performance** — `benches/pipeline.rs` gained a real cold-startup
  measurement (spawns the built binary, median of 5) and a 20,000-object
  sweep tier, every run now printing its own hardware/rustc/OS context.
  No comparative claims.

Live-verified against `kind-sauron-test`: `scripts/accept-m12.py` (7
sequences: real plugin stdout/exit code, live no-credential-leak proof,
live timeout and live mid-run cancellation each confirmed via a real
OS-process-table check, headless `schemaVersion`, packaged-archive
extract+run) passed twice clean; a 180-second bounded soak
(`scripts/soak-m12.py`) ran 64 cycles of Eye/Pulse/blast-radius/plugin
churn with zero reconnects and zero transient errors, flat RSS/fd/thread,
and an explicit post-shutdown process sweep confirming zero orphan
plugin processes.

---

## Building

Requirements:

- Rust toolchain
- Kubernetes kubeconfig
- network access to a Kubernetes API server

Build:

    cargo build --release

Binary:

    ./target/release/sauron

---

## Running

Default:

    ./target/release/sauron

Open Deployments across all namespaces:

    ./target/release/sauron deployments -A

Select context and namespace:

    ./target/release/sauron --context my-context pods -n my-namespace

Use a server-side label selector:

    ./target/release/sauron pods -n my-namespace -l app=web

Force read-only policy:

    ./target/release/sauron --readonly

Keyboard is the primary interface.

Inside SAURON:

    ?    effective keybinding help
    :    command palette

---

## Development checks

Run:

    cargo fmt --check

    cargo check --all-targets --locked

    cargo clippy --all-targets --locked -- -D warnings

    cargo test --all-targets --locked

Acceptance scripts live under:

    scripts/

including milestone acceptance and soak runners.

Live acceptance is performed against an isolated Docker `kind` cluster with
an explicit dedicated kubeconfig.

Fixture writes are guarded by:

    scripts/test-cluster.sh

The script validates the actual API endpoint against the expected Docker
container before allowing fixture mutation.

It never falls back to the default kubeconfig.

---

## Testing philosophy

A milestone is not accepted because it compiles.

It is not accepted because unit tests pass.

It is accepted only after the flows most likely to break it have been
demonstrated against a real controlled Kubernetes API.

When live acceptance discovers a real bug:

    reproduce
      -> classify
      -> identify root cause
      -> add regression coverage
      -> run full checks
      -> rebuild
      -> replay the exact live failure
      -> continue acceptance

SAURON records real limitations rather than silently retrying until a test
looks green.

---

## Development safety boundary

Development uses an isolated Kubernetes `kind` cluster.

The project's existing production cluster is strictly read-only during
development.

No fixture creation, mutation, exec, attach, helper Pod, upload, application
write, Helm action, GitOps action or destructive acceptance test is permitted
against production.

All mutable fixtures are isolated behind an explicit test kubeconfig.

---

## Documentation

### Core engineering documents

- [`HANDBOOK.md`](HANDBOOK.md)
  Architecture, invariants, decisions and engineering journal.

- [`docs/RUNBOOK.md`](docs/RUNBOOK.md)
  Current checkpoint and live testing procedures.

- [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md)
  Runtime architecture and subsystem boundaries.

- [`docs/SOFKA_PARITY.md`](docs/SOFKA_PARITY.md)
  Capability research/parity matrix.

- [`docs/RESEARCH.md`](docs/RESEARCH.md)
  Research baseline and source notes.

---

### Milestone acceptance

- `docs/M3_ACCEPTANCE.md`
- `docs/M4_ACCEPTANCE.md`
- `docs/M5_ACCEPTANCE.md`
- `docs/M6_ACCEPTANCE.md`
- `docs/M7_ACCEPTANCE.md`

M1 and M2 acceptance evidence is recorded in the engineering handbook.

---

### Feature contracts

- `docs/FILTERS.md`
- `docs/SORTING.md`
- `docs/DOCUMENTS.md`
- `docs/LOGS.md`
- `docs/SESSIONS.md`
- `docs/EXEC.md`
- `docs/PORT_FORWARD.md`
- `docs/METRICS.md`
- `docs/HEALTH.md`
- `docs/EXPLAIN.md`
- `docs/TIMELINE.md`
- `docs/RELATIONSHIPS.md`
- `docs/MUTATION_POLICY.md`
- `docs/MUTATION_JOURNAL.md`

---

## Design principles

SAURON follows a few deliberately strict principles:

1. UNKNOWN is not ZERO.
2. UNKNOWN is not HEALTHY.
3. Evidence comes before diagnosis.
4. Identity is UID-based.
5. Names do not identify object incarnations.
6. Partial evidence must remain visibly partial.
7. No API calls happen from rendering.
8. Work must remain bounded.
9. Safety decisions happen before operations.
10. Stale asynchronous results must never overwrite current state.
11. Reconnects must not invent history.
12. Read-only must actually mean read-only.
13. Relationship does not imply cause.
14. No mutation without policy; UNKNOWN never means allowed.
15. Confirmation is not authorization; dry-run success is not commit success.
16. A successful HTTP response is not a verified desired effect.

---

## Non-goals

SAURON is not intended to become:

- a hosted Kubernetes control plane
- a telemetry collection service
- a credential broker
- an automatic remediation system
- an AI-required diagnosis engine
- an embedded external security scanner
- a replacement for Kubernetes RBAC
- a system that guesses missing cluster state

AI may eventually be used as an optional consumer of structured evidence, but
SAURON's core understanding of cluster state must remain useful,
deterministic and inspectable without it.

---

## Philosophy

Kubernetes already exposes enormous amounts of information.

The difficult part is not inventing more information.

The difficult part is preserving identity, context, provenance and uncertainty
while turning that information into something an operator can understand
quickly.

SAURON should never make a cluster look simpler by lying about what it knows.

If a value is missing, it is missing.

If evidence is partial, it is partial.

If an object was replaced, it is a different object.

If a transition was not observed, SAURON does not pretend that it was.

One eye over the entire cluster.

But only what the eye can actually see.
