RUWAC v0.4.73 — colored scrollback and visible COPR update failures

Closing a tab from its context menu now retains the menu parent until cleanup
finishes, avoiding a GTK lifetime failure exposed by the RDP regression tests.

Terminal history retains text colors and attributes while scrolling and selecting
beyond the visible page. Styles are requested only for history snapshots, stored
as bounded runs and a shared palette, and rendered only for visible rows. Existing
selection, soft-wrap copying and application mouse/Shift-override behavior remain.
Older engines and plain recovery checkpoints continue to work with plain history;
discarded checkpoint colors cannot be reconstructed.

Remote Fedora updates distinguish a current package, an available upgrade and a
failed COPR query. Targeted checks require the COPR repository to be reachable,
keep DNF diagnostics visible and choose enabled Fedora/updates or Rawhide
repositories for dependencies. Fedora package failures do not trigger a parallel
per-user installation. CLI and desktop upgrade together when both are installed.

The README includes permanent COPR enablement, a repository-only five-minute
metadata cache override, immediate refresh commands and default-engine replacement
instructions. Apply the repository setup on each client. Ordinary system updates
keep their existing unavailable-repository policy.

Generated native tests cover ordinary wheel scrolling, selection at both pane
edges, historical colors and PRIMARY copy. The published v0.4.72 negative control
reproduces the missing colors. A controlled 2,400-key A/B/A check with 20 terminal
windows and deep history passed latency, CPU and memory budgets; this is received
output timing, not physical GTK/RDP latency. External hardware acceptance remains
separate. No broad typing-speed improvement is claimed.

Fedora CLI/desktop packages and portable CLI archive/Debian packages follow the
existing release gates. Portable targets include Ubuntu 22.04/24.04 and Debian
12/13; real Proxmox-host acceptance and an APT repository remain open. The known
RSA advisory and maintenance warnings remain visible and unsuppressed.

Updates now offers **Open updated engine**, including COPR installations. It
uses the current installed CLI, preserves the existing engine, and starts with
independent history. Update both the GUI and engine for colored history. Reopening a GUI can reuse its
older persistent engine. `ruwac engine gui` opens a separate updated engine without
ending existing work. Replacing the default engine ends any programs it still
owns; follow the README from an external terminal after closing those programs.
No live engine is automatically stopped or migrated by this update.
