# Checkpoint snapshots and typing tails — 2026-11-07

Periodic persistence reconstructs canonical history for every pane before its
background writer starts. With many history-filled panes, that synchronous work
can delay input even when most panes have not changed.

The engine now retains one immutable checkpoint archive per pane. It reuses that
archive until the terminal's canonical history epoch changes. Output, mode resets
and resizing invalidate it; removing a pane releases its cache. An in-flight writer
holds immutable references, so later output cannot change its snapshot. This cache
does not depend on the number of attached clients.

Serialization retains the existing plain archive shape. File validation, private
ownership checks, size limits, atomic writes, persistence intervals, PTY identities,
history limits and mouse behavior are unchanged. Full checkpoint writes remain;
this change does not claim a disk-write reduction. Existing engines keep running
their original code until the owner chooses an appropriate replacement boundary.

## Controlled comparison

The published v0.4.69 TUI stayed fixed while the engine alternated between the
public v0.4.69 binary and the prototype, in three A/B/A repetitions. Each run used
20 windows per TUI/GUI-protocol path, 5,000 generated history lines per window,
enabled persistence and 400 measured keys per path. The public timing loop and
glyph detector were unchanged. Private filesystem observations recorded checkpoint
creation without forcing a checkpoint during timing.

All 7,200 measured keys completed without timeouts on a Linux test host bounded
to 4 GiB and two CPU cores.

| Repetition | Public engine TUI p99, ms (A1/A2) | Prototype TUI p99, ms | Fixture CPU change | Sampled engine RSS change |
| --- | ---: | ---: | ---: | ---: |
| 1 | 55.22 / 67.45 | 5.46 | −33.8% | −40.7 MiB |
| 2 | 68.85 / 62.96 | 5.74 | −31.8% | −40.2 MiB |
| 3 | 67.48 / 93.02 | 5.70 | −30.6% | −39.7 MiB |

Typical TUI medians stayed around 3.2–3.5 ms. CPU covers the whole fixture and is
compared per equal key count. RSS is a sampled engine value after history prefill,
not a whole-desktop or universal memory reduction.

Additional shallow and busy A/B/A controls completed 2,400 keys without timeouts.
Shallow latency stayed close to its controls. With 50 Hz output configured in
every pane, TUI p99 remained about 87–91 ms and CPU stayed within the baseline
range: continuously changing panes still require reconstruction. That workload
does not establish an improvement. Sampled RSS was about 8–9 MiB lower in these
two controls.

GTK painting, physical input, network/RDP delay and long-term endurance are outside
these measurements. They do not establish Screen/tmux-equivalent physical latency.

## Recovery and reproduction

All 161 headless workspace unit tests passed. Three new unit cases cover immutable
writer snapshots, output/mode/resize invalidation, independent panes, cache lifetime
and the existing serialized archive shape. The real-PTY checkpoint-consistency
fixture covers seven persistence and restore cases; invalid-output preservation,
primary history, immutable history views and mobile capture checks also passed.

The unchanged dense-capacity fixture passed with 64 live panes at 100×200 cells
for both engines. Colored Unicode contents in both buffers survived an owned
daemon SIGKILL and restore, without process resurrection. Each daemon had a
1,536 MiB memory limit; the driver and daemon together stayed below 4 GiB/two CPUs.
Full 128-live-pane acceptance remains pending because its reader/writer/child
tasks exceed the test infrastructure's 256-task limit. That limit was not raised.
These cases do not cover arbitrary cell entropy or saturated scrollback.

Baseline commit: `c8cee23a28a2c575761aa5b32ebbd5a93305a384`.
Public engine/fixed TUI SHA-256:
`28bcee191e25e26f95be611fdda738292a422fe63575be5d1b865c35ad2aa454`.
Prototype SHA-256:
`bcdc2c36a5e14d4fb4a7823be2d3e52cb1ee62cf7e2c46cbaf684b22f9397b59`.
The prototype precedes release-version allocation; release artifacts require their
normal independent build and validation.

The public benchmark is `ruwac/tests/benchmarks/typing_latency.py`. This comparison
additionally used generated prefill, readiness and initial persistence preparation,
a fixed frontend with a selected engine binary, and passive resource/checkpoint
observers. Preserve those controls and compare latency tails, CPU, bytes and RSS
together when reproducing the experiment.
