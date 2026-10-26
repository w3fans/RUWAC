# RUWAC acceptance status

Public results summarize qualification without publishing host identities, accounts,
network topology, credential references or operational troubleshooting. Detailed
receipts and original reports are archived privately outside Git.

## Delivered v0.4.43 — reconciled 2026-10-25

Commit `1d49ae20a1e0b42be3312171fe9ee50da513c012` passed all nineteen
required frozen categories, integrity/provenance checks for all nine public assets,
and twelve runtime phases on actual published binaries. The runtime phases cover
headless operation, sessions, native clipboard/tab/reconnect behavior, PRIMARY
epochs, original-endpoint routing, packaging reproducibility, terminal ergonomics,
Web Preview, SSH Web Preview, RDP clipboard ordering, new-terminal desktop
environment and image paste. Qualified installations preserved configurations and
existing GUI, daemon and terminal process identities; GUIs need reopening to load
the installed update. These results qualify the scoped release, not the whole
product requirements ledger.

The optional native GNOME Shell 50 selection companion additionally passed managed
installation (28 checks), Unicode (21), ownership guards (33), startup failures
(11) and standalone package/schema checks against the final local and actual
published binaries. Counts overlap. This uses isolated compositors, generated
peers, authenticated loopback SSH/RDP and explicit remote GTK input relays. It does
not prove the physical or external-host clipboard matrix. Account publication
permission and per-connection approval remain explicit; installation does not
enable them automatically.

Owner feedback confirms scrolling, terminal-tab selection paste and automatic
GNOME sign-in after reopening v0.4.40, and screenshot paste into Codex in a fresh
desktop-launched v0.4.42 shell. These are specific journeys, not blanket acceptance.
One isolated native Ctrl+V run received no payload; the unchanged focused retry and
actual-public suite passed. The original failure remains retained with cause
unresolved. An earlier prototype SessionMode restoration refusal also remains
tracked; later successful isolated cycles do not establish its cause.

