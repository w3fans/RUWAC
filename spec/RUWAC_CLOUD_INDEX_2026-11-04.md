# RUWAC cloud development: start here (2026-11-04)

**Scope:** Linux RUWAC in `w3fans/RUWAC` only. This document is intended for a contributor or a cloud-based coding agent with GitHub access but **no access to the maintainer's machines, private build controller, remote hosts, keys or current sessions**.

**Inspected baseline:** `main` commit `a4be3f60dc12667e2e3bf5c727ba1b8b0ae7de7c`, source version `0.4.56`. The GitHub `v0.4.56` release was published at 2026-11-03 23:57:45 UTC. Exact-commit CI quality and privacy jobs passed; the unsuppressed desktop dependency-advisory job failed on the known RSA finding. Recheck refs and CI before asserting status for later releases.

## Status update — 2026-11-04, `main` after v0.4.56

Reconciled against `main` at the commit that merges this document; the v0.4.56
audit below remains a dated snapshot of `a4be3f60`.

- **Workstream B delivered:** PR #7 (`e13ff2fc`) runs the non-sizing viewer
  real-PTY fixtures and the older-daemon refusal in the public `quality` job. The
  authenticated SSH viewer fixture remains a separately qualified local gate.
- **Ordinary merges no longer fail Source release:** commit `4cd87dcb` adds
  `scripts/release-mode.py`. A push without the controller-assigned version tag at
  that exact commit is a development commit and publishes nothing; tagged
  releases keep every publication check. Covered by
  `ruwac/tests/integration/test_release_mode.py` in the `quality` job.
- **Workstreams C1 (Open remote web service…) and D (typing-latency benchmark)
  are being implemented on the maintainer's workstation** as separate increments.
  Do not start parallel cloud PRs for them; review those PRs instead.
- Main integration of documentation and CI changes is done by the maintainer's
  workstation as fast-forward merges. Cloud work still goes in draft PRs.

## Reading order and authority

1. **This entry point:** [cloud index](RUWAC_CLOUD_INDEX_2026-11-04.md) — source of truth for where to read, not an assertion of release qualification.
2. **Current independent review:** [v0.4.56 audit](RUWAC_CLOUD_AUDIT_v0.4.56_2026-11-04.md) — observed source, document discrepancies, open questions, verification boundaries.
3. **Actionable development:** [cloud work plan](RUWAC_CLOUD_WORK_PLAN_2026-11-04.md) — small PR-sized tasks and exit criteria.
4. **Product/readme:** [README](../README.md), [CONTRIBUTING](../CONTRIBUTING.md), [current CHANGELOG](../CHANGELOG.md), [release notes](../RELEASE-NOTES.md) — what users currently get and the agreed contribution process.
5. **Implementation status (historical plus dated receipts):** [acceptance](RUWAC_ACCEPTANCE_STATUS.md), [functionality audit](RUWAC_FUNCTIONALITY_AUDIT.md), [2026-10-29 audit response](RUWAC_AUDIT_RESPONSE_2026-10-29.md), [viewer contract](RUWAC_VIEWER_SIZING.md).
6. **Design-specific detail:** [API contract](RUWAC_API_CONTRACT.md), [architecture decisions](RUWAC_ARCHITECTURE_ADR.md), [clipboard/RDP matrix](RUWAC_CLIPBOARD_RDP_TEST_MATRIX.md), [dependency security](RUWAC_DEPENDENCY_SECURITY.md), [Fedora source packaging](../ruwac/packaging/fedora/README.md).
7. **Early project history, not a modern implementation backlog:** [2026-10-10 roadmap](../architecture/06-IMPLEMENTATION-ROADMAP.md), [early open issues](../architecture/05-OPEN-ISSUES.md), [large historical current-state notes](RUWAC_CURRENT_STATE.md), [large chronological next-implementation notes](RUWAC_NEXT_IMPLEMENTATION.md).

The early roadmap still describes a pre-GTK/pre-release experiment. Do **not** treat it as an instruction to reimplement delivered features. The long chronological documents preserve dated decisions; their headings are not evidence that every stated task is still open.

## Source map

