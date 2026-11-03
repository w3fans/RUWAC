RUWAC v0.4.55 installs RUWAC on remote Fedora machines from COPR and makes
copying a machine quicker.

"Install / update remote RUWAC CLI…" and agent launches with automatic RUWAC
installation now use the Fedora COPR packages (`zvegelj/ruwac`) through DNF on
Fedora machines. A machine without the package enables COPR and installs it,
replacing a GitHub testing RPM of the same version and retiring a per-user copy;
`ruwac-desktop` moves along when it is installed. A machine with the package
upgrades only when DNF reports a newer build, so routine agent launches ask for
no password. Other systems, and Fedora releases that COPR does not build for,
keep the verified per-user installation.

"Copy machine…" moved from the machine form to the properties dialog's action
bar, beside Save. It saves the copy with new machine and endpoint identities and
opens it in the editor.

Install or update a Fedora 44 machine directly with:

    sudo dnf copr enable zvegelj/ruwac
    sudo dnf install ruwac ruwac-desktop

Installed-file updates preserve existing daemons and running programs. Reopen
the GUI or detach/reattach a CLI frontend to load new client code. The known RSA
advisory RUSTSEC-2023-0071 remains unresolved and unsuppressed. No complete
security or physical qualification is implied.
