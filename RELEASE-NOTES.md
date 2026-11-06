RUWAC v0.4.63

Drag a mouse selection toward the first or last visible GUI terminal row and
hold there to scroll through history. Dragging beyond the pane still works. The mouse wheel extends
the same anchored selection, and scrolling stops when the pointer returns to
the middle or the mouse button is released.

The old edge trigger required the pointer to leave the terminal. Native isolated
Mutter tests reproduce that failure in the published v0.4.62 GUI and verify the
fix for Shift+drag in mouse-aware applications and ordinary shell selection.
They also check stationary edge holds, both directions, wheel scrolling, exact
PRIMARY copy, typing cancellation and released-button behavior. Physical and
cross-machine input acceptance remains separate.

For a nested ruwac -r session, Ctrl+A then [ opens its own history; select there
without Shift. Shift+drag selects the outer terminal's history.

Update ruwac and ruwac-desktop through DNF, then quit and reopen the GUI to load
this GUI change. The persistent terminal engine and its programs can remain
running. The existing RSA advisory remains open and unsuppressed.
