# Logs (M4.1 contract, ACCEPTED)

Basic `l` / `:logs [container]` and previous logs are accepted. M4.1 extends this with
explicit init/ephemeral names, `:logs *` (all containers of the selected Pod), and
`:logs_visible` (all known containers of the currently visible Pod rows). Source sets
are frozen by UID when opened, never automatically extended after a rollout. Refuse
more than eight sources rather than silently sample. Workload/Service resolution and
marked selections remain deferred; no claim of full Sofka aggregation parity.

Every line identifies namespace/Pod/container. Timestamp text from Kubernetes remains
unchanged; merge order is arrival order, **not globally timestamp-sorted**. Tail is 300
per source, including previous logs. No automatic reconnection/retargeting. Read-only
logs are permitted; their contents may contain application secrets and are never
automatically exported or traced.

Bounds: 16 KiB source line, 4 KiB read chunk, 256 shared event slots, at most 64 messages
per UI iteration, shared viewer 5,000 lines/4 MiB. Queue saturation backpressures reads;
cancellation interrupts queue waits. Oldest retained lines evict with a visible count.
Pause freezes following, not ingestion: the bounded tail continues updating and older
lines can evict. Clear discards retained lines, not the connection. Search is a literal
case-insensitive substring; optional matching-line filter uses that same search.

UID is checked before and after opening a stream and periodically while active. A
failed/forbidden identity check stops that source. The log API has no UID precondition:
these are best-effort checks, not an atomic Kubernetes contract. Never reconnect by
name. A failed source is reported while independent sources can continue.

Effective-keymap actions: logs-only pause, clear, matching-line filter; inherited
document search/next/previous/wrap/fullscreen; Refresh restarts the original frozen
sources with new request/session identity. Live acceptance evidence is in
[M4_ACCEPTANCE.md](M4_ACCEPTANCE.md).
