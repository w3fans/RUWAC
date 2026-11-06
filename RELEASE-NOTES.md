RUWAC v0.4.65

Starting a selection on a live shell and holding the drag at the top or bottom
edge now scrolls the terminal history. The terminal frontend arms its existing
selection-scroll state as the drag enters copy mode.
Scrolling follows vertical pointer movement, keeping horizontal first-row
selections on their original text.

Application mouse handling, Shift selection, refresh timing and existing terminal
processes are preserved. Isolated checks cover held drags, ordinary copying,
nested clients, Unicode and mouse routing while retaining the child processes.
Physical-owner RDP acceptance remains separate.

COPR provides both ruwac and ruwac-desktop for Fedora 43, 44, 45 and Rawhide
on x86_64. For immediate updates, use:

    sudo dnf upgrade --refresh ruwac ruwac-desktop

An already attached terminal frontend keeps its old code after installation.
Detach that frontend (default: Ctrl+A, then d), then reattach the same session
with ruwac -r SESSION. Its engine and running programs can stay alive.
Reopen the GUI to load its installed version.

This is a development patch; the existing RSA advisory remains open and
unsuppressed.
