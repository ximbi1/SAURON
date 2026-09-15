# Documents

One model (`src/app/document.rs`) backs every text overlay: Yaml, Describe, Explain,
Events, static help, and Pod logs. `j`/`k`/`PageDown`/`PageUp`/`g`/`G` move by visual
line; `←`/`→` (`h`/`L`) scroll horizontally, only while wrap is off (turning wrap back
on and scrolling is a real error, not a silent no-op). `w` toggles wrap, `F` toggles
fullscreen, `/` searches (case-insensitive substring), `n`/`N` cycle matches, `r`
refreshes, `Esc` returns to the table.

Wrapping is grapheme-aware (`unicode-segmentation`/`unicode-width`), not byte- or
char-based, so combining marks and wide/emoji graphemes wrap correctly. The visual-line
layout is cached and only recomputed when `(revision, width, wrap)` changes — resizing
or toggling wrap relayouts once, not every frame. Lines, bytes, visual segments, and
search matches are each independently bounded; exceeding a bound sets a `PARTIAL`
status flag rather than growing unbounded or silently dropping data.

`Freshness` (`Local` / `Snapshot(time)` / `Refreshing` / `Error`) is shown in the status
line alongside line/column/wrap/search state. Refresh reuses the exact UID-pin check
`kube::evidence::document()` already used for the initial open: it re-GETs by name and
rejects if the UID no longer matches (deleted, or replaced by a same-named different
object), rendering a clear "NOT CURRENT" screen instead of stale or silently-wrong
content. Logs have no refresh `Source` (they're a stream, not a point-in-time read) and
refreshing one errors clearly rather than attempting a GET.

Opening the command palette from within a document no longer discards it: the document
moves into `Runtime.palette_document` while the palette is open (showing the table
underneath, unchanged) and is restored on `Esc` or a failed command; a successful
navigation command replaces it as normal, and no history entry is created either way —
history is scope navigation, not document view state.

A per-action validation error (e.g. the wrap-off-required rejection) is transient input
feedback and uses `State.input_error`, the same field filter-input rejections use, not
`State.error` — a genuine transport/watch error must keep surviving unrelated
successful key presses until the watch actually recovers, so it is never written to the
same field a one-off document/action rejection uses.
