# Port forwarding — M4.3 contract

Native Pod-only TCP forwarding. `f` / `:forward` selects a declared TCP port;
`:forward REMOTE` allocates a local port atomically via bind(127.0.0.1:0).
`:forward LOCAL:REMOTE` requests an explicit port. No public bind option, no IPv6
in this slice, no local process killing. `:pf` shows background forwards; `:pf_stop ID`
stops exactly that session. Service resolution, saved/autostart and reconnect are deferred.
Default keys: `f` starts the declared-port picker from a Pod table, `P` opens the
manager, `s` in the manager opens the stop-by-ID prompt. Help comes from the effective
keymap. Manager uses shared document scrolling/search/wrap/fullscreen; Refresh reads
local session state, not another resource lookup. Scroll down to see older outcomes.

Conservative policy: readonly denies creation before network/listener work. Stop remains
available under readonly because it removes local access. CLI --readonly must remain a
hard override on reload as well as reconnect. No operational tests against production.

Ownership: reuse Sessions' IDs/task supervisor but use independent cancellation tokens,
not the view's scope token. Each forward retains its original Connection, context,
cluster, canonical v1/pods identity, namespace/name/UID and ports. A per-forward bounded
watch channel carries latest status independently of the current view's event channel.
It cannot deliver object rows or documents into a new scope. Navigation never migrates it.

Four active forwards maximum, eight concurrent TCP clients per forward. Each accepted
client gets a separate kube Portforwarder because its stream is one TCP connection,
not a reusable listener. A dedicated RAII owner calls abort on every drop path (kube
4.2.0 itself has no abort-on-drop). Normal completion aborts then joins the internal
task; cancellation aborts it before discarding the wrapper. All connection futures
live inside the session, without detached application tasks.

Check target UID before binding, before/after each WebSocket upgrade and every two
seconds while listening. Missing/terminating/terminal-phase/replaced/unverifiable target stops the
entire forward, dropping listener and clients. Kubernetes offers no atomic UID
precondition for the portforward subresource: this is best-effort identity validation,
not an absolute race-free API guarantee. No retry/retarget by name, ever.

"Listening" means a local port is bound; remote connectivity is established per TCP
client. Per-client connection/protocol errors and capacity rejections are visible in
the manager. Transport setup and identity reads use the configured API deadline.
An active stream has no arbitrary lifetime timeout; it remains bounded and cancellable.
Stopping cancels local work, not application transactions performed through a tunnel.

Config reload applies policy to each forward's original context/cluster and stops it
if that policy becomes readonly. A navigation-only switch to a readonly context does
not revoke a different context's existing forward. Latest progress is bounded to one
value per session; completion uses the supervisor rather than the UI event queue.
The manager retains at most 32 records; active entries appear before completed history.

Acceptance requires real HTTP-over-TCP, automatic/explicit/conflicting ports, view/ns/
context changes, logs, multiple sessions, stop, deletion/replacement, repeated cycles,
and absence of OS listeners after quit. No acceptance from compilation or mocks alone.
