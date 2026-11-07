RUWAC v0.4.70

Reuse immutable checkpoint snapshots for terminal panes whose canonical state has
not changed. This reduces repeated reconstruction of hidden histories while another
terminal is active, preserving file validation, history, styles, schema, persistence
timing and full-file writes.

Three bounded A/B/A prototype comparisons with a fixed published frontend reduced
history-filled TUI p99 from 55–93 ms to 5.5–5.7 ms, with lower fixture CPU and
sampled engine RSS. Shallow controls stayed close to baseline. Continuously changing
panes still require reconstruction: the all-busy workload retained its long tail.
See spec/RUWAC_CHECKPOINT_SNAPSHOTS_2026-11-07.md for method and limits; this is not
a physical, GTK or WAN latency claim.

The change requires an engine using the new code. Package updates preserve existing
engines and their terminals; no owner engine, GUI, agent or transport is restarted.
Real-PTY canonical/hidden/resize/restore checks and dense64-pane dual-buffer recovery
have targeted qualification. Full128-live-pane, physical/external and endurance
acceptance remain open. Published artifacts receive their normal separate integrity
and runtime checks. The known RSA advisory remains unsuppressed.
