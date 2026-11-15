# Open issues

## Historical prototype snapshot — clarified 2026-11-04

The experiment and pending milestones below describe the October 2026 prototype,
not the present feature backlog. Linux GTK, SSH/RDP, clipboard/image attachments,
persistent recovery and published packages were subsequently implemented. Their
existence does not prove complete physical, external, endurance or security
qualification. Preserve this snapshot as history and use the
[current development index](../spec/RUWAC_CLOUD_INDEX_2026-11-04.md),
[acceptance status](../spec/RUWAC_ACCEPTANCE_STATUS.md) and
[current work plan](../spec/RUWAC_CLOUD_WORK_PLAN_2026-11-04.md) for new work.

Current desktop interaction and embedded IronRDP state is recorded in [the acceptance report](../spec/RUWAC_ACCEPTANCE_STATUS.md) and [current architecture decisions](../spec/RUWAC_ARCHITECTURE_ADR.md). Earlier measurements and design limits below are historical evidence.


The spike is not a daily-driver or release candidate. No desktop pass is inferred from headless tests.

- **BLOCKED — desktop acceptance:** Wayland CLIPBOARD/PRIMARY, outer-terminal Ctrl-Shift-C/V, physical touchpad/divider drag, actual image attachment and nested desktop clipboard. A real authorized graphical workstation is required. No personal clipboard contents were accessed.
- **BLOCKED — remote:** a second host, authenticated remote daemon lifecycle, checked PNG transfer and an application consuming the attachment. Local SSH authentication to GitHub is repository access, not this product test.
- **PARTIAL — Screen compatibility:** c/n/p/d/a, doubled prefix, digits, S and Tab are implemented. `[` is bounded whole-capture copy, not interactive selection. `"`, `|`, `:`, scrollback search, keymap reload, pane move/zoom, vertical layout and full Screen conflict comparison remain pending.
- **PARTIAL — rendering:** canonical cell colors/wide/combining/emoji and alt-screen state are checked. TUI does not represent every attribute, dynamic palette/title event, Kitty keyboard protocol or graphics. Visual acceptance in real emulators is open.
- **UNTESTED — long operation:** 2–5 nested multiplexers, prolonged soak, high-throughput output, queue saturation under many blocked children and fragmented hostile-protocol fuzzing beyond the bounded fixture cases.
- **PARTIAL — lifecycle:** children survive client loss while the daemon is healthy. State is in memory. Daemon death/reboot cannot resurrect children. Cleanup targets owned leader/foreground groups; background jobs that deliberately change sessions are outside this spike's supervision contract. SIGKILL cannot run client terminal cleanup.
- **PARTIAL — security:** same-UID is the trust boundary; any admitted peer can control owned panes and stop the daemon. Cross-UID negative tests remain unverified. Ancestor symlink/hardlink races against another process of the same trusted UID are not a sandbox. Protocol is experimental, not a stable compatibility commitment.
- **PARTIAL — idempotency:** bounded, in-memory and daemon-lifetime only. Eviction/restart permits repetition. Do not use it as durable exactly-once remote execution.
- **UNTESTED — shipping:** product license, namespace/trademark review, Fedora RPM, macOS artifacts, dependency notice bundle, CI and branch protection are future milestones. No versioned binary release is published.

The matching Fedora rustfmt 1.98.1 package was installed for source formatting (about 5.5 MiB installed). DNF reported invalidating a pending offline update transaction, as during the earlier toolchain installation. No reboot, system upgrade, live session reset or personal dotfile change was performed.
