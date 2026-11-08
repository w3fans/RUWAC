RUWAC v0.4.72 — stable tabs, working tab actions and terminal recovery

- Fix tab context-menu actions being discarded when the popover closes: Reconnect, Disconnect and Close tab now dispatch before menu cleanup. Native-input regression coverage includes cancellation, the selected target and neighboring connections.

Keep tab widths steady while hovering, changing titles/status or switching tabs.
Short names use shorter tabs when the tab set changes. Keep close buttons visible
on inactive tabs, add clear borders, and retain readable text and 16px connection
icons in normal and compact layouts. Hold Alt for gentle 1–9/0 shortcut hints;
Map keys mode continues to pass these keys to the remote application.

An ended last local terminal stays open with its output and recovery controls.
Recreate starts a fresh local shell (or uses the saved remote launch), New terminal
keeps the ended tab, and New & close opens a replacement before closing it.
A failed creation keeps the old tab open. Existing sessions are preserved.

Native generated-desktop acceptance covers steady geometry, close controls,
pinning, one/two rows, crowded layouts, shortcut routing and terminal recovery.
These checks use generated terminals; physical owner acceptance remains open.
No new typing-latency improvement is claimed in this GUI patch.

Fedora CLI/desktop packages and the portable CLI archive/Debian package follow
the established release gates. Portable CLI tests cover Ubuntu 22.04/24.04 and
Debian 12/13; native Proxmox-host acceptance and an APT repository remain open.
The known RSA advisory and maintenance warnings remain visible and unsuppressed.

Reopen the GUI after updating to load these presentation changes. The persistent
engine and its running programs can stay running.
