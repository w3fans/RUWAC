RUWAC v0.4.52 preserves RDP scrolling under congestion and makes fast terminal
history selection wait for its matching snapshot. Wheel impulses keep their
order with clicks and keys; full queues report an error instead of silently
losing gestures. Pure pointer positions may still be discarded under pressure.

Mouse-up before history is ready keeps a bounded copy intent. Typing, focus or
target changes cancel it; an expired or unavailable request reports that nothing
was copied. A changed starting viewport is rejected, and incomplete history
prefixes report the 8 MiB limit rather than selecting unrelated text. Inside an
attached RUWAC session, use Ctrl+A then [ for its own scrollback/copy view.

A retained operation warning no longer makes a working terminal look failed.
Its health indicator stays green with a small amber warning marker; starting
and disconnected terminals retain their separate states. The warning text
remains available in the tab tooltip and controls.

Release publication now checks that these notes match the controller-allocated
tag and its unique changelog section. The acceptance ledger records completed
0.4.51 qualification and distinguishes generated native tests from physical and
external-host acceptance. Shared multi-client PTY sizing, checkpoint disk writes
and further remote typing latency work remain open for later increments.

The known RSA advisory RUSTSEC-2023-0071 remains disclosed and unsuppressed.
This release does not establish complete security, physical-device, external
Windows or endurance qualification. Installed-file updates preserve existing
daemons and their running programs. Reopen GUIs to load the new frontend.
