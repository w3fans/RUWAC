# RUWAC cloud-only development plan after v0.4.56 — 2026-11-04

**This plan is for independent GitHub-based PRs, not an instruction to run the owner's local release controller.** Start with [the cloud index](RUWAC_CLOUD_INDEX_2026-11-04.md) and [the source audit](RUWAC_CLOUD_AUDIT_v0.4.56_2026-11-04.md). Baseline is `a4be3f60dc12667e2e3bf5c727ba1b8b0ae7de7c` at source and published release version `0.4.56` (published 2026-11-03 23:57:45 UTC). Exact-commit quality/privacy passed; dependency advisory failed on the known RSA finding. Recheck before using newer refs.

## Maintainer constraints — settled decisions

1. **Keep mouse interaction exactly as presently designed.** Plain drag selects in shell/non-reporting mode; Shift+drag overrides mouse-reporting applications. Do not implement a selection-priority toggle.
2. Running PTYs and real Codex/Claude work must survive frontend detach, software updates and unrelated tests. Never stop, migrate, duplicate or replace owner's active sessions.
3. No direct commits to main; no releases, version bumps, COPR credential handling, SSH to maintainer hosts or live installation upgrades. Each cloud change goes in an isolated, reviewable PR.
4. The Android companion is a **different project**. This plan concerns Linux RUWAC exclusively.
5. Do not add root `CLAUDE.md`, `AGENTS.md` or agent-secret directories; the public inventory excludes them. `spec/*.md` is allowed.

## Workstream A — Documentation and acceptance reconciliation [P0; first cloud PR]

**Problem:** active planning docs have no current index, and several lead status statements lag behind released source.

