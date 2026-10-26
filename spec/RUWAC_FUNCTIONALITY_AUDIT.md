# RUWAC purpose, functionality and implementation audit

Review started: 2026-10-17; historical change review below covers v0.4.18.
Current behavior was reconciled on 2026-10-20 against v0.4.31 and the
stabilization candidate's explicit SSH review and update-readiness changes. This is a product requirements ledger and source review, not a claim
that all supported environments have passed acceptance.

Current evidence reconciliation (2026-10-25): v0.4.43 passed nineteen frozen
categories, nine public-asset checks and twelve actual-public runtime phases.
Native GNOME Shell 50 mouse-selection delivery is now implemented through an
optional companion with explicit account permission and connection approval;
isolated managed/Unicode/ownership/startup and package checks passed. Actual
cross-machine application, physical input, Windows, endurance and broader
distribution/security qualification remain partial. Specific owner confirmations
and retained intermittent observations are recorded in the
[current acceptance status](RUWAC_ACCEPTANCE_STATUS.md). Historical findings below
remain evidence of their dated scope, not declarations that later fixes are absent
or that every requirement is complete.

## Purpose

RUWAC is a remote workspace and agent cockpit: quickly reach machines,
SSH terminals, persistent terminal sessions, remote desktops and coding agents
from one native desktop. Work should survive detachment; the headless daemon
and CLI must remain useful without GTK. The desktop should use little space,
preserve focus and clipboard ownership, and make connection state obvious.

The priorities expressed during development are fast daily work, Screen-like
terminal operation, reliable Windows/Fedora RDP, reusable machine bookmarks,
separate credentials, convenient agent sessions and straightforward Fedora
installation and updates. macOS and Windows clients are prospective ports,
not currently delivered products.

## Reading the results

**Implemented** means the corresponding code exists with recorded automated
or owner-observed coverage; it does not mean universal interoperability.
**Partial** means a material part of the request remains open.
**Deferred** means no delivered implementation is evidenced. Historical fixture
results are distinguished from tests re-executed for the current batch in the
[acceptance status](RUWAC_ACCEPTANCE_STATUS.md).

