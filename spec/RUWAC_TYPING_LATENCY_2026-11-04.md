# RUWAC typing latency — measured baseline (2026-11-04)

**Review correction (2026-11-04):** the original GUI-path detector searched serialized
JSON, so `{` and `}` could match structure before an echo appeared. Treat the
historical GUI and SSH-GUI figures below as unverified. The corrected benchmark
reads only the measured pane’s visible glyphs and closes silent RPC/probe sockets
at a deadline; eight isolated regressions cover these cases. Corrected local
ASCII/Unicode runs are recorded separately; no performance fix is implied.

**Scope:** measurement only. No RUWAC timing, polling interval or protocol was
changed. Produced with `ruwac/tests/benchmarks/typing_latency.py` (workstream D
of the [cloud work plan](RUWAC_CLOUD_WORK_PLAN_2026-11-04.md)).

**Setup:** one 16-core Fedora 44 machine (kernel 7.2.9), RUWAC 0.4.56 release
build, tmux 3.7c, GNU Screen 5.0.1, OpenSSH 10.2p1. The SSH hop is a generated
`sshd` on 127.0.0.1, so it adds OpenSSH's own processing but **no network
delay**. Every keystroke is timed by one observer process with one monotonic
clock, from writing the key to its character being drawn; topologies are
interleaved in blocks of 20 keys. The GUI rows replay the desktop worker's own
RPC sequence and schedule; **GTK paint, compositor and display latency are not
included**.

## Corrected local recheck (2026-11-04)

Forty measured keys per path and charset on a bounded Fedora44 test job, using
post57 candidate compiled runtime (no timing/protocol changes), one40×120 pane,
active pacing. Topologies interleaved; zero timeouts across all400 measured keys.
This is a short local recheck on a different host/load, not a before/after speed
claim or the original ten-topology matrix. GUI means protocol frame receipt.

| Path | ASCII p50/p95 ms | Unicode p50/p95 ms |
| --- | ---: | ---: |
| Direct PTY | 0.23 / 1.45 | 0.27 / 4.69 |
| GNU Screen | 0.34 / 3.76 | 0.37 / 2.17 |
| tmux | 0.84 / 6.17 | 0.55 / 3.41 |
| RUWAC TUI | 17.47 / 28.62 | 15.98 / 21.37 |
| RUWAC GUI protocol | 16.54 / 30.22 | 12.83 / 22.41 |

The full-frame probe returned about155KB per40×120 frame, with median full-frame
RPC around7ms versus0.15–0.17ms for an unchanged reply. This identifies frame
serialization/transfer and frontend scheduling as candidates for further profiling;
it does not isolate each cost or establish physical keystroke latency. Preserve
legacy framing and semantic snapshots when investigating optimizations.

## Historical original results (GUI figures unverified)


| Path | p50 | p95 | p99 |
| --- | ---: | ---: | ---: |
| Direct PTY | 0.08 | 0.12 | 0.15 |
| GNU Screen | 0.17 | 0.28 | 0.45 |
| tmux | 0.23 | 0.40 | 0.58 |
| RUWAC TUI (`ruwac attach`) | 12.2 | 16.4 | 16.9 |
| RUWAC GUI protocol, local daemon | 7.6 | 9.8 | 11.0 |
| SSH only | 11.0 | 19.1 | 20.0 |
| SSH → Screen | 10.1 | 19.1 | 21.1 |
| SSH → tmux | 11.7 | 20.1 | 21.1 |
| SSH → RUWAC TUI | 22.2 | 31.5 | 33.1 |
| **GUI on a remote RUWAC** (GUI → local pane running ssh → remote `ruwac attach`) | **31.3** | **49.6** | **55.2** |

Other scenarios (p50 / p95):