**Concrete targets:** `spec/RUWAC_ACCEPTANCE_STATUS.md`, `spec/RUWAC_FUNCTIONALITY_AUDIT.md`, `spec/RUWAC_CURRENT_STATE.md`, `architecture/05-OPEN-ISSUES.md`, `architecture/06-IMPLEMENTATION-ROADMAP.md` and [issues #2 and #4](https://github.com/w3fans/RUWAC/issues).

**Implementation:**

- Add an **additive, dated** top-of-file reconciliation for v0.4.56 source and exactly whatever release/CI completed at execution time. Keep historic evidence intact and clearly labeled as dated.
- Distinguish `implemented`, `source-tested`, `public CI`, `qualified published artifacts` and `owner physical/external`. Do not infer success from a passing release workflow if advisory checks fail.
- Mark the October 10 architecture roadmap and open-issues documents as **historic prototype decisions** with links to the current index. Avoid mass rewriting or deleting them.
- Comment on issue #2 (Web Preview implemented, direct machine/service UX and external qualification pending) and #4 (CLI/SSH viewer partial, GUI controls/controller lease/pan pending) after the maintainer reviews the issue text.
- Update README navigation to this index **only through a controlled documentation PR**; preserve upstream build/release instructions.

**Exit:** link audits and source to exact commit; source inventory/privacy checks pass; no version edits; a contributor can identify five current tasks without parsing the 100 KB chronological notes.

## Workstream B — Make non-sizing viewer tests repeat in hosted CI [P0; isolated PR]

**Source:** `ruwac/crates/ruwac-core/src/viewer.rs`, `ruwac/crates/ruwac-tui/src/lib.rs`, `ruwac/tests/integration/client_sizing_viewer.py`, `viewer_legacy.py`, `ssh_sizing_viewer.py`.

**Findings:** v0.4.53 introduced opt-in non-sizing CLI/SSH viewers. In the inspected `.github/workflows/quality.yml`, the main headless integration loop **does not explicitly invoke** the three new viewer scripts. Their existence and local acceptance receipts do not ensure every PR rechecks them.

**Implementation:**

- Add the local bounded real-PTY viewer fixture to the existing quality workflow after the headless binary is built. Use a new ephemeral socket, generated HOME/XDG paths and evidence under RUNNER_TEMP.
- If feasible, add preserved-older-daemon refusal with an immutable, hash-verified fixture; otherwise document why it remains a separate qualified gate.
- Add the authenticated loopback SSH viewer tests only with entirely generated keys/hosts and bounded resource controls. Do not require external owner infrastructure or a new privileged service.
- Assert unchanged PID, TIOCGWINSZ/SIGWINCH count, 100 attach/detach/resize cycles, viewer-only last-sizer freeze, denied structural edits, distinct session namespace and old daemon refusal.
- Keep CI duration bounded. Correct test harnesses rather than deleting assertions when the job fails.

**Exit:** exact PR SHA GitHub Actions succeeds for viewer checks; if a full SSH fixture is unavailable in public CI, say so explicitly rather than claiming it passed.

## Workstream C — Direct remote web-service preview UX [P1; separate GUI PR]

**Goal:** from a saved authorized SSH machine, easily view a web application running on that machine's loopback port inside RUWAC.

**Existing foundation:** `ruwac/desktop/src/web.rs`, `web_route.rs`, `ssh_transport.rs`, `profiles.rs`, `launches.rs` and `ruwac/tests/integration/web_ssh_preview.py`. The current Ctrl+click-from-original-SSH-terminal route already works in generated fixtures.

**Stage C1 (low risk):** provide an action on an already opened, verified SSH terminal: **Open remote web service…** with an explicit positive integer port, optional relative path and a summary of exactly which current endpoint/transport will be used. Call the existing checked `web::open` / `Source::capture` path instead of constructing a second tunneling subsystem. Never treat CVF/workstation localhost as the intended remote host.

**Stage C2 (later, separate owner-approved design):** a machine-sidebar action may establish an **explicit user-requested**, newly authenticated SSH connection first, then create the preview using the same verification and lease contract. Never silently choose some other SSH session, resolve a changed alias or bypass a host-key prompt. Do not auto-start hidden connections on workspace restore.

**Required tests:** two different generated SSH identities with same remote port, correct original endpoint, changed credentials/profile refusal, WebSocket/HMR, redirects, close and abrupt death teardown, screenshot/source selection, no clipboard/focus regression, no cross-origin escalation. Make limitations of subresource/third-party network requests explicit.

**Exit:** test-backed first-party GTK UI, clear status and failure message, no manual ssh -L required, no unapproved access to other private network ports.

## Workstream D — Reproducible remote typing-latency instrumentation [P1; benchmark-only PR first]

**Problem:** owner perceives remote nested RUWAC as perhaps slower than GNU Screen/tmux; current source intervals do not prove an end-to-end regression.

**Source to inspect:** `ruwac/desktop/src/worker.rs`, `ruwac/crates/ruwac-tui/src/lib.rs`, `ruwac/crates/ruwac-core/src/lib.rs`, `ruwac/tests/benchmarks/api.py` and `architecture/04-BENCHMARKS.md`.

**Implementation:**

1. Build a disposable echo/marker PTY fixture with a single observer monotonic clock and explicit byte correlation. Do not compare unsynchronized remote and local clocks.
2. Measure: input arrival, RPC/daemon admission, child PTY output, returned render/frame and local paint scheduling. For native GUI, label synthetic/Wayland callback timing separately from real photons.
3. Alternate direct ssh, GNU Screen, tmux and RUWAC local/SSH/nested configurations under identical generated host conditions. Report warm/cold, 1/5/20 panes, low/occupied output, Unicode, quiet/active, p50/p95/p99 and CPU/bytes.
4. Identify the actual dominant segment before changing polling (8/20/100/250ms) or wire protocol.
5. Evaluate event-driven push or negotiated render deltas in a separate proposal only if evidence supports material wins. Maintain old daemon fallback and bounded queues; do not create a busy loop.

**Exit:** reproducible script and sanitized result schema; no speed claim without a measured reference. Benchmark-only change should not alter production timing.

## Workstream E — Distribution and dependency check [P1; research/guard PR]

**Source:** `ruwac/packaging/fedora`, `spec/RUWAC_DEPENDENCY_SECURITY.md`, `ruwac/desktop/Cargo.lock` and `.github/workflows/quality.yml`.

- Inspect DNF/COPR source installation upgrade graph and cleanup of old per-user links through generated fake package managers; distinguish acceptance that needs an actual COPR runner or user system.
- Qualify that v0.4.56's refreshed --repo-scoped DNF commands update only approved repositories, preserve installed desktop alongside headless, do not introduce duplicate installation PATH precedence, and never restart an active daemon.
- Investigate whether patched upstream IronRDP/SSPI feature separation can remove unused private-key RSA dependencies. **Do not suppress `RUSTSEC-2023-0071` or apply a homegrown cryptographic fix.** A fresh normal-target dependency tree plus RDP tests is mandatory for remediation claims.
- Keep GitHub testing RPMs and Fedora/COPR source-build output clearly distinguished. No implicit public COPR publishing from cloud CI.

**Exit:** testable plan / PR with exact dependencies and outstanding warnings. Do not claim approval for official Fedora inclusion without package review.

## Workstream F — Session UX and notifications [P2; only after A–D]

Useful feature candidate: optional activity/silence markers for existing background terminal tabs, with a configurable interval and no new per-pane remote polling storm. A quiet tab does **not** necessarily mean an agent has finished, and terminal text is **not** structured agent state. Start with a design/prototype PR after proving low event-processing overhead; keep it disabled by default until accepted.

Potential future: GTK non-sizing viewer mode, explicit resize-controller lease/take-over and small viewport pan/fit. This remains unfinished under [issue #4](https://github.com/w3fans/RUWAC/issues/4). Prototype one component at a time; do not silently convert every attached session to viewer.

Avoid adding full Screen configuration-language emulation, synchronized pane typing or broad hooks without a specific user workflow.

## Cloud agent output contract

For each chosen workstream, produce:

- `BASE_SHA` and `HEAD_SHA`; exact branch and changed paths;
- a one-paragraph problem statement and behavior change;
- a small source diff plus tests, not a generalized refactor;
- each command actually executed and its pass/fail result, including skipped hardware tests;
- safety review of existing PTYs, authenticated SSH identity, clipboard focus, headless dependency boundary and packaging ownership where relevant;
- remaining blockers for owner/CVF external acceptance;
- a PR that **does not modify version/release/tag artifacts** and does not claim a published success.

Use PR review and the existing release controller after owner approval. A passing isolated test is not a substitute for the complete frozen release gate.

## Recommended parallel schedule

| Cloud worker | Initial PR | Dependency |
| --- | --- | --- |
| Documentation | A: current acceptance/source reconciliation | none |
| Headless Rust/CI | B: viewer regression workflow | none |
| Performance | D: no-production-change latency harness | none |
| GTK | C1: terminal-bound remote-service dialog | reviewer coordination with current GUI worker |
| Dependency/packaging | E: source/Rust feature-graph analysis | none |

Keep worktrees and PRs separate; never parallel-edit the same release-controller branch. The owner's normal process controls mainline merges and source release publication.
