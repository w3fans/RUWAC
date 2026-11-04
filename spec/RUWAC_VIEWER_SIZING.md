# Explicit non-sizing terminal viewers

`non_sizing_viewer: 1` advertises `size_role: "viewer"` on `session.attach`.
Use `ruwac attach --session ID --viewer` (`-O`), optionally with `--window ID`;
the existing authenticated `--host` path supports the same arguments. Both paths
check support before attaching. An older daemon is refused, with no automatic
ordinary sizing fallback. Existing GUI and ordinary TUI attachments keep their
previous minimum-size policy; no existing session is migrated.

A viewer's allocation and local window/pane/zoom choices do not vote in session
or pane sizing or overwrite shared defaults. Its navigation remains within its
attached session. When only viewers remain, the last per-pane geometry freezes.
Role changes replace attachments atomically; removing an old sizing vote still
updates the aggregate when other sizing attachments remain.

Viewers retain ordinary input, copy/search, window/pane navigation and local zoom.
Shared window/pane creation, closure and split-ratio changes are refused before
idempotency replay or caching. Denied calls cannot poison an owner's layout key.
The common render decoder does not automatically close a viewer's ended splits.
A completed viewer exits without pruning shared history.

`client.resize` returns `resized:false, viewport_resized:true`. Rendered frames
include the role and local allocation while preserving canonical grids; these
fields participate in quiet-projection invalidation. The TUI clips rows, dividers,
cursor and wide-glyph boundaries to its physical dimensions and labels cropping.
It does not reflow terminal application screens. History copy has its own viewport.

This increment is connection-local and does not change the checkpoint schema.
Viewer roles are not persisted. Controller-owned policies, takeover/release,
local pan/fit, default GUI viewer controls and physical mixed-workstation input
remain separate work. A viewer is not read-only or a security boundary; ordinary
same-user daemon requests and sizing attachments retain their existing powers.

## Regression gates (2026-11-04)

The public `Fedora source checks` quality job runs two viewer gates on every pull
request and every push to `main`, after the headless binary is built:

- `client_sizing_viewer.py`: 16 real-PTY cases against the freshly built daemon
  and TUI, including the unchanged PID, `TIOCGWINSZ` size and SIGWINCH count, 100
  resize/zoom/reattach cycles, the viewer-only last-sizer freeze, refused
  structural edits, session-namespace isolation and the negotiated `-O`
  attach/detach.
- `viewer_legacy.py`: the current CLI's `--viewer` refusal against an older
  daemon, with unchanged PTY size, signals and terminal modes. The older daemon
  is built in CI from the full commit SHA of v0.4.52 (`fca85c5ec0bc`), the last
  published source without `non_sizing_viewer`. It is built from content-addressed
  source, not a downloaded published binary.

`ssh_sizing_viewer.py` and its `ssh_sizing_server.py` fixture are not part of
public CI. That fixture requires a `ruwac-bounded-*` systemd user unit with
memory and PID limits, plus a `sudo -n` cgroup binder. A hosted runner provides
neither, so the authenticated SSH viewer path remains a separately qualified gate.
