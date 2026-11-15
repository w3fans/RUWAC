# Controller-owned terminal sizing (opt-in)

Capability `controller_owned_sizing: 1` adds an explicit session policy, separate
from focus and input. Existing and ordinary new sessions use `legacy_minimum`.
Create an opt-in session using `session.create` with
`size_policy: "controller_owned"`, or:

```
ruwac session create --name shared --size-policy controller_owned --detach --json
```

`-Y` is the short form of `--size-policy`. No existing session is migrated.
All attachments, including an older client's plain attach, enter as viewers.
Their allocation and local zoom do not vote in either session or pane sizing.
A viewer can navigate and type normally; it cannot mutate shared layout.

## Lease and canonical layout

`session.size` reports policy version 1, canonical rows/columns, whether a
controller is present and an opaque generation bound to this daemon and session.
An attached client explicitly calls `session.sizeClaim` with the displayed
`expected_generation` and boolean `takeover`. Claiming while another client
controls sizing requires `takeover: true`; duplicate or stale claims fail.
Claim itself does not resize. `client.resize` and shared layout mutations require
that connection's current `size_generation`. Checks precede idempotency replay.
A token from another session, connection or restarted daemon cannot grant control.

Pane dimensions derive from the canonical grid and persistent split tree.
Local zoom, pane/window focus and viewer joins do not modify canonical defaults
or PTY dimensions. `session.sizeRelease`, detach, socket loss or reattach revokes
the lease. Nothing automatically inherits it; the last grid freezes. A former
controller may still type as a viewer. Lost ownership never causes input replay.

TUI command mode (`Ctrl+A`, `:`) provides `size claim`, `size takeover` and
`size release`. Claim/takeover explicitly fits the canonical grid to this
terminal's current allocation; the status bar shows its role and grid.
GTK controlled sessions show `Take sizing control…` with default-Cancel
confirmation, or `Release sizing control`. A delayed confirmation uses the
original displayed generation and fails if ownership changed. The existing GUI
command prompt provides the same explicit commands. No implicit claim on open,
reconnect, map, local zoom or resize.

## Persistence and compatibility

Legacy checkpoints retain their previous field shape and schema selection.
Controlled checkpoints use schema 7, preserving policy and canonical grid, never
client identities, lease tokens or live generations. The new reader accepts old
schemas. An old reader rejects schema 7 and preserves the original file instead
of silently downgrading the policy; this is not old-reader support for controlled
sessions. Restored panes remain ended history and receive no input or respawn.

Generation counters are daemon-lifetime ownership generations, not distributed
leases or provider execution identities. Ordinary single-connection resize is
ordered by the daemon; no exactly-once or cross-daemon sizing claim is made.

## Ended panes and older viewers

A controlled viewer cannot prune shared panes. Closing an already-ended pane
returns `closed:false`, `retained:true`, `reason:sizing_control_required` without
changing layout, PTYs, revision or shared idempotency cache. This also covers
older TUIs that omit `if_ended` during automatic exit cleanup. It requires an
attachment to that exact session; foreign sessions and live panes remain denied.
A sizing controller still needs its current generation to close the pane.
Viewer and older TUI cleanup keys do not poison a controller's later mutation.

Unpinned current viewers move to another live window when their displayed
window finishes; pinned or last-live-window views exit cleanly and retain shared
history. Merely ending one split does not collapse the canonical split tree.

Local TUI pan is implemented; see [viewer sizing](RUWAC_VIEWER_SIZING.md).
A GTK viewer crops the canonical grid to its window; GTK pan/fit controls are
not part of this branch.

## Remaining scope

This branch is unpublished. Existing-session policy switching, GTK pan/fit,
detailed geometry-generation pointer qualification, nested
Screen/tmux matrix and physical two-workstation acceptance remain open. The
current view crops to its allocation; it does not reflow an alternate screen or
advertise universal multi-desktop acceptance from isolated fixtures.

Authenticated SSH (`--host`) acceptance of controlled sessions has not been
run on this branch; the prototype's guarded SSH viewer tests were not ported.

## Verification on this branch

Headless units (167), `client_size_controller` (10), `client_sizing_viewer`
(16), `controller_legacy_exit` with actual 0.4.49 and 0.4.81 TUIs (9),
`client_focus`, `recovery`, `narrow_unicode`, `checkpoint_consistency`;
desktop units (277) and `desktop_size_controller` (8) on an isolated Mutter
display. These are generated fixtures, not physical workstation acceptance.
