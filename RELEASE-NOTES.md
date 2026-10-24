# Keep the first new remote selection when switching tabs

After returning to an RDP tab, the first new mouse selection could be classified
as an initial snapshot. If a terminal or another RDP tab already owned a selection,
RUWAC could retain that older text instead. The relay now records the epoch in
which the selection actually changed, using server ownership notifications to
distinguish it from GDK's initial snapshot: a new selection in the active desktop
can replace older text, while background snapshots and late replies remain guarded.
PRIMARY and the Ctrl+C/V clipboard stay separate.

The desktop lockfile also updates crypto-bigint to 0.7.6 for
[RUSTSEC-2026-0387](https://rustsec.org/advisories/RUSTSEC-2026-0387.html),
with compatible ctutils 0.4.5. The separate RSA advisory remains unresolved.

Regression coverage includes the first deliberate selection after a mode change,
inactive snapshots, late completion, independent X11 INCR transfer, Unicode,
multiline text, size limits and preservation of both Ctrl+C/V clipboards.
The acceptance ledger now separates the completed v0.4.40 baseline from its
historical prepublication notes and later development candidates.

Reopen the GUI after updating to load the new relay. Daemon-owned terminal
processes remain available; RDP reconnects after reopening. Physical touchpad,
clipboard-manager behavior, generic cross-host Wayland PRIMARY injection and
external GNOME/Windows qualification remain separate limits. The known RSA
dependency advisory remains unresolved.
