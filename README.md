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

SAURON is being built as a 12-milestone project.

M1 through M5 are currently **ACCEPTED**, with local annotated milestone
tags (`m1-accepted` through `m5-accepted`) and live verification against an
isolated Kubernetes `kind` cluster.

M6 is the next milestone and has not started yet.

| Milestone | Scope | Status |
| --- | --- | --- |
| M1 | Read-only foundation: kubeconfig, discovery, live resource table | ACCEPTED |
| M2 | Context/namespace switching, generic resource commands, history | ACCEPTED |
| M3 | Typed filters/sort, documents, Events, CRD printer columns | ACCEPTED |
| M4 | Interactive sessions: logs, exec, shell, attach, port-forward | ACCEPTED |
| M5 | Evidence-driven metrics, deterministic health, Explain 2.0, Timeline | ACCEPTED |
| M6 | Relationship graph, adjacent resources, Xray | NOT STARTED |
| M7 | Central mutation policy, guardrails and operation journal | NOT STARTED |
| M8 | Guarded mutation workflows | NOT STARTED |
| M9 | Flux, Argo CD and Helm integrations | NOT STARTED |
| M10 | Bulk workflows, workspaces, bookmarks, themes and keymaps | NOT STARTED |
| M11 | Eye, Pulse, evidence bundles, context diff and blast-radius analysis | NOT STARTED |
| M12 | Plugins, providers, headless workflows, packaging and performance hardening | NOT STARTED |

See [`HANDBOOK.md`](HANDBOOK.md) for the full engineering record and
[`docs/`](docs/) for architecture notes, feature contracts and milestone
acceptance ledgers.

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

Future mutations will pass through a central pipeline:

    intent
      -> policy
      -> RBAC
      -> preview
      -> confirmation
      -> identity revalidation
      -> Kubernetes API
      -> outcome journal

This central mutation gateway is planned for M7.

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

Not started.

Planned scope includes:

- Kubernetes ownership graph
- resource adjacency
- upstream/downstream relationships
- Service relationships
- workload relationships
- config relationships
- storage relationships
- bounded graph traversal
- relationship evidence
- Xray inspection
- blast-radius foundations

M6 remains read-only.

---

### M7 — Central mutation policy

Not started.

Planned scope includes:

- central mutation gateway
- operation intent
- policy evaluation
- RBAC checks
- previews
- confirmations
- UID/resourceVersion preconditions
- final identity revalidation
- operation journal
- uncertain-outcome handling
- cancellation semantics

No mutation workflow should bypass this layer.

---

### M8 — Mutation workflows

Not started.

Planned workflows include:

- delete
- scale
- rollout restart
- set image
- editor-based resource changes
- CronJob trigger
- suspend/resume
- Node cordon/uncordon
- drain
- selected bulk actions

Every workflow must use the M7 safety pipeline.

---

### M9 — GitOps and Helm

Not started.

Planned scope includes native inspection and guarded operations for:

- Flux
- Kustomizations
- HelmReleases
- GitRepository
- OCIRepository
- Bucket
- image automation
- Argo CD Applications
- ApplicationSets
- Helm releases
- revisions
- reconciliation state
- ownership/dependency relationships

---

### M10 — Operator workflow layer

Not started.

Planned scope includes:

- bulk workflows
- marks/selections
- workspaces
- bookmarks
- saved views
- remembered sorts
- configurable keymaps
- themes
- favorites
- richer navigation
- global object finder
- clipboard actions

---

### M11 — Eye / Pulse / evidence intelligence

Not started.

Planned scope includes:

- Eye overview
- cluster Pulse
- evidence bundles
- context comparison
- baseline diff
- blast-radius analysis
- related-resource summaries
- bounded operational snapshots
- portable diagnostic bundles

These remain evidence-driven features.

No required AI diagnosis is planned.

---

### M12 — Extensibility and distribution

Not started.

Planned scope includes:

- plugins
- external providers
- structured plugin protocol
- bounded plugin execution
- headless workflows
- machine-readable output
- packaging
- installation paths
- compatibility hardening
- performance campaigns
- larger-scale benchmarks
- release engineering

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
