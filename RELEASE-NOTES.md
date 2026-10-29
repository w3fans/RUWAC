RUWAC v0.4.51 avoids rebuilding and transmitting unchanged terminal screens when
only hidden panes produce output. Visible output, window names, focus, resizing,
input modes and process exit continue to update. The regression includes multiple
clients and a workload with 19 panes and four render clients. Checkpoint disk writes
and broader remote typing latency remain separate work.

The desktop dependency libz-sys is updated to 1.1.31 for RUSTSEC-2026-0418: its
compiler probe no longer logs the build environment. Updating cannot remove output
from an earlier build; builders should remove affected build caches separately.
The known RSA advisory RUSTSEC-2023-0071 remains disclosed and unsuppressed.

Version 0.4.50 added mouse selection through history with wheel and edge dragging.
Inside an attached RUWAC session, press Ctrl+A then [ and select without Shift;
hold the mouse button and use the wheel or top edge to extend into older output.
Shift selects the outer terminal instead. Older history without colour metadata
remains plain text.

File updates preserve existing daemons and their running programs. The new
daemon-side performance change takes effect when a new daemon starts; reopening
only the GUI does not replace an old daemon. Avoid stopping a daemon that still
owns work you need. Isolated acceptance does not replace physical-input,
external-host or full platform qualification.
