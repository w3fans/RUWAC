RUWAC v0.4.62

The terminal viewer stops waiting for input when its next frame is already due.
This keeps the existing refresh budgets and reduces avoidable typing delay.
An isolated released/candidate/released comparison measured candidate median
latency of6.6ms versus7.6–8.7ms with one quiet window. Twenty-window output tests
also improved tail latency. These are local viewer measurements, not physical
display or real-network latency guarantees. The typing benchmark document
records CPU costs and the comparison's limits.

Package updates preserve running terminals and engines. Reattach with the
updated CLI to load the viewer change; the persistent engine can remain running.
An already-running nested viewer keeps its startup code until reattached.
ruwac engine status --json reports installed CLI and actual engine versions.

This development patch does not claim completion of the broader physical-input,
external-host or security qualification. The existing RSA advisory remains open
and unsuppressed.
