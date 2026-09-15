# Sorting

`S` / `:sort` cycles visible columns; `I` / `:reverse` reverses direction.
`:sort NAME[:asc|:desc]` chooses a column. Bindings come from the central registry and
can be remapped; these are the defaults. `:sort count:field:/spec/count:desc` also accepts
the typed JSON Pointer field syntax described in FILTERS.md (the field need not be shown).

Numbers/counts compare numerically, booleans false then true, quantities in base units,
percentages in percentage points, and age in elapsed seconds (ascending = youngest first).
Strings use case-sensitive lexical order. Native mixed JSON numeric values compare without
rounding integer values to float; other mixed types order numbers, booleans, text.
Absent/null/invalid values sort **last in both directions**, never as zero or empty text.
Exact i128 integers and u64 counts; quantities/explicit floats have finite f64 precision.

Stable ties follow the store's canonical namespace/name order in either direction; reverse
changes the primary comparison only, not tie or unknown ordering. Values are computed once
per row per rebuild. Sort does not change history, GVR, watch epoch or selected UID.
History retains sort and UID; pending initial lists must not prematurely clear selection.
Deletion/replacement clears selection and a later same-name creation does not auto-select.
Explicit resource navigation resets to NAME ascending; namespace/context/history keep sort.

Rebuild memoization includes view epoch, store revision, filter and sort. A timer rebuilds
only age-predicate membership; ordinary key movement/rendering does not re-sort or query
Kubernetes. Generic server Table column types are the next presentation slice, not yet
claimed here.