| Requested capability | State | Realized behavior and remaining scope |
| --- | --- | --- |
| Independent Rust daemon and headless CLI | Implemented | Persistent PTYs, authenticated private control socket, canonical terminal state; GTK dependencies stay in the desktop workspace. |
| Short create, list, attach and kill commands | Implemented | `-S`, `-r`, `-ls`, exact session IDs and scoped `kill-session`; ambiguous names are refused. |
| Several clients attached to one session | Implemented | Client-local focus, stable windows/panes and shared terminal size policy. Different clients cannot simultaneously give one PTY different kernel sizes. |
| Persistence across frontend close | Implemented | Closing a view detaches; daemon-owned processes continue. |
| Persistence across reboot/daemon loss | Partial | Saved history/layout can recover; terminated operating-system processes cannot be resurrected. Restored panes can start a new shell. |
| Screen-compatible navigation and aliases | Partial | Common create/attach/detach, previous/next/window chooser, splits, zoom, selection/search and paste; not the entire GNU Screen command/configuration language. |
| Nesting RUWAC, Screen and tmux | Implemented | Prefix forwarding, independent focus and mouse routing have fixtures. A known upstream Screen paste interaction is separately documented. |
| TUI status bar and window titles | Implemented | Clickable window bar, split commands, clipped/Unicode labels, foreground command names and a light-blue bar; custom names remain available. |
| Native GTK/libadwaita desktop | Implemented | Tabs, multiple windows, split views, compact actions, theme/font choices and optional dock. |
| Saved window geometry and workspaces | Implemented | Normal size, maximized/fullscreen state and saved mixed layouts; reopening a workspace can reconnect RDP rather than preserve its transport. |
| RDP lifetime across frontend replacement | Accepted boundary | The owner accepts reconnecting RDP after reopening. Terminal processes persist; seamless cross-process RDP transport preservation is not a current completion requirement. |
| Shell startup, exit and reconnect | Implemented | Normal attached exit removes its completed window, including the final session. Explicit retained/restored history can be removed with `-wipe`; live processes are preserved. Saved SSH reconnect controls also cover children that ended before the frontend opened. |
| Host identification in terminal tabs | Partial | Generic terminal tabs show the local daemon computer; saved remote/agent titles retain their host labels. Arbitrary manually typed or nested SSH is not reliably inferred. |
| Keyboard tab shortcuts and fullscreen | Implemented | Alt+1–0 selects desktop tabs by default; AltGr toggles Map keys; Ctrl+Shift+T/R creates terminal/chooses RDP; Ctrl+Alt+Enter toggles fullscreen. |
| Installed command manual and app discovery | Implemented | `man ruwac`, dedicated app icon and launcher keywords in per-user and RPM installs; the manual describes current limitations. |
| Fast Machines/Agents access | Implemented | Alt+M focuses machine search; Alt+A opens saved/running agents, including with collapsed header actions. Further keyboard-first bookmark refinement remains useful. |
| RDP tab actions and visible connection state | Implemented | Context controls, reconnect overlay, loading/status indicators, host-only titles; current change adds color plus distinct shapes. |
| Embedded authenticated RDP framebuffer | Partial | IronRDP lifecycle, decoded frames and targeted authorized Windows/Fedora journeys exist; broad server/version interoperability is not complete. |
| Dynamic resolution and viewport controls | Partial | Dynamic/Fit/Native/Stretch, zoom/fullscreen and initial/reconnect size fixtures; Windows Display Control and external application placement remain separate gates. |
| Prevent spurious RDP resize/drag jumps | Partial | Overlay chrome and pointer-release/remapping fixes have fixture coverage; every external desktop/window-manager combination is not verified. |
| RDP clipboard text and PNG/DIB | Partial | Real GDK/CLIPRDR, consecutive transfers, cancellation and focus epochs implemented; real Fedora journey recorded, broader Windows and mixed-external-host coverage remains open. |
| Three RDP tabs plus terminals clipboard ownership | Partial | Loopback multi-tab ownership tests exist; three real external hosts and physical focus/clipboard-manager timing remain separate. |
| Terminal desktop copy/paste | Implemented | Selection, Shift+drag, PRIMARY/middle paste and image staging; owner confirmed specific scrolling, tab-selection and fresh-shell image journeys. Optional native GNOME Shell 50 PRIMARY delivery requires desktop permission and per-connection approval; full physical/external coverage remains incomplete. |
| GNOME service and Linux user credentials | Implemented | Distinct usernames/references, independent Set/Replace/Forget and compatible profile metadata. Passwords remain in Secret Service. |
| Assisted and automatic GNOME sign-in | Partial | Explicit assisted input and opt-in recognized English GDM flow, input shielding and cancellation; not a general unattended-login protocol or system-wide autologin change. |
| Machine/profile editor and Save & Connect | Implemented | Basic fields grouped first, advanced sections, endpoint identity, CRUD, favorites, search, groups and SQLite/JSON compatibility. |
| Collapsible, scrollable groups | Implemented | Unpaginated desktop list, persisted group expansion and search reveal; list virtualization/very large inventory profiling is not established. |
| Remmina import | Implemented | Default native/legacy/Flatpak discovery, bulk import, duplicate review and optional file chooser; reviewed Secret Service password copying preserves existing credentials. Source files remain read-only; certificate bypasses and private keys are excluded. |
| Separate SSH identity and authentication | Implemented | The candidate lists eligible machines before contacting them, checks only on an explicit action, and requires selection plus fresh confirmation before adding a separate SSH endpoint. Strict public-key checks identify accepted fingerprints where available; unknown/changed trust and failed verification are explicit. Current-user/config defaults, explicit username/key selection, saved password/passphrase references, host-key review and confirmed public-key authorization. |
| SSH routing | Partial | Jump aliases use OpenSSH configuration. Implicit image channels reuse the original authenticated connection; file recovery verifies the original server before authentication, and additional rsync channels require that same key. General RDP gateways and arbitrary gateway management are not implemented. |
| Integrated SFTP file panel | Partial | Upload/download, integrity, progress, cancel and resumable transfer work; recursive/rsync and clean-environment qualification retain separate limits. Share enumeration does not constitute an integrated SMB file client. |
| Codex/Claude/Kimi host actions | Implemented | Saved commands, compact shortcuts, header/context menus, recent SSH, RUWAC/Screen/tmux/direct backends; generated launch tests are not proof of every provider account. Other providers, including Grok, remain future work. |
| Agent installation and shared sessions | Implemented | Explicit opt-in provider/bootstrap installation, remote RUWAC action and named agent windows sharing a session. No automatic provider authentication. |
| PNG attachment to local/remote agent | Partial | Owner confirms useful local/remote image paste. Explicit draft/staging and SSH paths exist; implicit delivery is now bound to the original pane with cancellable bounded I/O. New saved launches retain explicit daemon/pane/endpoint/provider associations across frontend replacement. Legacy sessions without metadata and broader provider acceptance retain their limits. |
| Job/activity and workspace automation | Partial | Job queries, events and permission-controlled workspace/RDP APIs exist; comprehensive resource-exhaustion and activity classification acceptance remain incomplete. |
| Fedora installation, updates and RPMs | Partial | GitHub source, testing bundle/RPM delivery, provenance checks, rollback and optional update timers exist; Fedora repository inclusion, clean-machine DNF and distribution qualification remain open. |
| Public privacy, license and provenance | Partial | Apache-2.0 original source, dependency notices, scanners and publication inventory exist. Historical privacy cleanup still needs owner decisions; RSA advisory remains unsuppressed. |
| About, support and donations | Partial | Author attribution, QR and one dedicated optional-name/email bank handoff page delivered. Payment confirmation/invoice integration through the invoice API is deferred. |
| macOS/native Windows builds | Deferred | Linux-specific IPC, credentials, process handling, packaging and updates need adapters and real platform validation. |

