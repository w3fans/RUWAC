RUWAC v0.4.59 reports running engine versions and opens separate updated-engine workspaces.

Package updates preserve existing terminal processes. Those terminals keep using
their original daemon, so installing new files alone does not activate backend
fixes. Run `ruwac engine status` to distinguish the installed CLI from the running
engine. GUI terminal footers also identify their local engine; this information
is cached at connection time rather than queried while typing or rendering.
`ruwac --host HOST engine status` discovers an updated CLI and its engine on the
selected verified SSH endpoint. Older remote CLIs must be updated first.

Run `ruwac engine gui` for new work on the updated engine. It opens an independent
window with a separate socket, workspace and durable terminal checkpoint. The
original window, daemon and terminal programs keep running; sessions are not
migrated. `ruwac engine new` provides equivalent terminal-only launch arguments.
Retain both included RUWAC_SOCKET and RUWAC_STATE_FILE settings when reconnecting.
The new engine's shells inherit its socket, so plain RUWAC commands address the
correct engine. The checkpoint override stays with the frontend and engine; it
is removed from new PTY shells so nested daemons cannot accidentally reuse it.
Explicit custom checkpoint overrides are refused rather than
silently shared. A new engine can activate the palette frame encoding while an
older preserved engine continues serving its existing clients.

Agent attachment tests now retry only Linux ETXTBSY when starting copied test
executables, for at most two seconds. Other spawn errors fail immediately.
Production process launching is unchanged. Tests cover a real open writer,
eventual execution, a persistent busy error and an unrelated permission error.

Release qualification retains the existing 19 mandatory categories and adds
real-process engine coexistence, concurrent startup, durable recovery, refusal,
actual palette frame and isolated mapped-window preservation checks. Published
artifact verification is a separate delivery step. Physical input, broader
external-host qualification and the remaining development plan are still open.
The known RSA timing advisory remains unresolved and unsuppressed.
