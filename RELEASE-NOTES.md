RUWAC v0.4.53 adds explicit non-sizing terminal viewers. Attach from a smaller
terminal with `ruwac attach --session SESSION_ID --viewer` (`-O`), locally or
through the existing authenticated `--host` SSH path. Viewer resize, navigation
and local zoom preserve the shared PTY dimensions; larger grids are cropped to
the local terminal, with a visible viewer/cropping indicator.

Viewer input and copy remain available. Shared pane creation, closure and split
changes are refused; viewers do not automatically prune ended panes. Older
daemons refuse this mode before attachment without silently falling back to a
sizing client. Ordinary CLI and GUI attachments retain their existing minimum
size policy. With only viewers remaining, the last pane geometry is preserved.
Controller takeover/release, local pan/fit and GUI viewer controls remain future
work; this increment does not change recovery storage or migrate sessions.

The desktop dependency lock updates crypto-bigint to 0.7.10 and ctutils to 0.4.7.
The known RSA advisory RUSTSEC-2023-0071 remains unresolved and unsuppressed.
Public audit ledgers now distinguish completed v0.4.52 fixes from remaining
sizing, physical-device, external-host and endurance work.

Installed-file updates preserve existing daemons and running programs. Reopen
the GUI or detach/reattach a CLI frontend to load new client code. Viewer mode
requires an updated daemon; an older daemon and its sessions stay intact until
an explicit, appropriate refresh. No complete security or physical qualification
is implied.
