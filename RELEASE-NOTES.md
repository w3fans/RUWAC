RUWAC v0.4.67

PNG paste into a local nested RUWAC terminal now hands the staged image to
that specific terminal, which validates its selected agent and attaches through
its existing engine connection. Displayless persisted Codex/Claude processes no
longer need direct X11 clipboard access for this route. No Enter is sent.

The handoff is private, short-lived and bounded. Typing, changing pane/view or
losing GUI focus revokes unfinished delivery. Ordinary native Ctrl+V, SSH image
transfer and older nested frontends retain their existing routes.

Update and reopen the GUI, then detach the nested CLI with Ctrl+A followed by d
and reattach with ruwac -r SESSION. The engine, agent and terminal programs can
keep running. Updating only the package files does not reload an attached CLI.

Generated displayless-agent tests use an actual published v0.4.59 engine and
preserve its agent PID/start identity. Native GDK/PTY comparison retains the
published v0.4.66 GUI as a negative control. Frozen release and actual-public
acceptance remain separate from candidate results; owner physical/provider
image consumption and external Windows acceptance are not claimed.

The known RSA Marvin advisory and maintenance warnings remain unsuppressed.
