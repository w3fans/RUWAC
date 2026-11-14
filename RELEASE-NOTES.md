RUWAC v0.4.76 — keep RDP connected when clipboard replies stall

Previously, a missing RDP clipboard reply closed the entire connection after
10 seconds. RUWAC now disables clipboard transfers for that connection while
leaving display and keyboard input available. The connection status explains
what happened. Use Reconnect to enable clipboard transfers again.

Late clipboard data, acknowledgements and new offers are ignored while the
channel is disabled. Pending copy/paste work is cancelled, so a late reply
cannot become a different request's content or trigger a delayed remote paste.

An authenticated loopback regression reproduces the disconnect in published
0.4.75. The candidate stays connected for a full minute, handles native keyboard
input and a framebuffer change, preserves local clipboard content, and restores
transfers through a normal saved-profile reconnect. All 277 desktop unit tests
and ten selected native RDP regression groups passed before release qualification.
These generated tests do not certify physical GNOME sign-in or Windows clients;
other GNOME handover and network failures remain separate investigations.

Update Fedora packages with:

    sudo dnf upgrade --refresh ruwac ruwac-desktop

Reopen the GUI to load the client fix. Existing terminal engines and programs
can stay running. No owner RDP connection or login service is restarted by the
package update. The existing RSA dependency advisory remains unsuppressed.
