# Filters (M3 item 1)

`/` opens a local expression editor. Enter applies a valid AST atomically; Esc cancels
the draft. An invalid expression leaves the previous filter, selectors, scope and history
unchanged. Clear the draft with Ctrl-U. Root Esc clears a filter, then quits if pressed again.

Examples:

```text
api                         fuzzy subsequence, case-insensitive
"api-worker"                substring, case-insensitive
/^api-.*$/                  bounded case-insensitive regex
NOT /canary/                inverse predicate
label.app                   key existence (empty string value still exists)
label.example.test/Team=Ops case-sensitive key and value
age>1h30m AND restarts>=3
(name=api OR name=worker) AND NOT label.canary
field:/spec/enabled=true
field:/spec/count>=8
number:field:/spec/ratio>1.2
cpu:field:/spec/cpu>100m
memory:field:/spec/memory=1024Mi
percent:field:/spec/percent>=75%
```

`NOT` / `!` binds tightest, then `AND` / `&&` / adjacent terms, then `OR` / `||`.
Parentheses override precedence. Uppercase operator words are reserved; quote them to
search as text. Comparisons `=`, `==`, `!=`, `<`, `<=`, `>`, `>=` are case-sensitive
(a deliberate distinction from fuzzy/substring/regex search). Quoted RHS preserves spaces.

Built-in field names are case-insensitive. Labels and JSON Pointer paths are not.
`field:/...` reads scalar JSON from the already-redacted cached object. JSON Pointer
escaping is `~1` for `/` and `~0` for `~`; array indexes work, wildcards/JSONPath do not.
Native JSON numbers/bools retain their types; object/array/null/absent values are unknown.
Explicit casts: `integer`, `number`/`float`, `count`, `duration`, `cpu`, `memory`,
`percent`, `bool`. Counts are nonnegative u64; integers use exact i128 comparisons;
floats/quantities are finite f64 (not arbitrary precision accounting). Boolean literals
are `true` and `false`. Percentages use percentage points, so `75` equals `75%`.
Duration supports composite `s m h d w`; CPU/memory accept decimal/binary SI and exponent
notation in base cores/bytes; no `%` quantities. Invalid known-type literals are syntax
errors; a value not convertible to the declared type evaluates UNKNOWN.

Known fields: name/namespace/status/age, projected columns such as restarts, plus explicit
JSON Pointer. `age` is seconds since creation internally and duration literals externally.
Missing Pod status/restart entries are unknown, not zero; restart total covers declared
regular and init containers, including completed init containers. CPU/memory *usage*
fields are unavailable until a future metrics collector; Node `cpu/a` and `mem/a` are
real allocatable fields, not usage metrics.

## Unknown and server scope

Strong Kleene logic: NOT UNKNOWN is UNKNOWN; FALSE AND UNKNOWN is FALSE;
TRUE OR UNKNOWN is TRUE; other combinations involving UNKNOWN remain UNKNOWN.
Only TRUE matches are displayed. The title `?N` and status `unknown excluded: N` report
indeterminate rows. Snapshots report `unknownExcluded` (JSON/YAML) and a stderr notice.
That count applies to cached rows inside the server scope, not unseen/RBAC-inaccessible
objects. An absent label existence predicate is FALSE; its value comparison is UNKNOWN.
An omitted labels map on valid metadata means no labels, but malformed metadata is unknown.

```text
:pods -n sauron-fixtures -l app=healthy -f status.phase=Running / age>1m
```

Selectors are separate literal Kubernetes label/field selectors, passed unchanged to both
list and watch; the AST is never translated/pushed down. Kubernetes validates supported
field selectors; failures are visible, never retried without selectors. Explicit resource
commands start a new query (local filter/selectors empty unless supplied); namespace,
context, Refresh and history preserve the active query's selectors/filter. History stores
canonical GVR metadata; same-catalog restores never re-resolve aliases.

Kubernetes contracts consulted: [labels and selectors](https://kubernetes.io/docs/concepts/overview/working-with-objects/labels/),
[field selectors](https://kubernetes.io/docs/concepts/overview/working-with-objects/field-selectors/),
[quantities](https://kubernetes.io/docs/reference/kubernetes-api/definitions/quantity-resource/).
Local missing-value inequality deliberately differs from server label `!=` semantics:
use `NOT label.key OR label.key!=value` when absence should match locally.
