RUWAC v0.4.68

Double-click words in GUI terminals and ordinary shell panes in the terminal
client. Applications that request mouse input keep receiving their mouse events;
hold Shift in the GUI to select terminal text instead.

Selections preserve combining accents, wide characters and joined emoji, including
words split by a real soft wrap. Oversized word copies leave the terminal attached
and preserve its previous copy buffer. Existing held-drag scrollback, keyboard
controls, PNG handoffs and persistent terminal sessions remain in place.

Reopen the GUI and detach/reattach the terminal client to load the new frontend.
The engine, agents and terminal programs can keep running. This development patch
has generated PTY/native-compositor acceptance; physical input, clipboard-manager
and external Windows acceptance remain separate. The known RSA advisory remains
unsuppressed.
