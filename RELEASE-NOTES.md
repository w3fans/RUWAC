RUWAC v0.4.57 lets you paste images into Claude Code in sessions started by an older RUWAC daemon.

Daemons before v0.4.54 could attach a pasted PNG only to Codex. Updating the
packages keeps running daemons and their sessions, so Ctrl+V into Claude Code in
such a session failed until the daemon was restarted. The updated client now
recognizes that daemon, checks on its machine that Claude Code is the selected
foreground program and ready for input, and pastes the image path, which Claude
Code turns into `[Image #N]`. Nothing is submitted. Current daemons still attach
the image themselves.

Image paste also no longer waits forever when Claude Code shows a shortened
footer such as `⏵⏵ auto mode on · 1 shell`.

Ordinary commits merged to `main` no longer fail the Source release workflow.
Only a commit carrying its controller-assigned version tag is published.

Update a Fedora machine with:

    sudo dnf upgrade --refresh ruwac ruwac-desktop

Then reopen the GUI, or detach and reattach a CLI frontend, to load the new
client. Existing daemons, sessions and running programs are not restarted.
The known RSA advisory RUSTSEC-2023-0071 remains unresolved and unsuppressed.
No complete security or physical qualification is implied.
