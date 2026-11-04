RUWAC v0.4.58 reduces terminal frame traffic and adds useful terminal controls and remote Web service previews.

Terminal clients can use compact complete frames while preserving exact glyphs,
colors, styles and input modes. Older clients continue receiving plain frames;
updated clients accept plain replies from preserved engines. Expansion has cell
and byte limits. Oversized aggregate views keep plain frames, including views
spanning independently zoomed canonical grids. Polling, socket framing and SSH timing/privacy settings are
unchanged. Existing daemons and terminal programs do not need to be restarted.

Generated A/B/A tests measured local TUI medians of 16–19 ms before versus 11 ms
in the candidate, and nested SSH GUI-protocol medians of 52–54 ms versus 33 ms.
Local frame traffic fell from about 158 KB to 12 KB per key. Local GUI median
varied with host load; these measurements exclude physical display latency and
real-network RTT. The dated design/results documents record controls and limits.

The new explicit-target command subset includes tmux-style window/pane navigation,
splitting, renaming and literal input, plus Screen-style stuff/paste/split/info and
session-buffer controls. Stable identities and strict flags reject ambiguous
requests; literal text is not submitted automatically. The command inventory
lists supported subsets and remaining differences, without claiming full tmux or
Screen scripting/configuration compatibility.

On a terminal opened through a verified saved SSH machine, choose “Open remote
web service…” and enter the port and optional path of the app listening on that
machine's localhost. The preview uses that terminal's original SSH connection
and a temporary local-loopback forward. No manual ssh -L or replacement login is
needed. Invalid ports, foreign-host paths and changed connection identities are
refused. Closing a preview closes its forward while keeping the terminal alive.

Reopen the GUI and reattach a TUI frontend to load new client behavior; preserve
running programs. New engine-side compression applies when using an updated
engine; preserved engines remain supported through plain frames.

The known RSA RUSTSEC-2023-0071 advisory remains unresolved and unsuppressed.
Physical/external-host, full viewer/controller and broader product qualification
remain open. Published-artifact verification and installation are separate from
source publication.
