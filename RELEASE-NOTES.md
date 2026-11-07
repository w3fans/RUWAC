RUWAC v0.4.69

Mobile terminal observers can request ANSI captures that retain foreground and
background colors and text emphasis, without attaching to or resizing the PTY.
Captures contain SGR styling only and keep complete newest rows within 64 KiB.
Default captures and waits stay plain; matching stays independent of styling.
Saved checkpoint history retains its existing plain-text fallback.

Unchanged cell styles are reused during ANSI serialization to reduce allocation
work. Unicode, bounded output, primary/alternate buffers, matching, typed parameter
validation and non-sizing real-PTY cases have targeted qualification. Published
artifacts receive separate integrity and runtime checks through the normal release
workflow.

This capability requires an upgraded engine. Existing running engines keep their
terminals and plain capture fallback; package installation does not replace them.
No Android changes or owner engine restart are included. Physical mobile-device,
external Windows and endurance acceptance remain separate. The known RSA advisory
remains unsuppressed.
