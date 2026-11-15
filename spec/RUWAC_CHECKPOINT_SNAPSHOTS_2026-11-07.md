# Checkpoint snapshots and typing tails — 2026-11-07

## Busy-history row scratch reuse — 2026-11-15 (release candidate)

Continuously changing panes cannot reuse a whole cached checkpoint. Encoding each
history row previously grew a fresh temporary string, then usually allocated its
trimmed copy. The candidate reuses one temporary row buffer per archive and still
copies each retained row into independent text storage. Empty/short rows do not
retain terminal-width allocations. History budgets, whitespace/wrap rules, styles,
checkpoint intervals, canonical file format and active PTYs are unchanged.

A bounded Linux A/B/A comparison kept the published v0.4.79 frontend fixed. Both
uninstrumented engine builds used the same compiler/toolchain. Twenty windows each
had 5,000 generated history lines and busy output; each deep run measured 600 keys.
Two shallow quiet controls measured 300 keys each. The existing public timing loop
and glyph detector were unchanged; persistence remained enabled. All 2,400 measured
keys completed without timeouts, with no cgroup CPU throttling.

| Deep-history run | p50 / p95 / p99, ms | Maximum, ms | Whole-fixture CPU seconds | Sampled engine RSS, KiB |
| --- | ---: | ---: | ---: | ---: |
| Original A1 | 2.625 / 4.408 / 36.430 | 54.332 | 17.08 | 339664 |
| Candidate | 2.507 / 3.766 / 16.569 | 30.912 | 16.07 | 335824 |
| Original A2 | 2.472 / 3.858 / 34.918 | 58.791 | 16.31 | 341116 |

Quiet p50 was 2.276 / 2.262 ms and p99 4.394 / 4.439 ms for original/candidate;
CPU was 1.77 / 1.76 seconds. Sampled RSS is not peak memory: deep-run high-water
marks were 339960 / 340248 / 341116 KiB. Checkpoint output remained about 5.8 MB/s
into private runtime tmpfs; this is not a disk-write reduction. These local TUI
measurements do not establish physical GTK, RDP, WAN or Screen/tmux parity.

All 164 headless workspace unit tests and real-PTY checkpoint-consistency,
primary-history and history-snapshot suites passed. The added regression compares
the old encoder's content and byte-budget decisions across blank/Unicode/wrapped
rows, alternate screens, resizing and 1/2/7/80/1024-column fixtures. One-column
fixtures use ASCII/combining text; width-two glyph input at one column exposed a
separate pre-archive panic and needs unchanged-baseline reproduction.

Baseline source commit: `d51e32be9c7d075996e5f3de3a9d3700fa23d7ef`.
Fixed public frontend SHA-256:
`350b38484b8de79c0e12298d48cb4fe01ef7db7a2ab52d2e771efd7069de9862`.
Original same-toolchain engine SHA-256:
`7708746bd8f312bf9aed31620a2333cdf6ae720dddb4acbe61f6b34c162d6971`.
Candidate engine SHA-256:
`eb987f912d6a4fc25bf243d09a321cf9359886de528d9dfffebfb4a1d266aea6`.
Release-version builds and downloadable artifacts still require the normal
independent qualification; these candidate receipts do not certify publication.

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
