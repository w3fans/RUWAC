RUWAC v0.4.74 — upgrades that reach new work, roomy tabs and many windows

Package updates now reach new work without stopping anything. After an update,
new terminals, `ruwac -S NAME`, `ruwac start` and the GUI use an engine of the
installed version. An older engine that is completely idle is retired safely
and replaced on the default socket, keeping its ended history readable; an
older engine that still runs programs is preserved untouched and the installed
engine starts beside it. Existing named sessions are still found wherever they
live, and `ruwac -ls` lists every engine's sessions. The new atomic
`daemon.stopIfIdle` (and `ruwac engine stop`) never stops an engine while a
terminal runs or another client is attached. `ruwac engine show` and the GUI's
Updates dialog show the installed package, every engine and where new sessions
go.

Fedora machines can be set up once and then update RUWAC on their own:
`/usr/libexec/ruwac/fleet-setup --ring canary|standard|disabled` enables the
RUWAC COPR with five-minute metadata, installs the matching packages and
enables a package-only update timer. It never touches running engines or
terminals; `ruwac update status` shows the result.

Tabs share the bar equally — at most about a fifth of it each — shrink evenly as
more open, and scroll with arrows when crowded, with a lighter bottom-border
style. New windows open in the same RUWAC, so tabs can be dragged between
windows, and a saved workspace can be opened in a new window to continue its
terminals. Saved SSH machines can run a startup command such as `su` after
login. Ctrl+V of a clipboard image now reaches Claude Code behind a nested
RUWAC session. File transfers show progress again with rsync 3.5.2, which
changed its progress format. Remote Codex and Claude tabs on COPR machines
start without grep warnings and use the installed package, not a leftover
~/.local/bin copy. Function keys F1–F12 and Insert now reach terminal programs
such as nvtop, htop and mc; with Map keys on (Right Alt tap), F10 does too.

Update a Fedora machine with:

    sudo dnf upgrade --refresh ruwac ruwac-desktop

Reopen the GUI to load the new client; running engines and terminals are kept.
The desktop's SQLite binding moves to rusqlite 0.41.1 / libsqlite3-sys 0.39.1,
clearing RUSTSEC-2026-0459 (RUWAC links the system SQLite and never built the
affected bundled copy). The desktop RSA advisory RUSTSEC-2023-0071 remains: stable `rsa 0.10.0` is now
patched upstream, but IronRDP's Picky/SSPI/winscard releases still pin the
release candidate. No complete security or physical qualification is implied.
