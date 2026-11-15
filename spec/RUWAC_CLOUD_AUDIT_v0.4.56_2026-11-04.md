# RUWAC Linux source and documentation audit — v0.4.56 (2026-11-04)

**Basis:** read-only inspection of public GitHub source and workflows at commit `a4be3f60dc12667e2e3bf5c727ba1b8b0ae7de7c`. Source manifest `ruwac/Cargo.toml` declares version `0.4.56`. The `v0.4.56` GitHub Release was published on 2026-11-03 at 23:57:45 UTC. For the exact v0.4.56 commit, CI quality and privacy jobs completed successfully; the dependency-advisory job failed on the known desktop RSA finding. Publishing source and passing quality/privacy do not equal full security qualification. This is a **source/document audit, not a new compiled, hardware or external-machine acceptance**.

## Executive assessment

The repository provides an unusually complete set of subsystem specs, architecture notes, CLI/native/RDP acceptance scripts and release gates. It does **not yet provide one current, explicit, prioritized master plan** suitable for a GitHub-only contributor. Several public ledgers still describe earlier versions as future candidates. The review therefore recommends a cloud entry point, new status ledger and small, isolated PRs rather than a large rewrite.

The product is usable and its main differentiators should be preserved: Screen-style persistent terminals, native GTK workspaces, SSH/RDP, text/PNG attachment to coding-agent drafts and original-SSH-identity-bound Web Preview.

## Verified source changes since v0.4.52

| Source version | Verified change | Scope and caveat |
| --- | --- | --- |
| v0.4.53 | `ruwac/crates/ruwac-core/src/viewer.rs`; `attach --viewer` / `-O`; terminal resize exclusion and tests | Non-sizing CLI/SSH *observer role* is implemented. It is not read-only, and ordinary sizing clients and GTK still use the prior minimum-size policy. No controller lease, explicit take-over or local pan/fit. |
| v0.4.54 | Claude Code PNG draft attachment and source-based Fedora/COPR packaging | The desktop recognizes supported Claude inputs and uses explicit draft semantics; the distribution spec builds from source. Runtime external-provider approval is narrower than source feature existence. |
| v0.4.55 | Fedora remote agent auto-install/update from COPR; Copy machine action moved to properties dialog | Added DNF/COPR path and no-duplicate-install design; existing daemons stay running. |
| v0.4.56 | `ruwac/desktop/src/agent_launch.rs`: refresh DNF check/upgrade and restrict repositories to Fedora and selected COPR | Fixes stale metadata and risks from unrelated enabled third-party repos. The implementation is shell-generated and has synthetic stub checks. Verify actual supported Fedora and COPR behavior before claiming fleet-wide acceptance. |

The owner expressly **accepts current mouse-selection semantics**: normal shell mouse drag selects; when the foreground app requests mouse events, Shift+drag is RUWAC's override. **Do not add an alternate mouse-selection-priority feature or change this mapping as part of the next cycle.**

## Remote web preview: already implemented, usability still improvable

**Existing behavior** (read `ruwac/desktop/src/web.rs`, `web_route.rs`, `ssh_transport.rs` and the README Web Preview section):

- A user opens a saved, authenticated RUWAC SSH terminal for the intended machine. From that terminal's displayed `http://localhost:PORT/` URL, Ctrl+click launches WebKitGTK inside a RUWAC tab. With application mouse capture, Shift+Ctrl+click deliberately overrides.
- RUWAC binds the preview to the original session, pane, process, saved endpoint and verified OpenSSH master. The implementation requests an ephemeral **local-loopback-only** forward and maps the URL through it; no manual ssh -L command is needed.
- Its tests cover generated OpenSSH masters, real loopback HTTP/WebSocket fixtures, namespace separation, redirects, close/restore cleanup and bounds. Those are fixtures, not a physical remote-host guarantee.
- A generic blank Web Preview is not automatically associated with a remote machine. It must not reinterpret workstation localhost as remote localhost.
- The current UX lacks a simple **Machines → Open remote web service** flow to choose a trusted host and its localhost port, reusing that exact live SSH binding without requiring a clickable link in terminal text.
- Navigation is guarded at WebKit's top-level origin policy; any future broader network policy must separately examine subresource fetches, CSP, WebSockets and redirects. Do not claim the browser fully sandboxes all network requests.

**Recommendation:** keep the existing tunnel as-is; propose a small optional UI action on an **already established, authorized** SSH connection. Do not silently connect to another endpoint, bypass host-key checks, forward intranet addresses, auto-open a port on workspace restore or recreate dead master transports.

## Terminal latency: plausible hypothesis, not yet a demonstrated regression

Source read: `ruwac/desktop/src/worker.rs` and `ruwac/crates/ruwac-tui/src/lib.rs`.

- GTK terminal worker schedules visible output polls around 8 ms just after typing, 20 ms for recent activity, 100 ms in quiet/visible state and 250 ms for hidden views. The attached TUI similarly uses 8 ms shortly after input and 25 ms otherwise.
- There is a daemon-wide revision channel and a per-client projection stamp to suppress unnecessary full renders for output confined to hidden panes. The v0.4.51 public acceptance document reports a controlled improvement from 4.65 MB/s to 3.4 KB/s in a 19-pane/four-client fixture, **not** end-to-end typing latency.
- Remote nested configurations can add an SSH round trip, PTY, RUWAC client, daemon JSON serialization and a second render scheduler. A comparison with Screen/tmux is not established by API benchmarks.
- A future event-driven delta/notification protocol may help, but needs targeted measurements and old-daemon fallback before implementation.

