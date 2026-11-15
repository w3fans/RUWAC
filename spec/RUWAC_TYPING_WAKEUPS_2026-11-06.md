# TUI output wakeups — 2026-11-06

The TUI can wait for its next render deadline even after the engine has received
an application's echo. It now uses the existing `events.subscribe` stream to wake
the render loop after input. At most one hint is queued and each input batch
allows at most two extra wakeups. Other output retains the existing 8/25 ms
polling deadlines. Unsupported, failed or disconnected subscriptions fall back
to those deadlines. Input timestamps and Escape decoding depend on keyboard
input, not notification traffic.

The auxiliary connection retains socket ownership, private-directory and peer-UID
checks. Its optional connect refuses a full listen backlog without waiting. Reader
shutdown cancels blocked reads; frames and queue size remain bounded. The engine
protocol, PTYs, session identifiers, mouse semantics and archive format are unchanged.

## Measured result

An isolated Linux test compared the published v0.4.65 TUI (A), the changed TUI (B),
then the published TUI again (A). Every case used the same published v0.4.65 engine,
generated PTYs and enabled persistence. Additional windows were hidden within one
session. The history case generated 5,000 lines per window before measurement.
The busy case generated 50 Hz output in every window. Each quiet run measured
300 keystrokes; each busy run measured 150, for 3,150 total, with zero timeouts.

| Scenario | Baseline median, ms | Changed median, ms | Baseline p95, ms | Changed p95, ms |
|---|---:|---:|---:|---:|
| One window | 6.43–6.66 | 2.67 | 10.27–10.43 | 3.13 |
| 20 quiet windows | 6.65–6.82 | 2.95 | 10.44–10.62 | 3.56 |
| 20 windows with history | 7.21–7.23 | 3.29 | 11.30–11.34 | 4.05 |
| 20 busy windows | 7.55–7.92 | 3.16 | 12.79–13.06 | 5.06 |

The history case still had a 38.39 ms p99, versus 42.88–51.15 ms in the baseline
repeats. Typical latency improves; long delays remain a separate concern.

Quiet fixture CPU and sampled TUI RSS stayed close to the baseline repeats.
In the busy case, aggregate fixture CPU was 4.33 seconds versus 4.10/4.14 seconds:
about 5% more per key, or 11% more per second because the faster run finished sooner.
Three-second idle controls used 0.77 CPU seconds versus 0.72/0.73 with busy output;
quiet idle controls were similar. CPU includes the generated applications and
engine, rather than only the TUI. Terminal bytes per key stayed close to baseline.
An earlier uncapped prototype cost about 29% more CPU per busy key and was rejected.

These are TUI echo measurements on a bounded build/test host. GTK paint, physical
input, WAN/RDP latency, 20 simultaneous GUI attachments and endurance are outside
this result. SSH timing privacy and refresh intervals were not changed.

## Validation and reproduction

The existing `ruwac/tests/benchmarks/typing_latency.py` timing loop and glyph
detector were unchanged. Generated-fixture wrappers selected the fixed published
engine, prepared persistent history, waited for readiness and sampled idle resources.
The candidate executable SHA-256 was
`b3584bc59fd1db279bd15f9cf9cd98b5d2f2fbea7434a385459415bdf5f3ae1a`;
this is the experimental build before release version allocation.

Run the public benchmark against each binary in A/B/A order, for example:

```sh
python3 ruwac/tests/benchmarks/typing_latency.py \
  --binary /path/to/ruwac --evidence /path/to/private/evidence \
  --topologies ruwac --keys 300 --panes 20 --pacing active --output quiet
```

That command exercises the standard fixture; the fixed-engine/persisted-history
setup above requires its additional fixture preparation. Preserve the timing loop,
record source/binary hashes, and compare CPU, bytes, memory and tails with latency.

All 42 TUI unit tests passed, including subscription refusal, oversized frames,
EOF, cancellation, a full keyboard queue, hint coalescing, the per-input wake budget
and pending Escape decoding. Real-PTY checks passed split UTF-8, literal bracketed
paste, standalone Escape under continuous output, and detach/reattach with stable
application PID/start time for healthy, unsupported and EOF subscriptions. Existing
acceptance passed 100 startup races, Screen prefix/lifecycle controls, six nested
mouse checks and atomic paste limits. Release artifact/native acceptance remains
part of the normal release controller.
