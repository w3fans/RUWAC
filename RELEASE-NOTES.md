RUWAC v0.4.66

Terminal views now wake promptly after input using existing engine change
notifications. Extra render work is capped at two wakeups per input batch;
older engines and disconnected streams retain the existing polling fallback.

Isolated A/B/A tests against the published v0.4.65 engine reduced typical TUI
echo latency from about 6–8 ms to 3 ms, including 20-window fixtures. All 3,150
measured keystrokes completed without timeouts. History-heavy tails remain.
The busy-output fixture used about 5% more CPU per key (11% more per second);
quiet CPU/memory stayed close to baseline. GTK paint and physical RDP/WAN
latency are outside these measurements. See the typing-wakeup report for scope.

Unicode, bracketed paste, mouse routing, scrollback and persistent application
processes are preserved. Validation covers notification failure, bounded queues,
Escape decoding, attach/startup races, paste boundaries and stable child PIDs.

COPR provides both ruwac and ruwac-desktop for Fedora 43, 44, 45 and Rawhide
on x86_64. For immediate updates, use:

    sudo dnf upgrade --refresh ruwac ruwac-desktop

An already attached terminal frontend keeps its old code after installation.
Detach that frontend (default: Ctrl+A, then d), then reattach the same session
with ruwac -r SESSION. Its engine and running programs can stay alive.
Reopen the GUI to load its installed version.

This is a development patch; the existing RSA advisory remains open and
unsuppressed. Stable milestone acceptance remains separate.
