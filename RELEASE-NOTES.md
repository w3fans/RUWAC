RUWAC v0.4.82 — image paste to npm Claude Code and opt-in controller sizing

Ctrl+V of a clipboard image now reaches Claude Code installed through npm,
whose native program runs as `.../@anthropic-ai/claude-code/bin/claude.exe`.
A nested RUWAC session previously refused the handoff with "PNG handoff
requires the selected local agent" while Codex in the same session worked.
An unrelated program named `claude.exe` is still not treated as an agent.

Sessions can opt in to controller-owned sizing: create one with
`ruwac session create --size-policy controller_owned` (or `size_policy` in
`session.create`). Only the client that explicitly claims control resizes it;
every attachment, including older clients, starts as a viewer whose window
never resizes the shared terminal. The TUI offers `size claim`, `size
takeover` and `size release` in command mode and shows Controller or Viewer in
its status bar. The GTK terminal shows "Take sizing control…" with Cancel as
the default, or "Release sizing control". Detaching or losing the connection
releases control and keeps the last size. Existing and ordinary sessions keep
their current sizing.

Verification covers headless and desktop unit tests, real-PTY sizing and
compatibility suites with actual 0.4.49 and 0.4.81 terminals, and an isolated
GTK comparison where the published 0.4.81 desktop fails the new checks. GTK
pan/fit, SSH host acceptance of controlled sessions and physical
two-workstation acceptance remain open. The desktop RSA advisory remains
visible.

Update a Fedora machine with:

    sudo dnf upgrade --refresh ruwac ruwac-desktop

Reopen the GUI to load the new client; running engines and terminals are kept.
