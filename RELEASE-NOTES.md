RUWAC v0.4.75 — copy nested selections and pass application Alt keys

Ctrl+Shift+C can now copy a nested terminal application's mouse selection to
CLIPBOARD. Desktop Ctrl+C also copies that selection, and becomes an interrupt
again after typing clears it. Previously a nested RUWAC could report that text
was copied to its mouse-selection buffer while the outer window had no text
for the keyboard Copy action. The new fallback requires the focused pane's
approved selection and current PRIMARY ownership. A new selection, typing,
focus loss or a different owner invalidates it; stale responses cannot copy
text from another pane. Copy requested while that approved selection is still
loading finishes when it arrives, rather than interrupting the application.

Plain Alt+M opens Machines and Alt+digits select workspace tabs. Other plain
Alt letters belong to the application: Alt+A no longer opens RUWAC's Agents
menu. Agents remains available from its button. Native tests verify exact
Alt+A, Alt+X and Alt+S input bytes, keyboard selection copy, interrupt restoration
and stale-owner refusal, alongside the existing word and Shift-selection tests.

Update Fedora packages with:

    sudo dnf upgrade --refresh ruwac ruwac-desktop

Reopen the GUI to load the new client. Running terminal programs are preserved;
for a nested terminal inside an RDP desktop, its remote RUWAC GUI also needs the
updated client. No live engine, remote desktop or login service is restarted.

Repeated GNOME RDP handover/disconnection failures remain under investigation.
This patch does not claim to fix them and does not raise handover limits or
change automatic-login settings. Native tests use isolated generated input;
physical keyboard, touchpad and cross-machine acceptance remain separate.
The existing RSA dependency advisory remains unsuppressed.
