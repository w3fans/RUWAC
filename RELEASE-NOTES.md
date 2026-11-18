RUWAC v0.4.84 — -wipe cleans every engine; Windows entries save cleanly; GNOME RDP reconnects

`ruwac -wipe` now removes dead sessions in every running engine. After an
update the dead sessions usually live in a preserved older engine, so `-wipe`
reported "Removed 0" while `ruwac -ls` still listed them. `ruwac -wipe NAME`
cleans that session in whichever engine holds it.

The machine editor no longer stores "wayland-0", a selection-sharing account
or the "ruwac" agent backend in entries that do not use them, such as
Windows hosts. Cleared values stay cleared after Save, and a save the editor
refuses now scrolls its reason into view instead of looking like nothing
happened.

Opening a GNOME remote-login or desktop-sharing connection while another
RUWAC tab is still connected or connecting to the same machine now
disconnects that tab first and then connects. Two connections at once left
both stuck. Windows and other plain RDP connections are never disconnected.

Verification covers headless and desktop unit tests, the cross-engine session
suite with the new -wipe checks, and an isolated GTK machine-editor test in
which the published 0.4.83 fails. The GNOME handover itself is covered by unit
tests only. The desktop RSA advisory remains visible.

Update a Fedora machine with:

    sudo dnf upgrade --refresh ruwac ruwac-desktop

Reopen the GUI to load the new client; running engines and terminals are kept.
