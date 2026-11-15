RUWAC v0.4.80 — fewer typing stalls with busy terminal history

Reuse the temporary row buffer while encoding checkpoint history. Retained
history rows still own their text; byte limits, whitespace/wrap semantics,
styles, persistence intervals and canonical recovery format remain unchanged.

In a controlled local TUI comparison with twenty busy, history-filled windows,
p99 echo latency was 34.9–36.4 ms on the original engine and 16.6 ms on the
candidate. All 2,400 measured keys completed without timeouts; quiet latency
was essentially unchanged. These measurements exclude physical GTK painting,
RDP and WAN delay, and do not claim Screen/tmux parity.

Recovery and Unicode/blank/wrapped-row budget regressions are retained.
The desktop RSA advisory remains visible; Windows acceptance remains pending
on R7. Existing engines and active terminal programs survive package updates.
They retain their loaded code until the owner opens an updated engine.
