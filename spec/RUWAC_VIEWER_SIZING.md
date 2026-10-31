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
