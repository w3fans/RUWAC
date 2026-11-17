RUWAC v0.4.83 — copy from Claude Code reaches the clipboard; one name, one session

Copying in a program that uses OSC 52 for the clipboard now reaches the
desktop clipboard: Claude Code's "copied to clipboard", and Neovim, tmux or
remote shells set up for OSC 52. RUWAC dropped every such write, so Ctrl+V
pasted older text. It also works through a nested RUWAC session. A write is
applied only right after your own click or key press in that pane, and
programs still cannot read the clipboard. A session that runs on an engine
older than 0.4.83 keeps dropping these writes until it runs on a new engine.

`ruwac -S NAME` and `ruwac -r NAME` typed inside a RUWAC pane now find the
session in every running engine instead of only the pane's own engine. From a
pane of an older engine, `-S d` used to create a second "d" beside the one
that already ran elsewhere. A name that exists in more than one engine is now
refused, with the `ruwac -r SESSION_ID` commands to choose one. `ruwac -ls`
names the engine of every session whenever several engines run and flags
duplicate names, and the session status bar shows its engine version.

Middle-click (touchpad three-finger tap) in a RUWAC session always pastes,
also while the program in the pane tracks the mouse. In a Codex or Claude
Code pane a plain left drag selects text in RUWAC, so middle-click pastes it;
clicks, the wheel and pointer motion still reach the agent. Ctrl+Shift+R now
reaches the terminal program or remote desktop instead of opening Machines,
which stays on Alt+M. After a package update, a running window shows "Restart
to update" and a RUWAC session shows "UPDATE INSTALLED" until reattached.

Verification covers headless and desktop unit tests, real-PTY nested mouse,
engine routing and lifecycle suites, and isolated GTK sessions in which the
published 0.4.82 fails the new clipboard and session-name checks. Physical
input and remote-desktop clipboard synchronisation remain open. The desktop
RSA advisory remains visible.

Update a Fedora machine with:

    sudo dnf upgrade --refresh ruwac ruwac-desktop

Reopen the GUI to load the new client; running engines and terminals are kept.
