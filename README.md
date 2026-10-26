# RUWAC

A Rust remote workspace with persistent terminals: one daemon owns PTYs; replaceable TUI and JSON clients control sessions, windows and stable panes.

Implemented: persistent daemon-owned children, independent attached-client focus, nested horizontal/vertical splits, zoom, paged scrollback, private Unix IPC and a separate compiled GTK desktop. The desktop supports keyboard marking/search, real GDK clipboard transport, private PNG staging and an embedded IronRDP client. Targeted real Windows authentication/framebuffer and Fedora GNOME service/login/display tests pass; saved mixed layouts can preserve a live Fedora RDP transport. Local and authenticated SSH Codex PNG draft attachment is implemented. Physical input, broader external clipboard, broader interoperability and qualified Fedora packaging remain open; see the completion matrix for exact acceptance scope. Private ended-history recovery after daemon loss is implemented with the limits below.

Build with Rust/Cargo in your chosen build workspace. Keep build output and generated evidence outside the source tree:

```sh
export CARGO_TARGET_DIR="$(mktemp -d)"
cargo build --locked --release --manifest-path ruwac/Cargo.toml
cargo test --locked --workspace --manifest-path ruwac/Cargo.toml
export EVIDENCE_DIR="$(mktemp -d)"
PYTHONDONTWRITEBYTECODE=1 python3 ruwac/tests/integration/spike.py \
  --binary "$CARGO_TARGET_DIR/release/ruwac" --evidence "$EVIDENCE_DIR"
```

Create and enter a persistent named session with one command. The daemon starts automatically when needed:

```sh
R="$CARGO_TARGET_DIR/release/ruwac"
"$R" -S development
```

Use Ctrl-A then `d` to detach, `ruwac -r development` to reattach, and `ruwac -ls` to list sessions. `ruwac -dmS development` creates a detached session. Multiple connections can attach to the same session with independent window and pane focus.

Exiting an attached shell removes its completed window, including the final window and session. `ruwac -wipe` clears older dead windows and sessions; `ruwac -wipe NAME` limits cleanup to an exact session name or ID. Windows containing live processes remain intact. `-S` and `-R` start a fresh shell when the matching old session has ended; `-r` explains when a session is dead instead of opening an unusable terminal. Explicit `attach --session ID` can still inspect retained, unattached history before cleanup.

New CLI sessions and TUI windows/splits explicitly load your selected login shell, including with a preserved older daemon. Existing shells retain their startup flags. For `--host` named creation, update the endpoint RUWAC CLI too: it resolves the remote shell over verified SSH, rather than using your local shell path. Raw `rpc` and JSON `request` parameters remain literal for custom commands and automation.

New local terminals also receive the creating CLI or GUI's desktop environment,
including its display and clipboard connection settings. Missing settings remove
stale daemon values. This works with preserved older daemons; it does not change
already running shells or applications. A new window created by an already
attached older client retains that client's behavior until it is reopened.
Remote `--host` launches do not forward your local desktop settings.

The default socket is beneath your private runtime directory. `RUWAC_SOCKET` or `--socket PATH` selects an explicit socket; its parent must be owned by you with mode 0700. Use the same socket in each terminal.

Ctrl-A then `c/n/p/d/a` creates/cycles/detaches/sends literal Ctrl-A. Double Ctrl-A forwards the literal prefix; Ctrl-A then o selects the previous window on a matching daemon. Ctrl-A `S` splits horizontally and `|` vertically; `Tab` changes pane; `0–9` selects windows. `[` opens interactive canonical keyboard copy/search in both clients; `]` pastes the independent internal buffer. `z` zooms, `"` opens the window chooser, `:` opens commands and `?` shows configured shortcuts. PTYs survive final-client detach. Each attached client keeps its own selected window, pane and zoom. Shared PTYs and split ratios remain canonical. The session uses the minimum attached rows and columns; each pane uses the minimum dimensions of its visible rectangles. Unattached explicit selection sets the default for future attachments without redirecting current clients. Existing daemons retain the behavior of their running version until deliberately replaced. `rpc daemon.stop` stops the selected daemon and its owned panes.

A pane reports its own process exit after draining ready final output. Background programs retaining the terminal cannot change its ended history afterward; continuous post-exit draining is capped at 8 MiB. Closing and reopening standard streams does not itself end a live program. The PTY regression tests require Python 3 as a test fixture; the installed headless engine does not.

Unix PTYs are required. The tested toolchain is Fedora Rust 1.98.1. Cargo.lock pins registry checksums. Prereleases describe a prototype, not production or desktop acceptance. Tagged source prereleases preserve operational implementation and engineering history. RUWAC’s original source is licensed under Apache-2.0; see [LICENSE](LICENSE). Dependencies retain their own licenses. Binary packaging remains subject to dependency notice and packaging validation.

See [architecture decisions](architecture/00-RUWAC-DECISION.md), [protocol](architecture/02-PROTOCOL-V0.md), [acceptance](architecture/03-ACCEPTANCE-MATRIX.md) and [benchmarks](architecture/04-BENCHMARKS.md).

## Native engineering prerelease

The retained headless core has named target-specific commands and an optional compiled Rust GTK4/libadwaita client, with real GDK text clipboard transport and private PNG staging. See [the implementation and evidence report](spec/RUWAC_INTEGRATION_SPIKE.md), [ADR-001](spec/RUWAC_ARCHITECTURE_ADR.md), and [acceptance limits](spec/RUWAC_ACCEPTANCE_STATUS.md). External-host RDP, physical PRIMARY acceptance, image attachment and RPM packaging remain open. The first machine sidebar and authenticated loopback RDP/CLIPRDR integration are implemented. This milestone is distributed as a source prerelease with the acceptance limits listed below.


The desktop is a separate Cargo workspace. Build it with `cargo build --locked --release --manifest-path ruwac/desktop/Cargo.toml`, using the same external CARGO_TARGET_DIR as the core. `ruwac gui` starts the sibling `ruwac-desktop`; Python GI is retained as a test oracle. Native dependencies are optional; only the desktop workspace includes IronRDP. After updating, close and reopen the desktop to load its new code: an already running GUI keeps its previous version. Daemon-owned terminal sessions remain available to reattach; RDP desktops reconnect after reopening. See the current engineering reports before treating fixture success as desktop acceptance.

