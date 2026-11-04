# Optional render palette — measured candidate results (2026-11-04)

Status: **unshipped candidate**, requiring the normal full release gate. See the
[design and bounds](RUWAC_RENDER_PALETTE_PROPOSAL_2026-11-04.md).

One Fedora 44 host, six CPUs, 40×120 cells, active pacing, 80 measured keys per
path per run. The same corrected observer script ran unchanged baseline,
experimental candidate, then unchanged baseline again (A/B/A). The baseline
is the post-57 integration candidate at `ad8ab3d`, before the palette change.
No polling interval, socket framing or OpenSSH timing/privacy setting changed.
Each run interleaves topologies in blocks of 20; cold first keys are separate.

The shared host's load varied. Report both baselines rather than attributing all
control variation to this change. SSH is a generated loopback hop with no network
RTT. GUI means protocol frame receipt and Python glyph extraction, excluding
Rust GUI cell expansion, GTK painting, compositor and physical display latency.
The real TUI timings include its Rust decoder. Native GUI tests below establish
functional behavior, not physical input latency.

## Quiet output: p50 / p95 milliseconds

| Scenario | First baseline | Candidate | Reverse baseline |
| --- | ---: | ---: | ---: |
| ASCII TUI | 18.89 / 28.35 | 11.37 / 15.48 | 16.01 / 22.54 |
| ASCII GUI protocol | 17.46 / 25.38 | 10.71 / 16.12 | 11.24 / 22.78 |
| Unicode TUI | 18.92 / 29.68 | 12.71 / 17.66 | 19.00 / 33.60 |
| Unicode GUI protocol | 18.36 / 28.36 | 4.92 / 15.59 | 15.28 / 35.18 |
| SSH → TUI | 31.31 / 44.26 | 23.03 / 34.97 | 30.14 / 48.80 |
| GUI protocol → local pane → SSH → remote RUWAC | 54.48 / 70.17 | 33.28 / 45.52 | 51.74 / 83.04 |

All 3,600 measured keys passed with zero timeouts. Local TUI and nested SSH
improvements remain visible against both baseline orders. Local ASCII GUI median
varied substantially between the two baselines; its isolated improvement is less
certain. Screen/tmux remain faster for local echo, so responsiveness work is open.

## Sustained output with five windows per session

A separate paired run used 40 measured keys per path, four interleaved topologies,
five windows each, and 50 Hz alphanumeric redraws in the measured and hidden
windows. The same one-host resource limits and script apply.

| Path | Baseline p50 / p95 ms | Candidate p50 / p95 ms |
| --- | ---: | ---: |
| GNU Screen | 1.19 / 5.21 | 0.94 / 4.57 |
| tmux | 0.67 / 6.50 | 0.98 / 7.20 |
| RUWAC TUI | 29.97 / 45.76 | 17.28 / 33.05 |
| RUWAC GUI protocol | 24.74 / 49.70 | 11.57 / 25.00 |

All 320 measured keys passed with zero timeouts. This covers one five-window
scenario, not the full 1/5/20-window, output and external-network matrix.

## Traffic and CPU

Local ASCII GUI render replies fell from about 158 KB to 12 KB per measured key;
nested GUI replies from about 284 KB to 26 KB. A separate real-PTY equality test
compared a quiet colored/Unicode 12×60 frame: 30,798 plain JSON bytes versus
5,646 palette bytes, with exact decoded cells, modes and metadata.

Whole-fixture CPU per measured key (all interleaved topologies combined) was
5.45/4.98 ms in the local ASCII baselines versus 3.08 ms in the candidate;
13.38/14.20 ms in the SSH baselines versus 8.43 ms; and 43.81 versus 27.25 ms
under sustained output. These are fixture totals, not per-application costs.
The existing full-frame micro-probe explicitly requests plain frames; it remains
a plain-encoding control rather than a measurement of the new compact response.

## Functional qualification and remaining work

Headless workspace and all 237 desktop units pass. New tests reject malformed
palette cells, indices, dimensions, ambiguity and expansion bombs before cloning.
Exact round trips cover RGB/indexed/default colors, extended styles, combining
and wide glyphs, normal/alternate buffers, input modes and restored snapshots.

Real PTYs verify old clients against the new engine and new clients against
preserved v0.1.9/v0.4.49 engines with pane PIDs preserved, plus encoding changes
at unchanged revisions. Viewer, projection, styles/fzf, mixed Screen/tmux nesting
and natural-grid suites pass. Sixteen native Web Preview groups pass with the
new pair. Six native styles/fzf/recovery groups pass both with the new engine
and with preserved v0.4.49. Generated private state and test displays only.

The original failed decoder test is retained: a no-grid legacy frame acquired
`grid: null`; the decoder now leaves such frames untouched. A native styles
fixture initially lacked fzf; the subsequent run stages the actual Fedora fzf
binary privately, without installing or configuring software on the owner host.

The candidate is not installed into owner sessions. Full release, actual-public
artifact qualification, broader resource/20-window checks and physical/external
input timing remain open. Profile remaining scheduling and Rust/GTK work before
considering event delivery or negotiated deltas. Do not infer full goal completion.
