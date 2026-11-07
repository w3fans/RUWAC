# Terminal render expansion — 2026-11-07

Compact terminal frames contain a palette and cell indices. Clients validate the
whole frame before expanding it for existing drawing and selection consumers.
The decoder previously cloned cells into owned rows, then serialized those rows
into another JSON value tree. It now moves the owned rows into the frame directly.
The frame shape, validation limits, plain-frame fallback and consumers are unchanged.
No refresh interval, terminal lifecycle or RDP transport changes.

The public typing benchmark's timing loop and glyph detector were unchanged.
Three A/B/A repetitions held the published v0.4.70 engine fixed and varied the
frontend between the qualified v0.4.70 build and the candidate on one build host.
Each run measured 400 keys through the TUI and 400 through the unchanged
GUI-protocol negative control, with 20 windows per path and shallow quiet output.
The fixture was bounded to 4 GiB and two CPU cores. This measures received terminal
output, not physical input, GTK painting, RDP or WAN latency.

| Repetition | Baseline TUI median, ms (A1/A2) | Candidate median, ms | Aggregate fixture CPU change |
| --- | ---: | ---: | ---: |
| 1 | 2.887 / 3.044 | 2.233 | −9.5% |
| 2 | 3.067 / 3.005 | 2.401 | −5.3% |
| 3 | 2.930 / 2.930 | 2.237 | −6.8% |

All 7,200 primary keys completed without timeouts; p95 improved in each repetition.
Another 4,800 keys covered the public frontend baseline, Unicode and continuously
busy history. Those controls passed their latency/resource budgets. Sampled frontend
peak RSS was 1.08–1.35 MiB lower. The public-baseline control improved median/p95 but
used 3.8% more aggregate CPU than its worse adjacent baseline, within the 5% budget;
its p99 did not improve. No universal CPU or p99 improvement is established.

Continuously changing history still causes checkpoint-related stalls. An additional
4,200-key diagnostic reproduced busy-history TUI p99 at 43.0/40.5 ms around a 6.7 ms
shallow control; all 26 echoes above 20 ms occurred near checkpoint creation.
This decoder change does not resolve that engine bottleneck or establish parity
with Screen/tmux. Default shallow comparisons showed those programs substantially
faster; their persistence and history behavior differs from RUWAC's.

All 161 headless unit tests and real-PTY palette/fallback, large-grid, projection,
history and atomic-paste suites passed. Moving a test-only import after measurement
produced an identical binary, preserving the exact measured artifact.

Baseline source: `3fa192ee5c8afd4f55b81f2cf0db3806fab1b009`.
Fixed public engine SHA-256:
`0272704589997d4ce247d3e527231e7ccf52b925814ee528d43c8e33d661ce7b`.
Controlled baseline frontend SHA-256:
`a3dafa8f740c7f153fe1c420ca15254be458fda3082a2d8ea82784f720f299d6`.
Measured candidate SHA-256:
`aaf2cf10a1842e5c5214d947468dd253f621af407d3536ce392a21a3a10d30cf`.

The benchmark is `ruwac/tests/benchmarks/typing_latency.py`. The controlled comparison
additionally selected the engine separately from the frontend and sampled process
resources at existing CPU boundaries. Run independent comparisons in A/B/A order;
retain CPU, bytes and tail latency alongside medians. Release-version artifacts
require their normal independent validation.