Optional automatic GNOME sign-in requires local `python3-pillow`, `tesseract` and `tesseract-langpack-eng`. Enable it under the saved profile’s GNOME User Login section. The supported English Fedora GDM flow is verified on an authorized host; unknown screens and changed focus stop input. See [GNOME login behavior](spec/RUWAC_GNOME_LOGIN.md).

The desktop inventory uses system SQLite through rusqlite. Fedora desktop builds also require `sqlite-devel`; the installed desktop requires the SQLite runtime library. The headless daemon and CLI require neither. Private `machines.sqlite3` is canonical storage; `machines.json` remains a compatibility bridge for older windows. Conflicting or malformed storage is preserved and unsafe writes are refused. Closing old editors before concurrent edits avoids races with older releases that do not participate in SQLite transactions.

Release versions use numeric `major.minor.patch`, with odd minor numbers for beta and even minor numbers for stable. Repeated releases in the same lane advance the patch. Historical suffix tags are retained as earlier numbering mistakes; subsequent releases follow the numeric policy.


Launch the desktop from the same external build directory and socket:

```sh
"$CARGO_TARGET_DIR/release/ruwac" gui --session session_ID
# Repeated --session IDs on ruwac-desktop open additional persistent tabs.
```

Create windows with Ctrl-Alt-A c, split with Ctrl-Alt-A S or |, move between panes with Ctrl-Alt-A Tab, and zoom/restore with Ctrl-Alt-A z. Drag dividers to change persistent ratios. Ctrl-Alt-A [ opens canonical history: arrows/hjkl, Page Up/Down, g/G, Space to mark, Enter to save internally, / to search and Escape to return. Ctrl-Alt-A ] pastes that buffer. Ctrl-Shift-C/V use desktop CLIPBOARD separately. Ctrl-Alt-A d detaches; opening the same session again retains running programs and layout. Plain Ctrl-A is forwarded to the terminal application.

Private configuration may set RUWAC_SCROLLBACK_LINES (default 50000) and RUWAC_SCROLLBACK_MIB (default 256; approximate cell budget). Copy view has an 8 MiB cap with visible truncation. RUWAC_SHORTCUTS remaps GTK accelerators for prefix/copy/paste; RUWAC_KEYMAP remaps prefix commands. Both remapping variables contain JSON objects, for example `RUWAC_SHORTCUTS='{"prefix":"<Control>b"}'` and `RUWAC_KEYMAP='{"x":"z"}'` (x invokes zoom). The TUI supports Control prefixes and prefix-command remapping. `v` toggles rectangular selection, `w/b/e` move by Unicode words, and `n/N` repeat searches. Oversized selections and cancellation preserve the existing internal buffer. See client help for the current configuration.

“New RDP tab” opens the in-process client. Supply an authorized host:port, username, a Secret Service profile reference and optional trusted CA PEM. The lookup uses attributes application=RUWAC and profile=the reference; secrets are not stored in the project. System certificate/hostname validation is mandatory. Fit/native/stretch/fullscreen and explicit send-key actions are implemented. Text and PNG CLIPRDR works in both directions with five clipboard policies, explicit send/receive and remote paste. Authenticated loopback sessions are tested; dynamic resizing is qualified against authenticated loopback sessions; external Windows/GNOME hosts remain unqualified.

“Machines” toggles the sidebar; Alt+M searches names, full groups, endpoints and tags. Favorites and recent-use filters are available. All matching profiles appear in one scrollable list; group headings collapse or expand their machines. Choose a Remmina file explicitly to preview approved nonsecret metadata, then import it. The keyring checkbox copies available saved service passwords into RUWAC's Secret Service entries; existing RUWAC passwords are retained. Certificates and private key files are excluded. To update an existing endpoint's metadata, preview the file, explicitly select its endpoint identity, and use Review merge into selected endpoint. Review the field changes before Apply; local IDs, credential references, trust/key paths and preferences are preserved. Active endpoints and changes made after review require closing tabs or reviewing again. Cancel leaves metadata unchanged; Undo restores the previous private library. RDP entries open native tabs; SSH entries create daemon-owned SSH terminals. Native profile editing and Set/replace/forget credential controls are implemented.

## Pasting images into SSH terminals

The existing Ctrl+V image workflow is retained: when an SSH bookmark's terminal
is focused and the clipboard contains only a PNG, RUWAC copies it to the same
SSH endpoint and pastes its remote path into that original pane. Continue typing
in the same pane while it uploads. Ordinary local Codex clipboard behavior and
the explicit local/remote draft-attachment actions use their existing paths.

Changing tabs, panes, active window, endpoint configuration or clipboard owner
cancels pending path delivery. A new image paste supersedes the previous one.
RUWAC never redirects the result to the newly selected terminal. An interrupted
operation may leave a complete private PNG in the remote cache; it does not
submit a provider turn or claim that a model has consumed the image.

New saved terminal and agent launches own a private OpenSSH control connection.
Implicit image paste opens another channel on that same authenticated connection;
it does not resolve the bookmark or jump configuration again. The socket owner
must be the original pane's SSH process. Missing, closed or replaced connections
refuse image upload without a new-network fallback; reopen the bookmark explicitly.

The remote receiver verifies the byte count and SHA-256 before reporting readiness.
The local uploader authorizes publication only while its cancellation ticket and
deadline remain valid. EOF or cancellation before that commit removes the temporary
upload. A later clipboard/focus change can leave an already committed private PNG
in the cache, but cannot redirect or paste its path into another terminal.

The original connection uses the selected OpenSSH route and verified host identity.
Saved SSH passwords and key passphrases stay in Secret Service; background image
paste does not open authentication dialogs or approve new host keys. Use the
normal connection setup when authentication needs attention. The byte count and
SHA-256 must match before an owned 0600 PNG is published in the remote owned
0700 cache directory. Temporary uploads are removed on ordinary failure.

