RUWAC v0.4.81 — keep narrow Unicode panes alive

Prevent wide Unicode output in a one-column split from stopping the terminal
engine. The PTY and emulator now share the terminal library's two-column
minimum, while a smaller visible rectangle clips the retained grid. Widening
the pane keeps the same child process and preserves its Unicode text.

GTK painting and pointer hits stay within each pane's visible rectangle;
the hidden column cannot cover or capture clicks in the split divider.
The existing session-size voting and non-sizing viewer roles are unchanged.

Candidate verification covers 165 headless and 277 desktop unit tests,
40 real-PTY narrow/wide resize cycles, the 100-cycle viewer regression,
checkpoint preservation, maximum grids and an isolated GTK comparison.
The published previous GUI reproduces divider painting and hit errors;
the candidate fixes both. This does not claim physical hardware acceptance.

The desktop RSA advisory remains visible; Windows acceptance remains pending
on R7. Package updates preserve active engines and programs. Open Updates →
Open updated engine, or run `ruwac engine gui`, to test the new engine without
closing existing terminal processes.
