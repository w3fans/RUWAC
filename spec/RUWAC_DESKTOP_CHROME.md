# Desktop controls and machine shortcuts

The desktop uses a compact header whose session tabs shrink to fit available width.
Controls offers **Two rows of tabs** and **Compact tabs**; both are saved privately
and apply to detached windows. The new-terminal + stays outside the scrollable
rows. Extremely crowded rows scroll horizontally and reveal the selected tab.
Full titles remain in tooltips. Header layout never recreates session content.

The default is one row. Collapse actions at
its left edge to give tabs more room. Labels adapt to narrow windows. Controls opens
session, clipboard and layout actions for the selected view. F10 or Ctrl+. opens it;
Dock makes it a normal row below the header. There are no hover toggles over session
content. Opening the popover does not resize a terminal or remote desktop. Explicit
docking, fullscreen and window resizing can change the available viewport.

Appearance offers Graphite, Nocturne, Fjord, Ember, Paper and High contrast. The six
terminal font choices are enabled only when installed; an unavailable saved font
falls back to Monospace. Size is 9–18 pt (default 12); line spacing offers 1.15, 1.30
and 1.45. ANSI colors follow the palette, while explicit RGB and extended indexed
colors remain unchanged. Font metrics drive rendering, selection and PTY dimensions.
Preferences and Updates are reachable from the main menu. Session status is in tab
tooltips; latency is marked unmeasured rather than estimated.

Machines lists all matching profiles in one scrollable list, within the existing
1000-profile library limit. Group disclosure controls remember collapsed groups.
Search temporarily expands matching groups. RDP and SSH actions use configured
endpoints sharing the same machine identity; no SSH username or endpoint is inferred
from an RDP service login.

Edit an SSH endpoint and open Remote terminals / agents. Enable Codex and/or Claude
shortcuts and set their commands. Defaults are `codex resume --all` and
`claude --resume`. These open interactive resume pickers without submitting a prompt.
Custom commands are nonsecret metadata: never include passwords or tokens. Remote
agent authentication remains in each agent's own configuration on the remote host.

Choose RUWAC, GNU Screen, tmux or Direct as the session backend. RUWAC creates a named
persistent remote session; Screen and tmux use their shared attachment modes. Direct
has no remote multiplexer and exits with the SSH connection. The agent and selected
backend must be installed. A machine shortcut starts only after a click; refreshing
the library performs no remote probes or agent launches.

Install / update remote RUWAC CLI opens an explicit confirmation for the saved SSH
endpoint, then runs the existing headless source bootstrap installer in a terminal.
It needs Python 3, Git and Rust build prerequisites on that host, preserves running
sessions, and does not install agents, modify remote login services or elevate
privileges. SSH host-key checking and interactive authentication remain enabled;
agent forwarding and port forwarding are disabled for agent/bootstrap commands.

New RDP profiles default to Dynamic resolution; saved explicit fit/native/stretch
preferences remain respected. Select Dynamic in Controls to update an existing
profile. Display Control support is negotiated with the server, with local fit as a
fallback. Fullscreen is available in Controls and with Ctrl+Alt+Enter.
Ctrl+Alt+Home moves keyboard focus to local controls and releases remote held keys;
click the remote desktop to resume input. Right Alt/AltGr remains available for
international keyboard layouts.

UI preferences migrate schema 1 to 2 on save, preserving sidebar width/pinning and
collapsed groups. The existing bounded 1 MiB limit is retained for large collapsed-
group sets, with ownership, permissions, no-follow and atomic-write protections.
Unknown fields or invalid appearance values protect the existing file from writes.

Testing note: Weston 15.0.1 exited while positioning a Controls popup after repeated
resizes. That compositor path remains unqualified; isolated Mutter results do not
substitute for physical desktop acceptance.

GTK terminal multiplexer commands use Ctrl+Alt+A by default. Plain Ctrl+A reaches the child application, including tmux and Readline. CLI RUWAC continues to use Ctrl+A. Existing explicit shortcut overrides are retained. Main menu → About RUWAC / Support exposes the installed version, source, license and donation QR.


`tests/integration/tab_layout.py` exercises 20 and 40 generated terminal tabs on
an isolated 1800×1000 Mutter display, maximized and half-width. It checks both
row counts and densities, the visible + action, native pointer and keyboard
menus, reorder, drag-out, detach/return, selected-tab reveal, private preference
persistence and unchanged PTY PIDs. It does not use an owner desktop or claim
physical-device acceptance.