The current receiver expects a Linux shell and standard coreutils on the remote
host. It bounds stdin, both output pipes and process exit with one deadline.
New saved SSH and agent launches retain their endpoint, provider and original
pane association across frontend restarts and independent windows. The private
catalog is bound to the running daemon and its pane/process identities. A new
local split does not inherit that SSH image destination. The Agents menu selects
the recorded agent pane/window rather than whichever local split was last active. Changed or deleted
connection identities refuse image delivery and require opening the current
bookmark. Restoring launch metadata never starts a session or an agent. Older sessions without
launch metadata remain available through Sessions; their endpoint is not guessed.

Reconnect rebuilds the explicit launch recipe with the current installed client.
Agent commands and backend choices are retained as nonsecret launch settings;
passwords and key passphrases remain in Secret Service. The catalog contains no
generated SSH argv or duplicate credential references. Private storage is bounded;
a failure to save is shown while the live connection remains usable. Changes to
external OpenSSH configuration do not redirect an existing protected image channel;
explicit reconnect uses the current configuration. Older catalog entries without
a protected transport remain usable as terminals and require reopening the bookmark
for implicit image uploads. A replaced
daemon gets a separate catalog; recovered ended histories are not treated as
verified live SSH connections.

## Machine workspaces and Files

Use Machines → New machine / endpoint or Network groups / defaults, and Edit beside an endpoint. Add endpoint retains the machine identity; duplicate creates new identities and credential references. Set/replace/forget credentials use system Secret Service. Metadata Undo restores the previous private library. RDP connects inside its tab. The SSH Terminal action opens a persistent terminal; use the separate Files action to open the native SFTP/rsync view. Files verifies known_hosts before authentication and offers navigation, upload/download, explicit overwrite and progress. Pause transfer retains a private journal and partial; Pending transfers… resumes or explicitly discards the selected job. Resume verifies the original server key before authentication, even if an SSH alias redirects to another trusted server; it also verifies the source, retained prefix and complete staged file before commit. Additional rsync SSH channels are pinned to the server verified by SFTP. Original files remain protected during overwrite and interrupted rename recovery. For encrypted SSH keys, configure the key file and the separate SSH key passphrase reference, then use Set / replace key passphrase; only the Secret Service reference is saved. SFTP transfers individual regular files up to 32 GiB, with 128 pending jobs. The Files view also offers staged rsync file/folder copies over verified key/agent SSH, explicit whole-destination replacement and interrupted-copy recovery. Partial rsync data restarts rather than resuming; see [file transfer limits](spec/RUWAC_FILES.md). Dynamic RDP uses negotiated Display Control; fit/native/stretch, zoom, fullscreen and Pan viewport remain available. See the current acceptance report for fixture evidence and remaining defects.

To create or reconnect to a named terminal session, run `ruwac -S d` in an interactive terminal. Use `ruwac -r d` to reattach without creating a missing session. Multiple clients can attach simultaneously, with independent selected windows and panes. Double Ctrl+A forwards the prefix into a nested terminal; Ctrl+A then o toggles the previous RUWAC window. Exact duplicate names require an explicit session ID. The same shortcut works with `ruwac --host SSH_HOST -S d`.

## Install and open the workspace

