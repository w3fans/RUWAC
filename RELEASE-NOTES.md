RUWAC v0.4.71

Move already-owned decoded terminal rows directly into render frames, avoiding a
second JSON value tree. Keep the cell shape, styles, Unicode, selection behavior,
validation/expansion budgets and preserved-engine fallback unchanged. No protocol,
refresh timer, RDP transport or terminal lifecycle changes.

Three bounded A/B/A frontend comparisons with a fixed public engine reduced
ordinary TUI median echo by 20–26 percent and aggregate fixture CPU by 5–10 percent.
Additional public-baseline, Unicode, busy-history and frontend-memory controls
passed their budgets. The public-baseline control used 3.8 percent more CPU than
its worse adjacent baseline; no universal CPU/p99 improvement is claimed.
See spec/RUWAC_RENDER_EXPANSION_2026-11-07.md for methods and limits. These are
received-output measurements, not physical, GTK, RDP or WAN latency acceptance.
Continuously busy history still has checkpoint-related stalls.

Reopen the GUI and detach/reattach terminal frontends to load this client change.
Existing engines and their programs can stay running. No owner GUI, engine,
agent, clipboard, profile or transport was restarted or changed for qualification.
All 161 headless unit tests and palette/legacy-fallback, large-grid, projection,
history and atomic-paste PTY suites passed before integration. Public artifacts
receive separate normal integrity/runtime checks. The known RSA advisory remains
unsuppressed; physical acceptance and broader product milestones remain open.

Every tagged release now also builds a headless Linux x86_64 CLI archive and
Debian package in parallel with Fedora. The same portable binary is tested on
Ubuntu 22.04/24.04 and Debian 12/13, including installation, Unicode PTY input,
attach/detach, persistence and reinstallation without restarting running programs.
Checksums, source provenance and an OS test receipt accompany the packages.
See README.md for installation. Actual Proxmox host acceptance and the portable
GTK desktop remain pending; no RUWAC APT repository is provided yet.
