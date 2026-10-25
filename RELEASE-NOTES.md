# Native mouse-selection delivery for GNOME desktops

The optional GNOME Shell 50 companion lets RUWAC publish a selected text into
native Wayland applications on an explicitly chosen remote desktop. Installation
is available from Preferences or `ruwac-desktop -G install`. Enable desktop
publication permission, then authorize each selection connection on that desktop.
Installation never approves a connection or restarts your desktop; GNOME may
require a new login before loading installed code.

Selection-only retry preserves the RDP connection and requires fresh approval.
Newer selections survive delayed replies, helper restarts and imported-selection
echoes. A separate ownership confirmation cancels stale paste input. Confirmation
is a point-in-time check; SSH and later RDP input are not atomic. Ctrl+C/V and image
attachments retain their independent clipboard routes.

Helper startup, identity/metadata checks and blocked X11 reads are bounded.
Long Unicode machine names fit the remote approval label without splitting a
character. The companion archive and managed installation include instructions,
the project license and a notice. A public packaging test verifies exact archive
contents and schema compilation without installing the extension.

Native publication supports GNOME Shell 50. Receiving remote selections still
requires that desktop's configured Xwayland bridge. Other desktops retain their
available compatibility routes. Isolated GNOME, virtual input, authenticated
loopback transport and generated RDP tests do not replace physical-input or
external-host qualification. Broader development work remains open. The existing
RSA advisory remains unsuppressed.