**Recommendation:** add deterministic instrumentation for key admission, child-PTY echo/output, daemon revision, frontend-frame receipt and paint scheduling. Compare RUWAC and Screen/tmux on identical disposable SSH/PTY topologies, reporting p50/p95/p99, CPU and bytes. Do not blindly replace polling intervals with busy loops.

## Screen/tmux coverage versus product scope

Read `README.md` (Screen aliases), `ruwac/crates/ruwac-cli/src/main.rs` and `ruwac/crates/ruwac-tui/src/lib.rs`:

Implemented: session create/reattach/list/detach, windows, next/previous/other, splits, focus/zoom, bounded history/copy/search and selected Screen-like `-X` aliases. Different clients can have independent focus. RUWAC also exposes typed JSON RPC and machine/agent/browser integration.

Not full equivalents: complete `.screenrc` or `tmux.conf` interpretation, arbitrary user command mappings, Screen logging, cross-user ACLs, tmux pane move/join/break, full hooks and generic synchronized input. The absence of full command parity is **not currently a correctness defect**. Add a few features only with a real workflow and tests.

Potentially useful later: background terminal activity/silence notifications (distinct from truthful agent completion). Not a reason to expand scope ahead of latency and GUI view ownership.

## Explicit public-documentation gaps

| Document | Current limitation observed | Recommended treatment |
| --- | --- | --- |
| `spec/RUWAC_ACCEPTANCE_STATUS.md` | Top delivered reconciliation remains v0.4.52; later v0.4.53 viewer is called an unreleased candidate despite later tagged releases. | Add a **dated v0.4.56 source / v0.4.55 release reconciliation** from exact evidence. Do not invent unexecuted tests or edit historic receipts. |
| `spec/RUWAC_FUNCTIONALITY_AUDIT.md` | Lead status also says v0.4.52 released and v0.4.53 viewer candidate. | Append a concise new version-status header. |
| `spec/RUWAC_CURRENT_STATE.md` | Lead reconciliation is v0.4.40 followed by very large historical material. | Keep as historical; link to current index. |
| `spec/RUWAC_NEXT_IMPLEMENTATION.md` | Large historical chronological notes, not a short current sprint. | Preserve content; do not use as unchecked future task list. |
| `architecture/05-OPEN-ISSUES.md`, `06-IMPLEMENTATION-ROADMAP.md` | Earliest spike says no desktop or binary release and lists already implemented features as pending. | Prominently mark **2026-10 experiment / superseded by current index**, without rewriting preserved history. |
| `spec/RUWAC_AUDIT_RESPONSE_2026-10-29.md` | Describes 0.4.53 non-sizing viewer as a candidate; reflects earlier snapshot. | Preserve date and add cross-reference to updated review. |
| Issue [#2](https://github.com/w3fans/RUWAC/issues/2) | Web Preview was implemented, but the issue remains open with an initial unimplemented spec. | Add acceptance/status comment; keep open for confirmed remote-service UX/external qualification, or split scope after maintainer review. |
| Issue [#4](https://github.com/w3fans/RUWAC/issues/4) | Non-sizing CLI/SSH viewer is shipped, but GUI view mode and explicit resize controller remain open. | Comment with delivered partial scope; do not close the issue. |
| Root agent instructions | `CLAUDE.md`/`AGENTS.md` are intentionally forbidden by `scripts/public-files.py`. | Use reviewed `spec/*.md` handoff and GitHub issue/PR links, never a policy bypass. |

## Release and security

- The public source is pinned at v0.4.56. Never infer completed v0.4.56 publication just because the branch/commit exists; query the release tag and exact-sha Actions first.
- Current Fedora packaging has a source-built COPR path; user/remote package lifecycle still needs clean distribution-based upgrade and old-daemon/PATH qualification on real systems.
- The desktop graph's `rsa 0.10.0-rc.18` and `RUSTSEC-2023-0071` remain documented and unsuppressed. Historical analysis describes password-based RDP rather than configured smart-card private-key operations; that is **not proof of zero exploitability or clearance**. Do not suppress the advisory. Patch only through reviewed upstream dependency changes and intact RDP tests.
- A failed dependency-advisory job does not mean quality/privacy necessarily failed. Report each job individually by exact commit.
- Do not equate a released RPM with official Fedora distribution acceptance, or a GitHub-origin RPM with a DNF-managed COPR build.

## Work that a cloud-only agent can and cannot certify

**Possible via GitHub PR and hosted CI:** source/document reconciliation, API-level unit tests, isolated PTY regression tests, deterministic latency harness, bounded UI integration implementation for later local qualification, quality-workflow coverage and feature-graph/security analysis.

**Requires owner's workstation, physical or external setup:** actual saved machine profiles, original SSH master into a real remote service, Wayland physical selection/clipboard manager, real Windows/Fedora RDP interoperability, production COPR publishing credentials, live agent-provider sessions, session-preserving desktop installation upgrades and high-load soak using authorized hosts.

## Source audit limitations

This audit performed no edits to source, no new Rust compilation, no manual workstation tests, no network connection to the maintainer's machines and no live agent-session control. GitHub fixtures and past acceptance reports are cited by path and test scope. A code review can identify an unverified edge case but must not report it as a reproduced defect.

**Next:** [Cloud work plan](RUWAC_CLOUD_WORK_PLAN_2026-11-04.md) for PR-sized tasks, acceptance and release boundaries.