| Component | Location |
|---|---|
| Persistent session, pane, geometry, rendering protocol state | `ruwac/crates/ruwac-core/src/lib.rs` |
| Non-sizing viewer contract | `ruwac/crates/ruwac-core/src/viewer.rs` |
| Private daemon and PTY | `ruwac/crates/ruwac-daemon`, `ruwac/crates/ruwac-pty` |
| Canonical cell grid and scrollback | `ruwac/crates/ruwac-term` |
| Screen-style TUI and copy | `ruwac/crates/ruwac-tui` |
| CLI, authenticated SSH --host | `ruwac/crates/ruwac-cli` |
| Desktop terminal/worker, focus/clipboard | `ruwac/desktop/src/view.rs`, `worker.rs`, `clipboard.rs` |
| RDP | `ruwac/desktop/src/rdp.rs` and protocol modules |
| Remote browser | `ruwac/desktop/src/web.rs`, `web_route.rs`, `ssh_transport.rs` |
| Agent launch and SSH profiles | `ruwac/desktop/src/agent_launch.rs`, `profiles.rs`, `launch_context.rs` |
| DNF/COPR source packaging | `ruwac/packaging/fedora` |
| Public reproducibility, privacy and release gates | `.github/workflows`, `scripts/acceptance-gate.py`, `scripts/public-files.py` |
| PTY/native/SSH integration fixtures | `ruwac/tests/integration` |

## Current important decisions

- **Mouse interaction is settled:** preserve application-owned mouse handling when apps request it; Shift+drag selects RUWAC text as an explicit override. Ordinary selection works when apps do not claim the mouse. The maintainer explicitly requested **no new mouse-priority mode**.
- Keep native Fedora RUWAC, CLI and daemon separate from the Android companion repository. This is a Linux handoff.
- Daemon-owned terminals and user processes survive detaching/reopening the GUI. Do not restart daemons or agents merely to run a test.
- The v0.4.53 non-sizing CLI/SSH viewer is delivered, but controller-owned sizing and the desktop GUI viewer choice are not finished. A viewer is still allowed to send input; it is **non-sizing**, not an authorization barrier.
- Remote Web Preview exists. It uses an explicitly bound current SSH terminal and a temporary, verified loopback-forward over the original SSH master. Direct machine/service selection is still a proposed usability improvement.
- Avoid claiming a clean dependency scan: desktop inherits an unsuppressed RSA advisory. A CI run with passing quality/privacy and failed advisory is **not** fully green.
- The source-built Fedora/COPR packaging is separate from older prebuilt GitHub testing RPMs. An authorized distribution installation should be updated by DNF, never a competing GitHub installer.

## Cloud-only contribution rules

1. Use a **new branch / small PR** based on a pinned current commit. Do not push to `main`, create a tag, run the private release controller, edit product versions, or publish packages.
2. No access to maintainer's workstation, production SSH connections, VPN, real credentials, machines, clipboard, personal agent sessions or private evidence is implied by having GitHub access.
3. Develop against **generated data and isolated fixtures**. State exactly which Rust/Python/CI tests you ran or could not run. Never claim physical gestures or external RDP acceptance from mocks.
4. Preserve the headless dependency graph's independence from GTK, WebKit and IronRDP; preserve process IDs, stable targets and existing keyboard/clipboard semantics.
5. Keep source, test evidence and CI secrets private as required by [CONTRIBUTING](../CONTRIBUTING.md) and `scripts/public-files.py`.
6. `scripts/public-files.py` explicitly excludes root `CLAUDE.md`, `AGENTS.md` and agent-private directories. Use allowlisted `spec/*.md` files, GitHub issues and PR descriptions instead. Do not bypass this boundary.
7. If the working branch is overtaken by a new upstream version, rebase/re-review instead of claiming an old audit qualifies new code.
8. Before starting work, read open issues and PRs to avoid duplicating another agent's work.

## Immediate suggested cloud assignments

First: docs/current-state reconciliation, generated viewer regression in public CI, and isolated latency measurement infrastructure. Second: a bounded remote-service Web Preview UX PR, after examining existing binding/security tests. Do not change mouse selection.

Use the [cloud work plan](RUWAC_CLOUD_WORK_PLAN_2026-11-04.md) for exact gates, or pick one task from its list and submit a draft PR. Cloud-only changes should be reviewed and integrated through the owner's usual controlled release workflow.