Capability states are not an acceptance percentage. The accepted RDP lifetime boundary is distinct from a deferred feature. A partly qualified complex protocol is not equivalent to a completed small UI action.

## Recent change review

The reviewed interval contains 51 changed files and approximately 3,500 added
lines. Commit authorship is not evidence of correctness. The following findings
are tied to code and observable behavior.

| Finding | Impact | Disposition |
| --- | --- | --- |
| v0.4.17 redirected Alt+digits away from desktop tabs | Regressed the previously requested default; v0.4.18 restored it with Map keys opt-in. | Keep v0.4.18 default; test menu chords in both modes. |
| Recent release notes still described the old split-width correction | New release descriptions omitted the intervening changes because the workflow publishes RELEASE-NOTES.md verbatim. | Updated current notes; historical release descriptions are unchanged. |
| New terminal tooltip changes invalidated exact-text GUI fixture actions | The real button still worked, but the login-shell regression could no longer click it; its tooltip also lost the action name. | Added a stable action widget identity, retained the tooltip action name and made fixture matching accept multiline tooltips. |
| Broad GUI regression still expected preference schema 2 after the schema 3 spacing migration | A valid saved preference file failed the old assertion; font size and collapsed groups were correctly persisted. | Updated the regression to assert schema 3 while retaining checks for the actual saved settings. |
| Broad GUI regression sent shell exit through an external control RPC | Recent GUI behavior closes locally interacted shells but intentionally retains externally ended output with a reconnect card; the old test bypassed local input tracking. | Exercise exit through the selected GTK terminal key path, keeping daemon/history assertions. |
| Machine-action geometry test sampled a newly rebuilt row before GTK allocation | Historical automatic SSH discovery could rebuild rows after the initial fixed sleep. The candidate removes that path; explicit inventory changes can still require waiting for GTK allocation. | Wait for all five controls to receive real allocation, then retain the existing alignment assertions. |
| Shortcut help/comments still described a timed AltGr leader | Help contradicted the actual toggle implementation. | Corrected in this batch. |
| Production tab timer called `fixture_inspect()` and `metadata()` every 100 ms per RDP tab | Allocated cadence/metrics JSON, locked connection state repeatedly and checked saved profile freshness unnecessarily. | Replaced with a small production summary; fixture inspection remains test-only. |
| SSH key probe allowed configured connection multiplexing | Reusing a password-authenticated master could incorrectly count as proof of key login. | Disabled ControlMaster/ControlPath/ControlPersist for verification probes; ordinary terminal connections retain their configuration. |
| TUI exit cleanup removed the last ended pane, then queried its deleted session | Normal shell exit returned STALE_ID and erased reattachable history; the existing real-PTY exit regression caught it. | Removed destructive cleanup: exit the view cleanly or select a live sibling, preserving ended history. |
| Same-content comparisons omit daemon-global render revision | Correctly avoids unrelated-session full redraw/flicker. | Retained; continued nested/idle regression coverage is required. |
| Hidden/idle terminal polling backs off and commands wake it | Reduces idle CPU while retaining input response. | Retained; measure rather than add another polling subsystem. |
| Remote image upload writes all stdin before starting its 60-second wait | A peer that stops reading can block the worker before the stated deadline. | Fixed: nonblocking full-duplex upload, bounded stdout/stderr, a single deadline and owned process-group cleanup. |
| SSH image upload completion calls `view.paste()` without a captured pane/focus generation | A pane switch during transfer may paste the path into an unintended pane. Saved in-memory target maps also do not survive a new GUI process. | Fixed pane/focus and new-launch persistence: explicit private records survive frontend replacement, bind the original pane/PID and refuse changed endpoints. No association is inferred for legacy sessions. |
| Agent-session menu identifies agents by substrings in the name | Can misclassify an unrelated name or miss a custom shared name. | Fixed for saved launches: persist explicit SSH/Codex/Claude/Kimi kind; ordinary names do not classify an agent. |
| Agent selection changes daemon focus but leaves the GUI target on a local split | The new real-PTY gate fails against the published source increment. | Correct with acknowledged GUI navigation, queued typing and intent epochs so a later manual pane selection wins; retain the failed source reference and require the corrected native gate. |
| PTY reaper emits end after a fixed 300 ms delay and reader may still emit later data | Late background output could change already-ended history; closed streams could report exit before the process finished. | Fixed: one reader drains ready output and emits end once, bounds continuous inherited output and supports closed/reopened streams. Owned real PTY/daemon cases and 128-pane descriptor-limited acceptance pass. |
| Color CSS targeted private tab nodes | Could fail silently across libadwaita versions. | Status now uses cached full-color icons with distinct shapes, not private CSS selectors. |

