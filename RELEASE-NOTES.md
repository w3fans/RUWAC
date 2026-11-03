RUWAC v0.4.56 makes remote RUWAC updates from Fedora COPR dependable.

On remote Fedora machines, "Install / update remote RUWAC CLI…" and agent
launches with automatic RUWAC installation now refresh DNF metadata before
checking for and installing a new build. Previously a stale DNF cache could
report "Nothing to do" for up to two days after a release. DNF now uses only
RUWAC's COPR repository and Fedora's own repositories, so a third-party
repository's signing key is never accepted on your behalf.

Update a Fedora machine directly with:

    sudo dnf upgrade --refresh ruwac ruwac-desktop

Installed-file updates preserve existing daemons and running programs. Reopen
the GUI or detach/reattach a CLI frontend to load new client code. The known RSA
advisory RUSTSEC-2023-0071 remains unresolved and unsuppressed. No complete
security or physical qualification is implied.
