# Local nested PNG handoff — 2026-11-06

A local GUI could stage a clipboard PNG but forwarded Ctrl+V when the foreground
was another RUWAC terminal. A persisted agent without DISPLAY/WAYLAND_DISPLAY
then attempted its own native clipboard read and failed. The SSH image route
already transferred an explicit PNG, explaining the different outcomes.

The GUI now offers its staged file to the exact foreground TUI PID/start through
a short-lived same-user private Unix socket. The actual attached TUI revalidates
its own session/window/pane, foreground PID/start, agent readiness, bracketed
paste and PNG before the existing explicit image RPC. It does not infer an inner
target from argv or footer text. Existing engines and processes remain alive.
No new daemon method, graphical headless dependency, SSH privacy change or Enter.

Workers are created only for an explicit image paste, capped at four per
frontend, with bounded messages and deadlines. Subsequent input/navigation or
GUI focus loss revokes pending work. Socket/lock ownership, mode, symlink,
hardlink, peer PID/UID, stale-instance and replacement-inode checks remain in the
shared transport. No background listener or new idle refresh timer is added.
No offer preserves ordinary native Ctrl+V; an older nested TUI retains the GUI's
previous native-key forwarding behavior. Claude's pre-54 guarded paste fallback
is retained when its engine lacks that adapter.

## Candidate evidence

The development comparison uses an exact production-source snapshot on the
approved bounded build host (4 GiB, two CPUs, zero observed swap, approximately
1.3 GiB peak) and generated isolated PTYs/userspace. It passed 28 PTY transport
units, 42 TUI units and 266 desktop units. Five real-PTY cases against the actual
published v0.4.59 engine cover one PNG/no Enter/stable PID, absent-offer native
Ctrl+V, typing cancellation/responsiveness, GUI EOF cancellation and a forged
agent footer refusal.

An isolated native Mutter/GDK test uses an independent PNG clipboard owner and
a displayless synthetic agent. With the published v0.4.66 GUI, the agent gets
only literal Ctrl+V and no image. With the candidate GUI/TUI, it gets exactly one
bracketed PNG path with matching bytes and private permissions; the old engine
and agent retain their PID/start identities. These are synthetic GTK key
callbacks and real clipboard/PTY checks, not owner physical input or provider
image-consumption acceptance. Native fixture setup failures are retained; the
successful comparison verifies actual peer focus and input serial before PNG
publication, rather than accepting missing clipboard formats as a product result.

Regression entry points: `ruwac/tests/integration/local_image_handoff.py` and
`local_image_native.py` inside the owned native compositor. Each accepts explicit
binaries/evidence; `--engine` permits an old published engine without touching
owner sockets. `--baseline` in the native fixture verifies the earlier native
key-only behavior. The release acceptance adapter must bind these checks to its
current frozen CLI/GUI in addition to the existing clipboard acceptance.

## Rollback and acceptance

Revert the frontend handoff change to restore native-key forwarding. Daemon
protocol, IDs, PTYs, persisted sessions and staged-file retention are unchanged.
Update/reopen the GUI and detach/reattach the CLI to use the fix; restarting the
engine or agent is unnecessary. Real owner laptop/RDP input, Windows clients and
actual provider image consumption remain owner/physical acceptance, not claimed
by these fixtures. The known RSA advisory remains unsuppressed.