From the root of a released source checkout, build both workspaces and install
with the same external Cargo target directory. Desktop builds require the
[development libraries](CONTRIBUTING.md#build-and-validate), including SQLite.

```sh
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$(mktemp -d)}"
cargo build --release --locked --manifest-path ruwac/Cargo.toml
cargo build --release --locked --manifest-path ruwac/desktop/Cargo.toml
python3 scripts/install-user.py --build-root "$PWD" --target-dir "$CARGO_TARGET_DIR" --desktop
"$HOME/.local/bin/ruwac" gui
```

For a headless installation, build only the first workspace and omit `--desktop`.
`--target-dir` applies to source builds; testing bundles retain their verified
internal artifact paths. The source installer does not allocate a release version.
Private maintainer build/release adapters are not included in public source
archives; the Cargo commands above do not require them.

The per-user installer adds a RUWAC desktop launcher and commands in the standard user binary directory. `ruwac gui` starts or reuses the persistent daemon. Use New terminal for another session and Sessions to reopen a running one. Closing a terminal tab detaches it; automatic checkpoints and normal window close save terminal/RDP/Files tabs, split geometry and selected tab. RDP restores disconnected, and Files restores its directory without automatic authentication. `ruwac start` starts the daemon without a graphical client.

The full headless suite and three genuine SSH remote-PTY detach/reattach cycles also pass on an authorized Fedora host. This does not establish external RDP login or physical-desktop acceptance. See [all remaining product gates](spec/RUWAC_PRODUCT_COMPLETION.md).

Use **Split tabs…** to place existing terminal, RDP or Files views side by side or above/below each other. **Separate views** returns them to tabs while preserving connections. RDP reconnect/disconnect, fit, dynamic resolution, zoom and fullscreen are visible in the native toolbar. Additional connection, remote-key and clipboard actions are under **RDP controls**. Clipboard ownership follows the focused canvas; local controls and Files cannot let inactive RDP sessions take over CLIPBOARD.

Drag a tab outside the window, or right-click its title and choose **Move tab to new window**. Its terminal, RDP or mixed split tree moves with its existing connections. Drag it back to a tab bar to combine the windows again. Closing the original window leaves detached windows running. Saved workspaces retain their tabs. Return detached tabs to the original window before opening or restoring a layout there; reopening a saved workspace gathers its tabs into one window.

New RDP connections and imported profiles default to **Dynamic resolution**. To update an existing library, run `ruwac-desktop --dynamic-rdp`; this updates saved RDP display modes and inherited group display defaults while retaining credentials and other preferences. Reopen the GUI and reconnect existing tabs to load the updated library. Servers without Display Control support use local scaling.

**Views…** lets you move one mixed view to its own tab or close it. Closing a terminal view detaches it; use Sessions to reopen its still-running programs. Closing RDP disconnects just that view. Closing Files pauses its transfer; reopen the same endpoint’s Files tab to resume or discard the retained job. Neighboring views and connections remain open. Empty workspaces can create a new terminal from the toolbar.

## Control another host

Use an existing SSH alias/key and verified host key. The endpoint must have RUWAC installed:

```sh
ruwac --host SSH_HOST describe
ruwac --host SSH_HOST start
ruwac --host SSH_HOST session list --json
ruwac --host SSH_HOST pane send-text --id pane_ID --file local-input.txt --json
ruwac --host SSH_HOST pane send-key --id pane_ID --key Enter --json
ruwac --host SSH_HOST attach --session session_ID
```

Global `--remote-binary PATH` and `--socket REMOTE_PATH` select an explicit executable/socket. Remote request content is literal JSON over stdin. Password prompting, agent forwarding and desktop forwarding are disabled. `describe` exposes actual endpoint capabilities and supports older live daemons through read-only aggregation; it does not upgrade or restart them.

## Desktop navigation and diagnostics

Launch `ruwac gui`. By default, Alt+1–9 select workspace tabs 1–9 and Alt+0 selects tab 10; system shortcuts such as Alt+Tab stay on this computer. Tap AltGr (Right Alt) on its own to enable **Map keys**, shown in the header: those keys then go to the terminal program or remote desktop. A current RUWAC attached client receiving Alt+digits selects its own windows. Tap AltGr again to return to workspace tabs and local system shortcuts. AltGr held with another key always types keyboard-layout characters. The shortcuts work from terminal, RDP and Files views. Missing tabs are a no-op. Other unbound Ctrl+Alt and Shift+Alt shortcuts remain available to applications. Use `ruwac gui --theme system` to follow the desktop color preference, or `--theme light` / `--theme dark` explicitly.

`ruwac doctor --desktop` checks local runtime availability without changing it. `ruwac doctor --terminal` inspects the current terminal. After a crashed client leaves raw/no-echo attributes, run `ruwac doctor --terminal --repair` directly in that affected foreground terminal. Repair never targets another terminal and does not reset emulator mouse/private modes. Remote read-only checks use `ruwac --host SSH_HOST doctor`; remote terminal repair is refused.

## Address native workspace views

Run the desktop, then query its separate local API using the same daemon/socket target:

```sh
ruwac workspace describe --json
ruwac workspace list --json
ruwac workspace activate --id "$RUWAC_TAB_ID" --json
ruwac workspace focus --id "$RUWAC_VIEW_ID" --json
```

Use actual IDs returned by list; titles and numeric positions are not automation identities. Enable **Allow automation to change workspace focus** in the desktop menu before activate/focus. The permission resets to off for a new launch; `ruwac gui --allow-workspace-control` enables it for that launch. Listing needs a running GUI and never launches one. Clipboard contents and RDP input are outside this API. `input_focus_view` reports actual GTK input focus separately from remembered focus, which may briefly precede widget mapping. Background focus changes do not grant remote clipboard ownership.

The native **Views… → Focus** action selects one mixed-workspace view. IDs and each tab’s selected view persist in schema-three metadata. Older workspace files get a private migration backup; terminal programs continue running across desktop restarts.

## Recover history after daemon loss

The ordinary daemon checkpoints private metadata and a bounded canonical history tail under the standard user state directory. Restart restores the same session/window/pane IDs as ended history, without starting their old commands. Open Sessions to view it, use the existing copy controls, and choose New terminal to start a live session.

`ruwac rpc daemon.checkpoint --params '{}'` waits for a completed checkpoint. `ruwac rpc health --params '{}'` reports checkpoint progress/failures. Changed state saves every two seconds; a crash may lose changes since the last completed save. The retained tail is up to 50,000 lines or 512 KiB per pane, with explicit truncation counts. Internal clipboard buffers, input/request logs and credentials are not serialized; displayed output remains private terminal state.

Default state is `RUWAC/terminal-history.json` under `XDG_STATE_HOME` or the standard home state directory. Custom sockets keep a sibling history file; set `RUWAC_STATE_FILE` to a file beneath an existing owned 0700 durable directory when persistence across runtime-directory removal is needed. Files must be owned regular 0600 files; corrupt/unsafe archives are preserved and rejected. Closing ended panes and checkpointing removes their history. Hidden primary-screen history while an alternate-screen application runs is not yet stored separately. Existing older daemons are preserved during installation and require a later owner-controlled restart to gain checkpoints.

## Native machine and RDP controls

The searchable machine sidebar shows grouped profiles with native protocol icons, names and endpoints. Use **New machine / endpoint…** to create a profile and **Edit → Set/replace credential** to enter its password through Secret Service. Passwords remain out of the profile file and command arguments. A connection still requires trusted server identity.

The RDP toolbar reuses the same authenticated connection and display handlers as the advanced controls. Reconnect/disconnect affects only that view; fit and zoom do not restart the connection. Dynamic resolution requests a negotiated remote display change, with local fit as a fallback when unsupported. Closing the GUI preserves daemon-owned terminal programs. A previously running older GUI remains active during installation; close/reopen its window to see a newly installed interface. Older daemons visibly reject unsupported previous-window navigation without losing subsequent input.

The GitHub workflow publishes source releases and compiled Fedora 44 x86_64 testing bundles and RPMs with checksums and artifact provenance. Local build.sh compiles the independent headless binary and optional GTK binary. These testing artifacts do not establish clean-system dependency resolution, complete external interoperability or production security qualification; see the current acceptance status.

### Native RDP automation

`ruwac rdp list --json` and `ruwac rdp query --id view_ID --json` read safe attached-view metadata. `ruwac rdp open --profile endpoint_ID --json`, `ruwac rdp reconnect --id view_ID --json` and `ruwac rdp disconnect --id view_ID --json` require the desktop permission “Allow automation to open/control RDP connections”. Open defaults to background operation; optional `--activate` also requires focus permission. Startup is asynchronous: query phase and decoded updates to establish readiness. Disconnect keeps the view; closing makes its ID stale. GUI operations reject idempotency keys.

Prefix these commands with `--host user@host` to use the existing verified SSH transport and installed remote RUWAC CLI. The remote desktop must already be running with its trusted GUI socket and matching permissions. Missing GUI endpoints do not start graphical or daemon processes. This does not enable remote input, credential retrieval, clipboard reads or event subscriptions.

The machine sidebar keeps saved endpoints, search/filter controls, paging and **New machine / endpoint…** visible. Expand **Machine tools** for network defaults, refresh and Undo; expand **Import from Remmina** for explicit file selection, preview and reviewed import/merge. Both panels start collapsed. The import area scrolls when expanded, and diagnostic text is selectable.

### Mixed terminal compatibility

Reproduce the two-to-five-layer GNU Screen/tmux/RUWAC matrix with `python3 ruwac/tests/integration/mixed_nesting.py --binary /path/to/ruwac --evidence /path/to/private-evidence`. The fixture creates only private sessions/sockets/configuration and records actual tool versions, leaf PID/start time, byte hashes and dimensions. GNU Screen and tmux are test-only tools; missing tools mean blocked compatibility qualification.

Both common Control-A and native tmux Control-B configurations are exercised. Escape a prefix at each intermediate layer to address a deeper layer. Normal Unicode/multiline paste and a single bracket pair are verified; tmux/RUWAC also contain control prefixes in paste. GNU Screen 5.0.1 interprets its own Control-A commands inside a pasted payload: do not assume arbitrary-control paste is safe through Screen. Explicit `pane.sendText` to a known leaf RUWAC daemon bypasses intermediate client key parsers. [GNU Screen’s command bindings](https://www.gnu.org/software/screen/manual/screen.html) describe its detach and literal-prefix controls.

Process metadata transport limits: `job.list` and `job list` return at most the requested 100 rows and a 64 KiB serialized metadata page. Follow `next_offset` until null to read the complete collection. The native Jobs… monitor performs this traversal automatically, verifies membership and unique stable IDs, and displays 50 rows per page. It retains the previous display when a collection changes mid-read and retries on the next poll. Full-capacity testing covers 128 actual panes with long Unicode metadata; larger correlation IDs, other response types and broader protocol fuzz/soak remain separate gates.

Contribution and repository checks: see [CONTRIBUTING.md](CONTRIBUTING.md), [SECURITY.md](SECURITY.md) and [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md). The pinned Fedora workflow checks formatting, Clippy, units, real headless fixtures and declared dependency/checksum/graph boundaries on pull requests and main. Separate advisory checks remain failing for the documented unpatched desktop RSA dependency; this is not stable security qualification. Contribution labels use trusted base code and do not merge changes or publish artifacts.


Native windows, the overlay sidebar and GNOME dual-credential storage are documented in [GNOME login and desktop windows](spec/RUWAC_GNOME_LOGIN.md). GNOME modes support the non-AVC Graphics Pipeline and verified same-endpoint RDSTLS handover. Real Fedora GDM rendering is tested; Linux user sign-in remains a separate explicit action.

RUWAC agent connections check and display the remote CLI version and executable path on every attachment. Enable “Check and update RUWAC on agent connection” in the machine properties to check verified published releases and update an existing per-user installation as well as installing a missing CLI. If an update check fails, the connection reports the failure and tries the installed CLI. Existing engines and sessions keep running; a newer CLI does not replace them. Per-user installations use `ruwac update --check` / `ruwac update --install`; `ruwac-rpm-update` belongs to system RPM installations.

Fedora 44 updates: use Main menu → Updates or `ruwac update --check` and `ruwac update --install`. Automatic checks announce newer stable GitHub releases; per-user installations support source builds or explicitly selected testing bundles, retaining active sessions and the previous version. Bundle updates require `gh` to verify signed build provenance before installation. System installations use the release RPMs and DNF. See [prerequisites, bootstrap and rollback](spec/RUWAC_UPDATES.md).

The native Files view supports SFTP and verified staged rsync copies of files and folders. See [SSH file transfers](spec/RUWAC_FILES.md) for authentication, replacement and cancellation behavior.

## Fedora testing RPMs

Tagged releases provide upstream Fedora 44 x86_64 testing RPMs: `ruwac` contains
only the daemon/CLI; `ruwac-desktop` adds GTK/RDP and depends on the matching core
package. Download both RPMs and their checksum/provenance assets from the same
release, then install the downloaded files with `sudo dnf install ./ruwac-*.rpm`.
Launch the system package explicitly with `/usr/bin/ruwac gui` if a prior per-user
installation is also present. RPM transactions do not restart sessions or alter
private profiles and credentials.

Build testing packages through `./build.sh --release --desktop --package rpm`.
Omit `--desktop` for the independent headless package. The controller-assigned
Cargo version is reused; `1.fc44` is the fixed RPM packaging revision. Build output
stays under the configured external build workspace. Repeated outputs are preserved.

These upstream RPMs are unsigned RPM packages with GitHub workflow provenance;
they are not Fedora repository or complete security qualification. Verify the
published hashes and attestation before installation. Updates to system RPMs use
DNF or the verified system updater. The RPM UI supports administrator-authorized
installation and explicit opt-in to a system update timer. It refuses per-user
installation or rollback that would shadow the system package.
Per-user source/testing-bundle updates remain available for separately installed
per-user builds. System updates remain disabled until an administrator enables them;
see [setup and opt-out](spec/RUWAC_UPDATES.md). A signed Fedora package repository
remains unfinished. The desktop RSA advisory remains unsuppressed.

A fresh GTK workspace creates its own terminal session instead of borrowing an unrelated CLI session. Explicitly attaching the same session shares its PTY size: the smallest attached viewport governs it, and the status bar shows the shared size. Closing or detaching the smaller view allows it to expand. Remmina import discovers native, legacy and Flatpak profile directories, previews the selected metadata, and requires an explicit import; custom file selection remains available. [Remmina documents its profile locations](https://remmina.org/how-to-install-remmina/).

To retain the legacy Screen double-prefix previous-window action, set `RUWAC_SHORTCUTS='{"double_prefix":"previous"}'` when launching the client. Literal prefix forwarding remains available through the local prefix then a (Ctrl+A in CLI, Ctrl+Alt+A in GTK).


### Screen command aliases

`ruwac -ls` (`-list`) prints a readable session table; add `--json` for automation.
`ruwac -S work` starts or shares the named session in one command. `-r work`
and `-x work` attach to it without creating a missing session. `-R work` attaches
or creates it; `-dmS work` creates it detached. Concurrent attachments retain their
own selected window and use the canonical shared PTY size.

`ruwac -S work -X screen [NAME]`, `title NAME`, `windows`, `next`, `prev`,
`other`, and `select N` control an existing exact named session. Explicit `-X kill`
closes the current window's programs; `-X quit` closes all programs in that session.
These destructive automation commands do not prompt.

The CLI uses Ctrl+A as its local prefix; the GTK terminal uses Ctrl+Alt+A so
plain Ctrl+A reaches applications such as tmux and Readline. After the local
prefix, Space selects the next window; Backspace selects the
previous one; `w` opens the window chooser; `A` prompts for a title; Escape enters
copy mode; `k` asks before killing the window. Existing `c`, `n`, `p`, `d`, digits,
`s` or `S` splits horizontally; `|` splits vertically. Tab, `[`, `]`, `?`, and `:`
remain available. Double the local prefix to forward literal Ctrl+A to a nested
terminal. Follow the local prefix with `o` for the other window.
Command prompts accept `screen`, `title`, `windowlist`, `split -v`, and `focus`.
`only` maximizes the current pane without discarding the other persistent panes.
TUI `quit` requires confirmation; GTK window close preserves terminal sessions.
Screen configuration files, logging, multiuser ACLs, serial terminals and its full
command language are not implemented; unsupported commands report an error.

Workspace actions and RDP controls are collapsed behind centered disclosure arrows.
Hover previews the controls; click keeps them open. RDP status and resolution share
one compact row, with further display details in its tooltip. Remmina discovery is
read-only; selecting a profile previews metadata, then **Import selected profile**
imports it explicitly. Passwords are never imported.


The single-row header keeps tabs beside collapsible actions. **Controls** (F10 or
Ctrl+.) opens the selected session's actions without changing its viewport. An
optional dock occupies a normal row. Appearance offers six palettes, terminal fonts,
size and line spacing. See [desktop controls and remote shortcuts](spec/RUWAC_DESKTOP_CHROME.md).


Use **Machines → Import from Remmina → Import all Remmina profiles…** to review
and import metadata from the discovered native, legacy and Flatpak locations.
Confirm **Import all** to save the batch. Existing profiles are retained, matching
duplicates are skipped, unreadable/unsupported files are counted, and changed files
or a changed library require a fresh review. **Include saved passwords from Remmina's keyring**
copies available service passwords, including missing passwords for matching existing
profiles. It never replaces an existing RUWAC password or copies a service password
into the separate GNOME user login. Missing, locked or legacy encrypted-file passwords
require **Edit → Set/replace credential**. Turn the checkbox off for metadata only.
Certificates and private key paths are excluded; source Remmina files are read-only. The 1000-profile limit is
checked before writing any of the batch.

Remmina's TLS-error bypass is not imported. On a self-signed RDP server, use
**Review certificate** and verify its identity before trusting it. Fedora Remote
Login needs the RDP service account and the separate GNOME desktop account;
configure both before enabling automatic sign-in.

Terminal selection joins soft screen wraps and retains actual line breaks. It
also removes row padding after shell continuation backslashes. Current daemons supply wrap metadata directly. Compatible older daemons supply it
through a read-only history check that must match the displayed cells. Live engines
and their running programs are preserved.

### Machine groups

Click a group heading in Machines to collapse or expand its connections. RUWAC remembers collapsed groups across restarts. Search temporarily reveals matching connections; clearing the search restores your collapsed groups. Favorites stay first within each group.

The CLI bottom-row window labels accept mouse clicks. An interactive view leaves
a session when its last running shell exits; ended history remains inspectable.
New GTK windows and splits explicitly request the user's login shell, including
when attached to an older daemon. Existing shells keep their original startup mode.

Right-click an RDP **tab** for reconnect, disconnect and display modes. Remote
framebuffer right-click remains remote input. Tab icons show connection state;
disconnected configured views show a centered reconnect action. Reconnects retain
the previous decoded desktop size, with Dynamic mode initially using the current
viewport. External server behavior remains subject to interoperability testing.

**Main menu → About RUWAC / Support…** shows the installed version, Apache-2.0
license, source link and an offline QR code for the optional
[RUWAC support page](https://idz.si/bs/ruwac-donate.html). No payment credentials are
contained in the application. Updating installed files does not change a running
GUI; restarting it still reconnects RDP transports. Leave an active window open
until ready to reconnect. Terminal PTYs remain owned by the persistent daemon.

## Terminal sizes and older running daemons

Current engines support up to 512 rows and 1024 columns within a 262144-cell budget. Clients use the engine’s advertised bounds; simultaneous attachments still share the minimum dimensions. Updates preserve live older daemons, which retain their previous limits until you deliberately replace them after saving your work. A fresh private daemon can be used independently through --socket without stopping the existing one. Large terminal renders explicitly opt into bounded replies; ordinary control requests keep the existing 1 MiB limit.

The CLI window bar keeps the selected window visible. Click clipped labels or the ‹/› arrows to select visible or hidden windows. These clicks do not reach terminal applications.


RUWAC is authored by **Denis Zvegelj**. [Support development](https://idz.si/bs/ruwac-donate.html).

Machine shortcuts share one row: the machine button opens its saved protocol, with SSH and configured Codex/Claude actions alongside it. For an RDP-only bookmark, SSH/agent actions first open a separate SSH endpoint editor; the RDP service username and password are not reused. New SSH setup defaults to the current local username. Save an explicit username or selected identity-file path to override it. The private key stays on your computer; key passphrases belong in Secret Service. After saving a key, **Authorize SSH public key on machine** opens a confirmed `ssh-copy-id` operation in a terminal, preserving ordinary host-key verification and interactive authentication.

The header's agent menu lists saved hosts and configured agent actions. Terminal right-click offers recent saved SSH connections followed by saved bookmarks; use Shift+right-click when the foreground application handles mouse events. Ctrl+Shift+T creates a terminal tab; Ctrl+Shift+R opens the machine chooser filtered for RDP. These local chords are handled before remote terminal/RDP input.

Enable **Share one remote RUWAC session** in a machine's agent settings to put Codex, Claude and Kimi in separate named windows of one persistent session. Enter an existing session name (for example the previously created `codex` session) or a new shared name. Reopening an agent reuses its named window and process; a changed command applies only when creating that window. Separate sessions remain the default. Update the remote CLI first; shared windows require independent client focus from its engine. `ruwac attach --session session_ID --window window_ID` selects the initial window only for that client. An explicitly launched agent view exits when that agent ends, keeping other agents running.

Kimi is available as another machine and saved-host shortcut, with editable default `kimi --session`. **Automatically install missing Codex / Claude / Kimi on launch** is opt-in per machine. Only a new agent process runs this bootstrap; reattachment never installs or restarts its agent. It downloads the selected provider's official HTTPS installer completely into a temporary private directory, then runs it as the remote user with bounded download/install times. It does not use sudo, supply API keys, copy a login or start a conversation. Provider login remains in its own terminal flow. Existing tools, aliases and functions are detected before installation.

Installation references: [Codex](https://learn.chatgpt.com/docs/codex/cli), [Claude Code](https://code.claude.com/docs/en/setup), [Kimi Code CLI](https://moonshotai.github.io/kimi-code/en/guides/getting-started).

The desktop restores its last normal window size, maximized/fullscreen state and saved workspace tabs. Terminal processes stay in the independent daemon when a desktop window closes. Separate window identities keep their own geometry and workspace; remote RDP transports reconnect when a restored tab is opened. Window placement is managed by the desktop compositor.

Repeated `Terminal` names are independent persistent sessions created by GUI terminal tabs. Use `ruwac -r session_ID` (or `ruwac attach --session session_ID`) to select one exactly. `ruwac kill-session NAME_OR_SESSION_ID` closes every program in that selected session; ambiguous names are refused. Screen-style `ruwac -S session_ID -X quit` also closes that session. Closing a GUI window detaches and preserves its terminal processes.

Product purpose, requested capabilities, realized scope and recent code-review findings: [functionality audit](spec/RUWAC_FUNCTIONALITY_AUDIT.md).

After installation, `man ruwac` documents session commands, keyboard controls and updates. Machines: Alt+M; Agents: Alt+A. Preferences → Tight line spacing reduces vertical spacing; existing saved appearance settings are preserved.

### Mouse selection across Linux RDP desktops

For selecting text on one Linux desktop and middle-clicking on the other, edit
the RDP machine and enable **Share mouse selections with this Linux desktop**
under Advanced. Enter its Linux SSH account, SSH port (0 keeps SSH configuration),
and the exact remote Wayland session, such as `wayland-0`. The SSH account must
be the account owning that desktop, with an existing trusted host key and key
authentication. The configured SSH identity, known-hosts file and jump route apply;
the RDP service password is never reused. Receiving remote selections needs Python3,
GTK4 PyGObject and Xwayland on that desktop (Fedora:
`python3-gobject gtk4 xorg-x11-server-Xwayland`). Native publication through the
GNOME companion does not require Xwayland.

This optional helper receives remote text into local PRIMARY separately from the
Ctrl+C clipboard. It follows the RDP tab’s receive clipboard policy and operates
while its canvas has focus. Inactive remote selections are ignored. The focused
native GUI publishes the selection, including on Wayland; a background Xwayland
client cannot reliably do this. Native PRIMARY delivery to GNOME Shell 50
applications is available through the optional [desktop companion](ruwac/desktop/primary-companion/README.md).
Install it on the receiving desktop with `ruwac-desktop -G install`, open
`ruwac-desktop -G preferences` to enable desktop publication permission, and
authorize each selection connection from the source RDP controls. If status
requires a new login, sign out and back in when convenient; setup never restarts
your desktop. Other desktops retain their available compatibility routes,
including the confirmed remote RUWAC terminal path below.

The standalone companion archive can be checked without installing it or opening
a desktop. Under a bounded test runner, run
`python3 ruwac/tests/integration/primary_companion_package.py --evidence /path/to/new/evidence`.
This checks exact archive inventory/bytes, the project license and schema
compilation with GNOME tools; desktop login, authorization, transport and physical
input require separate acceptance.
Ctrl+C/V continues to work in both directions.
CLIPBOARD, images and saved credentials are unchanged. It stops on tab disconnect
or GUI exit; no background clipboard service or clipboard-history file is installed.
The optional companion remains installed until explicitly removed. A missing
SSH key, unavailable Xwayland, mismatched desktop or missing dependency leaves
normal RDP/Ctrl+C usable and shows a selection-sharing error. Use **Retry selection
connection** after correcting the settings; RDP stays connected and the new native
helper needs fresh approval. Receiving selections supports Linux desktops with a working
Xwayland PRIMARY bridge; it does not add PRIMARY support to Windows.

Headless setup for an existing RDP endpoint is also available (no graphical
display or passwords required):

```sh
ruwac-desktop --receive-primary ENDPOINT_ID LINUX_SSH_USER wayland-0 0
ruwac-desktop --disable-primary ENDPOINT_ID
```

Reconnect the RDP tab after enabling it to load the saved options. If its saved
host, user, credential reference or certificate trust changed, reopen it from
Machines instead. Disabling or changing the saved route stops its existing
selection helper; running terminal processes remain independent.

### Terminal menus and selection paste into remote RUWAC

Terminal tabs have **Copy**, **Paste**, **Paste selection**, **Select text…** and
**Close tab** actions. RDP tabs retain their connection/display actions and
**Retry automatic sign-in**, available for a stopped attempt on a verified
connected GNOME greeter. Copying
retained text is also available after a terminal session ends; pasting into an
ended session is disabled. The terminal's own context menu includes these actions
above the saved hosts. Applications using mouse input keep their right-click;
use the tab menu or Shift+right-click for RUWAC's menu. **Select text…** opens
native scrollback, where ordinary mouse selection works without holding Shift.
SSH terminal output is selected in exactly the same way as local output.

To middle-click a local RUWAC terminal selection into a remote RUWAC terminal,
configure the Linux selection-sharing route above and enable **Middle-click
pastes local terminal selections into remote RUWAC** on its RDP endpoint. In the
receiving GUI's Controls menu enable **Receive mouse selections from another
RUWAC desktop**. Both GUIs must support this feature, and the receiving RUWAC
window must already have focus on its desktop. With mouse-selection sharing enabled, disabling local selection paste causes a clear refusal for selections from outside that remote desktop. Native selections within the same desktop keep their normal mouse path. Select the source text, switch
to the RDP tab and middle-click the remote terminal. Programs handling the mouse
can use **Select text…** or Shift+drag for the source selection.

A selection belonging to the same RDP connection keeps its native middle-click
path within that desktop, including when cross-desktop selection paste is off.

The existing trusted SSH peer asks the foreground native GUI to publish PRIMARY,
confirms the complete selection through the compositor, and only then forwards
the original middle-button events. It neither types text nor changes Ctrl+C/V
CLIPBOARD. Pending clicks are cancelled on source, focus, endpoint, transport,
policy or display changes; refusal/timeout does not paste the old remote
selection. This return path accepts up to128 KiB of UTF-8 text and requires the
remote RUWAC window as its native publisher. For other foreground applications,
use their ordinary Ctrl+C/V clipboard. The opt-in receiver exposes a same-UID,
write-only `clipboard.primary.offer` GUI API; it adds no clipboard-read or remote
keyboard-input permission. Its checkbox disables reception immediately.

Explicit headless setup is also available:

```sh
ruwac-desktop --paste-primary ENDPOINT_ID
ruwac-desktop --enable-primary-receive
ruwac-desktop --disable-primary-receive
```

The first command applies to the sending RDP endpoint; the latter two apply to
the receiving Linux account. Reconnect the configured RDP tab after changing
its endpoint. Existing terminal children remain running during GUI upgrades.
The lighter blue CLI status bar appears in newly started/reattached clients;
older running clients keep their existing rendering until reattached.

New terminals advertise `xterm-256color` and true colour, and clear a launcher-inherited `NO_COLOR` setting. New GUI and CLI login shells also do this when reusing an older engine. Existing shells retain their environment: run `unset NO_COLOR` and restart the affected application to restore its colours. To deliberately disable colours for one application, run `env NO_COLOR=1 application` or configure that application directly.

Terminal status labels and window titles show the foreground command, refreshing quiet commands twice per second when the engine provides job metadata. Custom window names remain visible alongside the command; arguments are excluded. Exiting a shell removes its completed window when another live window remains. Exiting the last live window detaches the client and retains final history for explicit reattachment.

## Short command options

Every public long option on the CLI, desktop launcher and installed update tools
has a short spelling. Short options are scoped to their command; option values
remain literal. Existing Screen-style `-S`, `-r`, `-x`, `-ls` and `-wipe` commands
retain their meanings. Combined short-option bundles are not supported.

Examples: `ruwac update -i -p -H` installs the verified headless prebuilt update;
`ruwac -H HOST pane send-text -i PANE_ID -t TEXT -j` addresses an explicit SSH
endpoint; `ruwac gui -S SESSION_ID` opens a terminal tab. Use `ruwac -h`,
`ruwac gui -h` and `ruwac update -h` for their full short/long option pairs.

Desktop terminal shortcuts default to selection-aware Ctrl+C and clipboard
Ctrl+V. With no terminal selection Ctrl+C still interrupts the application.
Preferences can restore traditional Ctrl+C/V control bytes. Ctrl+Shift+C/V remain
available, and protected image-only SSH paste keeps its original destination
checks. Mouse selection publishes PRIMARY; separately opt in to also copy a
terminal selection to CLIPBOARD. No desktop-wide PRIMARY mirroring is enabled.

## Web Preview

New Web Preview opens an ephemeral native WebKitGTK browser in the normal mixed
workspace. It supports back/forward/reload, Ctrl+L, browser copy/paste, inspector,
and the existing tab/split/move actions. Ctrl+click an HTTP/HTTPS terminal URL, or
use its context menu, to open it. Terminal mouse-reporting programs retain their
clicks; use Shift+Ctrl+click to deliberately override that routing.

An SSH terminal's localhost link requires its original bound pane, process,
saved endpoint and verified SSH master. The preview forwards only that explicit
remote loopback port to a local loopback listener. It never substitutes the
workstation's server, re-resolves the SSH alias or creates a fallback connection.
Closing the preview cancels its own forward and preserves the terminal process.
Other processes on the same workstation can access a loopback listener; this is
not isolation from hostile local processes. Remote navigation stays on the
approved origin/port. Relative WebSocket/HMR URLs use that same forward; absolute
development-server URLs must use the approved origin. TLS verification stays on.

At most eight browser views may exist. Remote previews restore disconnected;
enter their original URL and explicitly choose a current verified SSH terminal
to reconnect. Workspace files contain endpoint identities, not browser URLs,
queries, fragments, cookies, tunnel ports or control sockets. Downloads and file
uploads are disabled in Preview; use the external-browser action when needed.
Headless RUWAC does not depend on WebKitGTK. Fedora desktop builds additionally
need `webkitgtk6.0-devel`; installed desktops require `webkitgtk6.0`.

Remote previews close active page networking when the source connection changes.
A changed host/account/trust binding requires a fresh Web Preview, so an old
ephemeral browser session is never reused for a different SSH identity. Reload
and history navigation stay blocked while a remote preview is disconnected.