| Scenario | RUWAC TUI | GUI local | SSH → RUWAC | GUI on remote RUWAC | SSH → tmux |
| --- | --- | --- | --- | --- | --- |
| 5 panes, quiet | 10.7 / 14.9 | 6.9 / 8.5 | 20.7 / 32.6 | 31.4 / 48.8 | 11.5 / 20.2 |
| 20 panes, 50 Hz output in each | 14.4 / 22.5 | 7.8 / 9.3 | 23.5 / 35.6 | 35.0 / 63.4 | 12.2 / 20.3 |
| Unicode (Greek, CJK) | 13.5 / 15.2 | 7.5 / 9.8 | 22.3 / 32.5 | 39.4 / 50.3 | 9.6 / 19.8 |
| Typing after 1.2–2 s idle | 13.2 / 15.7 | 7.1 / 9.0 | 16.4 / 27.2 | 31.3 / 41.5 | 3.9 / 18.9 |

No keystroke timed out in any scenario.

## Where the time goes

1. **RUWAC's daemon is fast.** A keystroke sent with `client.input` reaches the
   daemon's screen in **0.4 ms** (p50), on par with Screen and tmux.
2. **A full frame costs the daemon 5–7 ms and 155 KB.** `client.render` without
   a matching revision serializes the whole 40×120 screen as about 155 KB of JSON,
   even when it is nearly empty; an unchanged poll costs 0.1 ms. While a full
   frame is being produced, the daemon does not process the echo: input followed
   immediately by `client.render`, which is what both the GUI worker and the
   TUI do, puts the echo on the daemon's screen after **5.6–7.2 ms** instead of
   0.4 ms. This is the dominant RUWAC-specific cost.
3. **The TUI adds its render interval on top.** After input the TUI renders at
   most every 8 ms, so `ruwac attach` lands at 10–14 ms, against 0.2 ms for
   Screen and tmux. The terminal stream is also larger: about 670 bytes per key,
   against 5 (Screen) and 10–40 (tmux).
4. **OpenSSH 10 adds about 10 ms by itself.** Its default
   `ObscureKeystrokeTiming` sends keystrokes on a 20 ms schedule to hide typing
   rhythm. It affects Screen, tmux and RUWAC equally (p95 of about 19–20 ms for every
   SSH path) and is visible in the "after idle" row, where the first keystroke
   is not delayed.
5. **The remote GUI path stacks all of the above.** For the GUI on a remote
   RUWAC (the everyday setup), the remote daemon has the echo after 10.6 ms
   (SSH), the local pane after 23.9 ms (remote TUI redraw + SSH back), and the
   GUI's frame after 31.3 ms (local full frame + poll). Under heavy output the
   p95 reaches 63 ms and each keystroke moves about 0.8 MB of render JSON.

## Proposals (not implemented; each needs its own measured PR)

- **Incremental frames.** Send changed rows (or cells) since the client's last
  revision instead of the whole screen. This addresses findings 2, 3 and 5
  directly. Keep the full frame for older clients and daemons.
- **Do not block PTY processing while serializing.** Snapshot the grid and
  serialize outside the daemon's critical section, so an echo is never queued
  behind a frame.
- **Event-driven TUI and GUI refresh.** Render when the revision channel
  (`events.subscribe`) reports a change rather than on a fixed 8 ms timer,
  with bounded coalescing and no busy loop.
- **Document `ObscureKeystrokeTiming`.** For trusted links, users can set
  `ObscureKeystrokeTiming no` per host in their SSH configuration; RUWAC
  should not change it silently, because it is a privacy protection.

Re-run with the same script before and after any of these changes; a claimed
improvement should show in this table.

## Reproduce

```sh
python3 ruwac/tests/benchmarks/typing_latency.py --binary target/release/ruwac \
  --evidence /tmp/bench --keys 200
# scenarios: --pacing quiet, --panes 5|20, --output busy, --charset unicode
```

Needs `screen`, `tmux` and `/usr/sbin/sshd` for the corresponding topologies.
All fixtures (SSH keys, sshd, sessions, sockets) are generated and removed.
