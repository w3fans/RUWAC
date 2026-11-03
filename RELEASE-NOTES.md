RUWAC v0.4.54 lets you paste images into Claude Code and adds from-source
Fedora/COPR packaging.

With Claude Code running in a local RUWAC terminal, Ctrl+V with an image in the
clipboard attaches it to the Claude Code draft, as it already did for Codex. It
appears as `[Image #N]`; nothing is submitted. RUWAC recognizes Claude Code's
native binary and its npm package, and pastes only once its input box is ready.
Remote targets need this version of RUWAC on that machine too. "Attach PNG to
remote agent…" now lists Claude Code panes as well.

`ruwac/packaging/fedora` builds `ruwac` and `ruwac-desktop` from source with
vendored crates, for COPR now and Fedora review later. Packages built this way
are updated only by DNF: `ruwac update` and the desktop update button say so and
never contact GitHub. With such a package installed, the per-user installer and
updater no longer install or update a second copy, and the packaged `ruwac
update` retires a leftover per-user copy's command links, launcher and update
timer. Its files stay in `~/.local/lib/ruwac`, so running daemons and sessions
continue. Replacing a GitHub testing RPM with the DNF package also removes that
RPM's stale update-timer link. The GitHub testing RPMs and per-user bundles are
otherwise unchanged.

Installed-file updates preserve existing daemons and running programs. Reopen
the GUI or detach/reattach a CLI frontend to load new client code. The known RSA
advisory RUSTSEC-2023-0071 remains unresolved and unsuppressed. No complete
security or physical qualification is implied.