Font spacing review found that the previous “Compact” label selected 1.15× spacing, which increased rather than reduced spacing. The label is corrected, a 0.95× Tight choice is available and glyph cell widths retain fractional Pango precision. Existing saved font choices are preserved.

This batch intentionally keeps the immediate UI scope small. The SSH image focus and transport corrections follow the navigation batch; the
new-launch persistent association and provider classification are implemented; the PTY drain finding is now covered by the ordered-reader implementation and real PTY/daemon regressions.

## Remaining acceptance priorities

1. Retain broader saved-bookmark and active-session acceptance, including clear
   handling of legacy sessions that predate explicit launch metadata. Regress
   external OpenSSH configuration and trust changes: implicit image channels
   must stay on their original transport and pending file work must refuse a
   different server before authentication.
2. Exercise saved-bookmark navigation and active-session choice with realistic
   inventories; add explicit launch metadata rather than infer provider type.
3. Validate Windows dynamic resizing/clipboard and mixed external hosts, followed
   by physical Wayland keyboard, touchpad and clipboard-manager journeys.
4. Qualify clean Fedora RPM installation/update and resolve upstream dependency
   security findings before claiming complete distribution readiness.
5. Preserve terminal PIDs and workspace identity during frontend replacement;
   verify honest RDP reconnection status. Reconnecting after reopening is accepted,
   so seamless transport handoff is not an outstanding requirement.

Detailed real-host evidence remains private. See also
[product completion](RUWAC_PRODUCT_COMPLETION.md),
[terminal connection acceptance](RUWAC_TERMINAL_CONNECTION_ACCEPTANCE.md),
[dependency security](RUWAC_DEPENDENCY_SECURITY.md) and
[updates](RUWAC_UPDATES.md).
