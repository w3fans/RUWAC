# Protocol v0

Local Unix stream: unsigned 32-bit big-endian payload length, then exactly that many UTF-8 JSON bytes. Frames must be 1..1,048,576 bytes. One response per request; subscriptions continue with event frames on the same connection. Bad JSON returns INVALID_REQUEST; impossible lengths close the connection before allocation. Read/write deadlines and at most 64 connections bound resources. Peers must have the daemon's effective UID. Parent directory is owned/private, socket 0600, start lock exclusive.

```json
{"version":0,"id":"req-1","method":"pane.sendText","params":{"pane":"pane_0123456789abcdef0123456789abcdef","text":"literal $HOME ž"},"idempotency_key":"optional-operation-key"}
```

Responses contain version, matching id and exactly one of result/error. Errors carry code and message. Version mismatch is VERSION; malformed typed target INVALID_ID; absent typed target STALE_ID. IDs are opaque `session_`, `window_`, `pane_`, `client_` plus 32 random hexadecimal digits. Names are display labels and never implicit targets.

| Method | Params | Result/behavior |
| --- | --- | --- |
| health/version/capabilities/schema | {} | status, version, explicit limitations, discovery |
| session.create | name; optional argv string array, rows, cols | session/window/pane IDs and child PID |
| session.list | {} | stable IDs, active window, dimensions |
| session.attach | session; optional rows, cols | registers this connection as a view |
| session.detach | {} | detaches only this connection |
| window.create | session; optional name, argv | new window/pane/PID; creator selects it; existing views keep their focus |
| window.list | session | ordered windows and active pane |
| window.rename/select | window; name for rename | updates explicit window |
| window.cycle | session; optional previous boolean | ordered caller view focus; unattached calls set canonical defaults |
| pane.create | window; optional argv | nested horizontal/vertical split; max sixteen panes per window |
| pane.list | session | IDs, PID, dimensions, ended flag |
| pane.select/close | pane | selects or closes explicit pane; last pane removes its window |
| pane.sendText | pane, text | queued_bytes; never appends Enter |
| pane.sendKey | pane, key | Enter, Tab, Escape, CtrlA, CtrlC, Up, Down |
| pane.capture | pane; optional lines (1..2000, default 250) | canonical text; does not promise raw original output |
| pane.wait | pane, contains; optional lines, timeout_ms (max 60000) | waits on state notifications for captured text; TIMEOUT if absent |
| buffer.set/get | session; text for set | internal buffer only |
| buffer.paste | pane | internal text, bracketed when application enables it |
| client.resize | rows, cols | attached view size; minimum dimensions win |
| client.render | optional revision | active window's canonical grid or unchanged |
| client.input | pane, kind, text | keyboard/mouse/paste; disabled mouse reports discarded in actor |
| events.subscribe | {} | initial ack, then state.changed revisions |
| daemon.stop | {} | terminates this daemon and its owned panes |

Text is UTF-8, max 256 KiB; NUL and oversize are rejected. Send acknowledges queue admission, not application execution. Direct argv spawning avoids `sh -c` interpolation; the default shell follows the user account and loads its normal interactive startup files. Queued bytes are subject to the child's line discipline unless the child enters raw mode. Send literal Enter separately. Internal paste respects bracketed-paste mode; embedded paste end markers are rejected by client.input.

Idempotency keys are bounded (1024-entry FIFO), daemon-lifetime, and scoped to exact method/params. Replays return cached outcomes; changed requests yield IDEMPOTENCY_CONFLICT. Connection-local view/input/render operations and shutdown cannot use them. Keys do not survive daemon restart; a timeout can leave a mutation outcome unknown. Use a stable key when retrying a supported mutation. No durable transaction/exactly-once claim. Attached selection, zoom/layout, resize and view-dependent list keys have a fresh attachment namespace. Shared object creation and text mutations retain global deduplication. A replay returns the original outcome without replaying later UI focus changes. Reattaching starts a new selection namespace.

Grid cells are arrays `[text,wide_spacer,foreground,background,bold,inverse]`; colors are RGB triplets or indexed/named numbers. Capture strips trailing cell spaces and separates rows with newlines. Independent attached-client focus and minimum attached size are documented in the architecture; automation always names its target. Revision events coalesce output and mutations, not individual raw-byte delivery.

Remote use can pipe a JSON request into `ssh EXISTING_HOST ruwac request` using existing authenticated SSH. The request body stays on stdin. Host selection, remote installation, transfer and disconnect behavior have not been tested on a second host; there is no custom TCP listener or implemented --host convenience flag.


### Optional Codex process probe

`agent.probe` takes `{"pane":"pane_ID"}` and returns `pane`, `adapter` (`codex`), `foreground_pid`, `process_detected` and `selected`. Discover support from `schema.methods`. This read-only operation reuses the bounded same-UID foreground-process-group check; it neither reads arguments/environment/terminal contents nor changes selection. Process presence does not establish login, model readiness, draft acceptance or image consumption. Extra fields, stale/wrong IDs and idempotency keys are rejected.

### Natural terminal grids

Protocol 0 keeps the 1 MiB request/default-response ceiling. capabilities and schema now advertise terminal_size_limits (rows, cols, cells) and max_render_response_bytes. Current limits are 512 rows, 1024 columns and 262144 cells; session creation, attachment and resizing validate the whole area before mutation. Missing capability metadata selects the legacy 100-row/200-column client bounds.

Only client.render may opt into a larger reply using max_response_bytes (1 MiB through 64 MiB). The client must read that explicit bound while continuing to validate version and request correlation. Invalid bounds return INVALID_PARAMS. An over-budget encoded reply returns a small correlated LIMIT response before any frame bytes are written. Other responses and all requests retain their existing limit. Outstanding large renders have a separate two-permit budget and bounded waiting/write deadlines; BUSY requests can retry reads later without replaying input. This does not alter PTY ownership, authentication, focus or endpoint identities.

`pane.capture` accepts optional `format: "ansi"` (default `"plain"`). ANSI
capture returns SGR styling only, never OSC, hyperlinks, cursor commands or input.
It observes the selected active/primary buffer without attachment or resizing,
and retains complete latest rows within 64 KiB. Saved checkpoint histories
retain plain text only and therefore return plain text in this mode. Older
daemons ignore the extra field and return their ordinary plain capture.
`contains` is matched against plain captured text regardless of the output
format. `pane.wait` remains plain so match semantics do not depend on style.
