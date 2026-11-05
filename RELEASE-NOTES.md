RUWAC v0.4.60 improves remote localhost previews and terminal hyperlink copying.

Open a remote web service from its existing verified SSH terminal, choosing the
service's remote loopback port and path. The preview reuses that terminal's SSH
identity and forwarding route. Load and certificate errors remain visible.
For an untrusted HTTPS certificate, review its fingerprint explicitly for that
preview; approval is isolated and refused if the SSH identity or view changes.
This workflow provides access to services bound only to the remote machine's
localhost without manually setting up a separate tunnel or using an RDP desktop.

Mouse copying now joins matching styled URL fragments that a terminal application
has wrapped onto the next display row. It removes their display-only padding and
indentation, including in pinned mouse selections. Ordinary text, code, shell
continuations and genuine hard newlines keep their existing behavior. Unstyled
history is preserved conservatively. Reopen the GUI to load the clipboard fix;
your daemon and existing terminal programs can keep running.

The Machines dropdown adds RDP, Term, Codex and Claude capability filters, including
linked endpoints. All machines, Favorites, Recent and Alt+M remain available.
COPR documentation explains that the single ruwac source-package row supplies
both the ruwac CLI and ruwac-desktop binary RPMs, with normal DNF updates.

Completed development patches continue through the existing release controller
and mandatory release gates. Broader stable milestones retain their additional
acceptance criteria; incomplete unrelated features do not freeze patch delivery.
This increment retains the 19 required release categories, with additional native
checks for wrapped links, capability filters and remote HTTPS preview handling.
Published-artifact qualification and authorized installation remain separate
steps after source publication. Physical input and broader external-host coverage
remain open. The known RSA timing advisory remains unresolved and unsuppressed.

Packaging recognizes an additional validated x86-64 byte-indexed jump-table
encoding. This avoids numeric-data privacy false positives while retaining
checks for actual private strings, unknown/malformed tables and key material.