Physical input/clipboard-manager timing, actual cross-machine application PRIMARY,
external Windows interoperability, controlled recovery cycles, the segmented
one-hour soak and the observed development workflow remain open. The RSA advisory
is unsuppressed. Hosted quality, privacy and release-build jobs passed for this
commit; the advisory job failed separately. See the
[v0.4.43 release](https://github.com/w3fans/RUWAC/releases/tag/v0.4.43).

The older sections below retain their original evidence scope. References to an
unreleased epoch or native-selection candidate are historical and do not undo
later release receipts; they also cannot qualify later changed code.

## Released v0.4.40 baseline — reconciled 2026-10-24

Commit `5b62c3404397bd7591e5f5c9351a77ff96178619` completed all nineteen
required frozen acceptance categories. All nine published assets passed integrity,
provenance and installation qualification; nine actual-public runtime phases
passed, including terminal ergonomics, RDP clipboard ordering and native Web Preview
with original-SSH routing. The eight terminal ergonomics cases cover full-screen
wheel up/down, click-only mouse mode and offset splits as well as clipboard/SIGINT
and inline history. These are generated native input and authenticated fixtures,
not a physical keyboard/touchpad or external-provider conversation pass.

Actual owner external GNOME sign-in, Windows interoperability, physical Wayland
input, clipboard-manager persistence and generic cross-host Wayland PRIMARY
injection remain unqualified. The RSA advisory remains unresolved; see
[dependency security](RUWAC_DEPENDENCY_SECURITY.md). Reopening after installation
loads the new GUI while preserving daemon-owned terminal processes; reconnecting
RDP after reopening is an accepted boundary.

New changes after this baseline require their own frozen acceptance and public
artifact qualification. In particular, the current remote-selection epoch fix is
development work, not part of the v0.4.40 receipt.

## Historical prepublication notes and earlier evidence

The notes below retain what was known before v0.4.40's final release. References
to a changed candidate or a run still required are historical and do not undo the
completed baseline above. They also cannot qualify a later changed candidate.

Prior published baseline (2026-10-23): v0.4.39, commit
`8e792df38731a1b6c11a9896f965ca33de4023b2`, completed all 16 required frozen
local gate categories, retained 72 runtime/artifact groups, and verified all nine
published assets and their install/runtime qualification. This is scoped operational
qualification, including generated/authenticated loopback input. Physical cross-host
selection paste, real clipboard-manager persistence and the reported external GNOME
login remain unconfirmed. The advisory job reported three vulnerabilities on that
release; see the dependency security ledger. Its source and installed artifacts are
available on the [v0.4.39 release page](https://github.com/w3fans/RUWAC/releases/tag/v0.4.39).

The terminal/clipboard/Web Preview development candidate has changed beyond that
release. Historical evidence below does not qualify it: a new complete frozen run
and the additional interaction/browser checks are required before publication.
The current gate adds `terminal-desktop-ergonomics`, `rdp-clipboard-ordering`
and `web-preview` to the historical requirements. These require executed
keyboard/inline-scroll, clipboard event-order/backpressure, and native browser
plus original-SSH preview evidence. Current preflight includes 194 desktop
unit/transport tests, native terminal key/scroll checks, and real WebKit/OpenSSH
HTTP/WebSocket lifecycle checks. A complete source/binary-frozen run remains
mandatory; passing preflight or older GNOME reconnection checks cannot substitute
for that run. The release adapter refuses publication without all nineteen gates.

Native terminal preflight also reproduces and fixes dropped wheel events in a
full-screen mouse-reporting PTY application: wheel up/down, click-only mouse mode
and a right-hand mixed split require exact widget-local coordinates. The failing
baseline received pointer motion but no wheel input. The corrected eight-check
terminal suite passes; this is generated application input, not an authenticated
provider conversation or physical touchpad acceptance.

The Web fixtures use an owned Mutter compositor, native GDK keyboard/pointer
callbacks, real WebKit pages and generated OpenSSH identities. Two generated SSH
servers on one fixture host advertise the same loopback port through a test-only
process-local port translation. This proves connection/origin isolation, not a
separate-machine or physical-input result. Checks cover browser/terminal
clipboard and Ctrl+L isolation, mixed splits, ephemeral storage, explicit
restoration, changed-profile cancellation and stale-forward refusal. Closing a
preview preserves its original SSH PTY. Physical clipboard-manager persistence,
external HTTPS development servers and the complete physical-input matrix remain
separate qualification limits.

Stabilization reconciliation (2026-10-20): this table retains historical evidence
through v0.4.31; historical PASS rows do not qualify the changed candidate.
Changed candidates require a complete final source/binary-frozen stabilization
run. Historical rows alone leave that candidate run **NOT RUN**; the project
release adapter requires its current executed receipt before publication.
Unexecuted preflight work is not a pass. External Windows, physical Wayland input and live provider
acceptance remain **BLOCKED** until the corresponding authorized resource and
executed evidence are available. Generated loopback controls cannot satisfy them.
RDP reconnection after GUI reopening is explicitly accepted; cross-process RDP
transport preservation is not a completion requirement.

The project release adapter requires a configured frozen acceptance runner and
its receipt before publication. `scripts/acceptance-gate.py` checks every required
technical gate, hashes of the current source and compiled applications, executed
evidence hashes, and the staged source again before commit. Missing, failed,
blocked or preflight-only required gates hold publication. External/physical
resource gates remain separate and can keep full product acceptance incomplete.
Hosted source publication also waits for exact-commit quality/privacy checks;
dependency advisories remain separate and unsuppressed.

| Area | Result | Scope |
| --- | --- | --- |
| SSH terminal image-paste delivery | NOT RUN (final stabilization candidate) | Thirteen existing real SSH/GDK delivery and cancellation groups remain covered. Private launch preflight adds saved-action capture, frontend restart, local-split isolation, real ended-connection Reconnect and daemon replacement: 18 groups each for agent, plain SSH and agent with an older engine. Synthetic key-event probes establish actual owned-seat focus before claiming clipboard or testing deactivation. Owner-confirmed provider use remains separate from generated receivers. |
| Persistent SSH/agent launch identities | NOT RUN (final stabilization candidate) | Six storage/recipe units and three 18-group native journeys cover daemon-bound private records, original pane/PID, explicit provider metadata, real SSH, restart, reconstructed reconnect and replaced engines. The final frozen suite additionally exercises selecting the saved agent pane from a local split and the worker's membership refusal test. No retroactive binding of legacy sessions or external OpenSSH-configuration tracking is claimed. |
| Quick host menus, tab identity and status | PASS | Ten preflight native GTK/Wayland groups, including actual synthetic-seat Alt+M/Alt+A in both Map keys modes with collapsed actions, hostname tab titles and cached full-colour indicators. Final stamped qualification repeats these groups with an added idle-redraw guard. |
| Installation discovery and manual | PASS | 28 generated bundle/installer contracts, owned legacy-launcher migration, resource collisions and real `man` rendering; final RPM payload/lifecycle checks verify installed manual and app icon. |
| Explicit SSH key-access review | NOT RUN (final stabilization candidate) | Implicit startup/periodic probes and endpoint creation have been removed in the candidate. The explicit review requires Check, selection, confirmation and fresh source/trust/key inputs. Current OpenSSH and native GUI qualification is required; prior automatic-discovery evidence does not qualify this replacement. |
| Compact machine and saved-host agent actions | PASS | Eight real GTK/Wayland groups: one-row SSH/Codex/Claude/Kimi actions, explicit separate SSH setup, cancellation, saved-host/recent menus, install/key-authorization confirmations, real synthetic-seat keyboard chords, close/reopen geometry and session identities, and separator pixels after pane repaint. No external connection or key grant in this fixture. |
| Agent installation bootstrap | PASS | Nine generated download/availability/failure contracts and real official Codex, Claude and Kimi installation in separate disposable user homes, each followed only by `--version`. No provider login, model request or external-host deployment claimed. |
| CLI split dividers and status colour | PASS | Real kernel PTYs and nested RUWAC rendering, horizontal/vertical divider glyphs, intact application corners and canonical light-blue status colours. New engines reserve divider cells; preserved older engines keep their original layout. |
| Exact session identities and scoped closure | PASS | Ten actual CLI usability groups including repeated GUI-style names, exact-ID attachment, optional explicit session flag, ambiguity refusal and closure of only the selected generated session. |
| CLI/TUI login-shell compatibility | PASS | Eighteen actual startup groups against current and isolated older daemons, real Bash profile loading, named/detached/Screen creation, TUI window/split commands, unchanged custom argv, retained reattach PID and authenticated loopback SSH endpoint-local shell resolution. Ten synthetic discovery cases; no external-host claim. |
| Natural terminal grid bounds | PASS | Real kernel PTYs up to the 262144-cell boundary; large coloured replies, nonmutating invalid-size refusals, TUI resizing and shared-client sizing; larger checkpoint recovery and previous-reader refusal without file changes. Existing older daemons keep their limits. |
| Native large terminal canvas | PASS | Actual GTK/Wayland on an owned 4K virtual compositor: exact requested window geometry, allocation-to-grid-to-kernel agreement, all four rendered corners at 400×103 cells, repeated resizing and same child process. No physical desktop claim. |
| Clipped CLI status controls | PASS | Shared width-aware drawing and hit targets; actual PTY clicks on clipped Unicode labels and hidden-window arrows with current/older daemons; mouse-reporting application receipts distinguish local bar clicks from forwarded application events. |
| Split-pane exit collapse | PASS | Twelve real PTY/native GTK groups against current and isolated older daemons: two concurrent TUIs, GTK-only cases, full viewport, repaired focus, exact input bytes, nested survivors and attachment after exit. Completed split history follows ordinary pane-close semantics. Normal attached final exit now removes its completed session; explicit restored or previously ended history remains available until `-wipe`. |
| Terminal shell startup and exit | PASS | Real PTYs, native GTK and an isolated older daemon: explicit login-shell argv in new tabs/windows/splits, real Bash profile/rc, live-window fallback, acknowledged final-window/session removal, safe dead-window cleanup and explicit retained history |
| GTK Ctrl+A routing | PASS | Real tmux configured with Ctrl+A receives a single prefix; local RUWAC GTK prefix is Ctrl+Alt+A; CLI prefix remains Ctrl+A |
| CLI status controls | PASS | Actual bottom-row SGR clicks select windows; lowercase horizontal and vertical split shortcuts create persistent panes; nine CLI regression groups |
| Initial Dynamic RDP framebuffer | PASS | Native Save & Connect at three window sizes, actual Secret Service and loopback CredSSP, decoded-size traces exclude fallback frames; queued cancellation before worker startup and background connection preserve selection, clipboard and neighboring transports. External desktop placement remains separate. |
| Reconnect framebuffer dimensions | PASS | Initial client-size-aware authenticated loopback peer, three reconnects at 1400×868, bounded trace of decoded sizes excludes transient smaller frames after settling; external Windows window-placement remains unverified |
| Saved-profile reconnect controls | PASS | Four mapped editor/Save & Connect cycles, centered disconnected overlay without viewport change, targeted tab reconnect and preserved neighbor transports; synthetic unmapped-dialog GTK limitation remains documented |
| About and donation link | PASS | Native dialog construction; independently decoded static QR points to dedicated public support page; no payment submission, invoicing or payment credentials in RUWAC |

| Scrollable, collapsible machine groups | PASS | 123 generated profiles without pagination, native disclosure, search reveal/restore, preference preservation and restart persistence |
| RDP drag release and viewport stability | PASS | Actual authenticated loopback left/right release receipts, GTK gesture cancellation, tab-switch release, zoom remapping and preserved pan; synthetic input, external physical reproduction remains separate |
| Bulk Remmina metadata import | PASS | Generated native/legacy/Flatpak profiles; confirmation, cancellation, changed-file refusal, duplicate skipping and credential exclusion |
| Dynamic RDP and local host shortcuts | PASS | Owned Mutter 50.5, twenty Display Control reactivations and allocation-driven resize storm; real GDK Ctrl+Alt+Home and Ctrl+Alt+Enter events release focus and enter/exit fullscreen without reconnecting |
| Header Controls and optional dock | PASS | 32px native header; popover preserves terminal/RDP allocation, explicit dock takes layout space; synthetic GTK callbacks |
| Screen CLI aliases and readable session list | PASS | Eight real PTY groups including full-width bottom status through normal and beyond-limit physical resizes, detached/shared sessions, automation title, control-key navigation and kill cancellation; full GNU Screen language remains unsupported |
| Desktop appearance and font metrics | PASS | Six actual rendered background pixel checks, saved schema-3 preferences and PTY dimensions changed by font size; unavailable fonts fall back to Monospace |
| Remote agent launch configuration | PASS | Native SSH profile saves commands/backend; generated command/quoting tests; no real agent account or external remote installation claimed |
| Persistent remote agent session reuse | PASS | Thirteen actual loopback SSH/PTY groups: concurrent creation, detach, same-process reattach, independent sessions, three named agent windows in one shared session, exact input isolation, ended-history retention, exit without input fallback, adding an agent to an existing session and duplicate/unsafe-lock refusals; generated agents only |
| Remote CLI installation and external agent launch | PARTIAL | Verified prebuilt installation and prerequisite repair are recorded on authorized Linux hosts. Authenticated external native provider journeys remain separate from generated receivers and installer version checks. |
| Weston popup-after-resize compatibility | BLOCKED | Weston 15.0.1 exited in popup positioning on two owned displays; Mutter qualification is recorded separately; no physical desktop pass inferred |
| Strict warnings-as-errors desktop lint | NOT QUALIFIED | Existing GTK deprecation/dead-code warnings remain. Ordinary hosted Clippy passes; it does not establish warnings-as-errors qualification. |
| Automatic GDM sign-in | PASS (selected Fedora journeys) | Recorded authorized login/handover journeys and owner confirmation cover selected hosts. General GDM variants and physical mouse/resize acceptance remain separate; no new real login is implied by a synthetic regression. |
| Named CLI creation/reattachment, window titles and concurrent clients | PASS | Actual generated local PTYs; exact-name and recursion safeguards; two simultaneous clients |
| Nested prefix routing | PASS | Two through five real RUWAC layers and native GTK with actual tmux; synthetic GTK callbacks |
| Remmina default discovery | PASS | Generated native/legacy/Flatpak profiles; read-only source and explicit metadata import; credentials excluded |
| Verified opt-in system RPM updates | PASS | Official signed workflow provenance, actual Fedora DNF, generated fault tests, native GTK controls and isolated system timer; physical authorization remains separate |
| Persistent terminal daemon, CLI, Screen navigation and history | PASS | Rust units and real generated PTY fixtures |
| Independent attached-client selection, zoom and input | PASS | Disposable daemon, two real TUI clients, generated raw receivers; shared PTY sizing, closed-target repair and scoped/global deduplication |
| Native GTK profiles, SQLite migration and credential separation | PASS | Generated profiles, disposable Secret Service, isolated compositor |
| RDP authentication, framebuffer, text/PNG clipboard and focus ownership | PASS | Authenticated IronRDP loopback fixtures with generated credentials |
| Fedora RDP sign-in and bidirectional text/PNG clipboard | PASS | Authorized Fedora RDP host; GNOME Remote Desktop 50.2 / GDM 50.3; simulated local input |
| Windows RDP authentication and framebuffer | PASS | Windows test host; single authenticated connection |
| RDP latency, CPU and memory comparison | PASS | Authenticated same-endpoint RUWAC/FreeRDP X11 fixture, ten alternating trials and fifty actual observed frames per client; external/physical and 3K scopes remain separate |
| Windows dynamic display and external clipboard matrix | BLOCKED | Qualification incomplete |
| Single-operation large native clipboard paste | PASS | Actual 1 MiB GDK transfer and daemon/PTy Unicode fixture; one bracket pair, staging/ownership/limits/expiry; synthetic GTK callback |
| Desktop clipboard deadlines and stalled-provider cancellation | PASS | Real GDK/GIO on owned Wayland; generated incomplete provider, kernel-stream recovery and partial-file cleanup |
| SSH/SFTP integrity, progress, cancellation and persistence | PASS | Isolated authorized fixtures; generated files and keys |
| Explicit SSH jump routing and separate gateway/target trust | PASS | Two disposable OpenSSH servers; eight actual transport groups and four native editor groups |
| Remote agent image transport/consumption | PASS | External CLI route and native loopback route; complete external native launcher remains unqualified |
| PRIMARY and native middle-click dispatch | PASS | Real GDK/Mutter protocol, generated text, synthetic click dispatch, SGR routing and Shift bypass; physical input remains separate |
| Physical GTK/Wayland mouse, keyboard and touchpad acceptance | PARTIAL | Owner confirms tab shortcuts and selected copy/paste workflows, including selection sharing. The complete current-version mouse, resize and touchpad matrix remains unqualified; automated input does not substitute for it. |
| Upstream Fedora testing RPM payloads and private transactions | PASS | Actual separate core/desktop packages, executable versions, launcher/notices and prior-release upgrade/removal; private RPM database uses host libraries and --nodeps |
| Clean Fedora desktop DNF install/upgrade/rollback/remove | PASS | Fresh pinned Fedora 44 RAM filesystem, actual published RPMs, real dependency resolution without --nodeps; daemon, PTY child and generated private configuration survive |
| RPM signing/repository and complete binary security qualification | BLOCKED | Workflow provenance is distinct from a signed Fedora repository; desktop RSA advisory remains open |
| Desktop rustls security patch | UPDATED | Lockfile pins upstream-patched 0.23.46 for RUSTSEC-2026-0346 through RUSTSEC-2026-0349; authenticated TLS, clipboard and certificate regressions are mandatory release gates. The independent RSA finding is not suppressed. |
| Desktop RSA dependency advisory | FAIL | RUSTSEC-2023-0071 remains unsuppressed |

These results describe the published implementation baseline. In-progress codec
experiments do not establish Windows resize acceptance or full stable qualification.

Reproduce portable tests after building into a separate build directory:

```sh
cargo test --locked --workspace --manifest-path ruwac/Cargo.toml
cargo test --locked --manifest-path ruwac/desktop/Cargo.toml
python3 ruwac/tests/integration/client_focus.py --binary "$CARGO_TARGET_DIR/release/ruwac" --evidence "$EVIDENCE_DIR"
python3 scripts/test-privacy-check.py
python3 scripts/public-files.py --tracked
python3 ruwac/tests/integration/test_gdm_recognition.py
python3 ruwac/tests/integration/test_updates.py
```

Native authenticated clipboard fixtures are driven by
`ruwac/tests/integration/rdp_clipboard.py --help`. They generate their own loopback
server, disposable credentials and isolated clipboard state. Never point synthetic
credential or destructive fixture tests at production accounts.

Build and publication use the existing `build.sh` and `release.sh` adapters; the
installed controller assigns versions. GitHub source archives contain source,
not a claim of qualified Fedora binaries. Historical references are audited
separately and remain unchanged pending owner approval.

Reproduce PRIMARY and click dispatch with test-only Mutter, D-Bus and Python GObject dependencies:

```sh
python3 ruwac/tests/integration/primary_wayland.py \
  --binary "$CARGO_TARGET_DIR/release/ruwac" \
  --gui "$CARGO_TARGET_DIR/release/ruwac-desktop" \
  --evidence "$EVIDENCE_DIR" --spawn-owned-display
```

The fixture creates and stops its own compositor and child processes. Its
`--owned-display` mode refuses the normal user desktop runtime and requires an
explicitly named test display. Synthetic callbacks qualify protocol and dispatch
behavior, not physical mouse or touchpad operation.

One combined RDP stress run timed out during reconnect and resize. Isolated and
repeated combined tests passed. The cause of that intermittent failure has not
been established; it remains recorded in private qualification evidence.


## Ordered terminal output and process exit

One reader owns all output publication and the end event. After observing the
owned process exit it drains ready bytes before publishing end; a continuous
inherited writer is bounded by an 8 MiB post-exit drain. Later background output
does not alter ended history. Temporary closure/reopening of standard streams
retains output and the actual exit code. Readiness uses a separately owned
close-on-exec descriptor, prepared before child creation.

Owned Linux PTY cases include delayed callbacks, final bursts through 1 MiB,
closed/reopened streams and continuous inherited writers. The real daemon/CLI
fixtures retain exit, live-sibling selection, ended-history reattachment,
independent focus and crash recovery. The actual 128-pane metadata fixture also
passes with a 1,024-descriptor limit. These are generated local processes, not
external-provider or physical-input acceptance. Existing engines are preserved
and require deliberate replacement before using this backend implementation.


## Server identity before file authentication — 2026-10-20 preflight

Two independently trusted generated OpenSSH servers reproduce alias redirection
between transfer preparation and recovery. The previous adapter authenticated
to the replacement before refusing its different key. Current preflight checks
require zero replacement-server authentications for SFTP Resume, remote Discard
and rsync Recovery, while restoring the original endpoint still succeeds.
A server/trust rotation during a prepared rsync copy also yields zero replacement
authentications, preserves the destination and cleans its owned staging. A fresh
copy to the restored original server succeeds.

Seven real protocol groups pass with current journals and with journals created
by the earlier qualified adapter. Existing SFTP resume/encrypted-key, rsync file
and recovery suites also pass. These are owned loopback servers, generated keys
and files, with bounded process cleanup. They do not claim external server or
physical input acceptance. The normal frozen source/binary gates and actual
published artifact checks remain required for distribution.
