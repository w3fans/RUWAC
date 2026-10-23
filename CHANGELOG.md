# Changelog

## Unreleased

## v0.4.40 — 2026-10-23 — Stable source release

- Forward wheel events to full-screen mouse applications such as Codex even when
  Wayland supplies no event position; use widget-local pointer coordinates in tabs
  and splits.

- Add command-scoped short forms for CLI, desktop and installed updater options,
  including `ruwac update -i -p -H`; preserve literal option values and existing
  Screen-style commands.
- Add desktop/traditional terminal shortcut preferences, selection-aware Ctrl+C,
  protected text/image Ctrl+V, optional terminal copy-on-select to CLIPBOARD,
  inline canonical mouse-wheel history with wrapped-command/Unicode reflow and
  bounded, generation-checked history request/handler lifetimes.
- Serialize CLIPRDR offers and acknowledgments, bind asynchronous data delivery
  to its exact pending request/transport, balance intercepted paste keys and
  bound input queues before insertion with stale-input cancellation.
- Retain simultaneous remote image/text formats with lazy format reads and
  preserve clipboard ownership while keyboard focus moves within the toolbar.
  Other applications can read both formats from an existing owned offer after
  focus moves away, without allowing a new background clipboard promotion.
- Present linked SSH login settings separately from shared machine agent options;
  save agent settings consistently across the machine's connection profiles.
- Add explicit GNOME login recovery choices: retry saved sign-in, reconnect the
  saved profile, or cancel automation and use manual login.
- Add native ephemeral Web Preview tabs/splits, conservative terminal URL actions,
  original-master-only SSH loopback forwarding and disconnected workspace restore.
  Close page networking on SSH identity loss, block stale-forward navigation,
  and refuse browser-session reuse for changed connection identities. Browser
  downloads/uploads remain disabled; the headless engine needs no WebKit.
- Patch rustls-webpki, ctutils and Tokio; keep the unresolved RSA advisory visible and
  refresh the acceptance delta. Distribution requires all nineteen frozen gates;
  physical/external-host qualification remains separate.

## v0.4.39 — 2026-10-23 — Stable source release

- Check the selected remote RUWAC CLI version on every agent connection, including
  reattachment. Automatic RUWAC installation now checks verified releases even when
  RUWAC already exists, updates the per-user installation, and preserves running
  engines and sessions. Report an older selected CLI and its executable path; an
  unavailable updater allows attachment with an existing valid CLI.
- Keep the actual tab popup menu current when switching between terminal and
  RDP tabs. Expose automatic sign-in retry there, including after a stopped
  attempt reaches a later verified GNOME greeter.
- Preserve a tab’s explicit “Trust once” certificate approval when refreshing
  options for the same verified endpoint.
- Load saved selection and sign-in options when reconnecting an unchanged
  endpoint. Preserve manually edited fields, and require reopening a saved
  machine if its host, user, credential reference or certificate trust changed.
- Keep native selection paste working within its source RDP desktop without
  requiring the cross-desktop receiver. On connections with mouse-selection
  sharing enabled, refuse outside selections when selection paste is disabled
  instead of forwarding a click that pastes unrelated remote text.
- Recheck a small, low-confidence GDM “Not listed?” button in a local crop while
  retaining the original confidence, field and ordered-password guards.

## v0.4.38 — 2026-10-22 — Stable source release

- Keep deliberate terminal and RDP mouse selections pasteable between tabs. Middle-click pastes an application-owned selection directly into terminal tabs, including nested RUWAC sessions on preserved older engines that cannot exchange desktop selections through OSC 52. Ctrl+C/V stays separate, and other applications retain normal mouse routing when the selection comes from outside RUWAC.
- Count connection setup as part of RDP's one-minute foreground recovery budget, then keep retrying the saved connection quietly with short setup limits. Reconnect remains available immediately while recovery or automatic sign-in is pending; failed authentication and certificate checks still stop retries.
- Restart enabled GNOME automatic sign-in after a completed desktop later reconnects through a new authenticated service handover. Network recovery no longer consumes the login-recognition time limit, and stable connections renew the per-outage handover limit instead of exhausting it over a tab's lifetime. Explicit cancellation, current-profile checks and recognized-field/password guards remain enforced.

## v0.4.37 — 2026-10-22 — Stable source release

- GNOME automatic sign-in now completes in every tab, not just the selected one. With several machines connecting at once, unselected tabs used to wait for focus until their 90-second limit expired and then showed GDM; they now sign in on their own, because credentials go only over that tab's own verified connection and your input to it stays paused until sign-in finishes. Switching tabs or focus no longer cancels it. If the connection drops and reconnects before the password is submitted, sign-in starts over on the new login screen instead of stopping. The manual "GNOME sign-in" dialog still requires the selected, focused tab.
- RDP tabs recover from more kinds of network failure instead of staying disconnected: a connection that times out (setup limit now 30 seconds), is reset during security negotiation, or closes during session setup is retried with the usual backoff. A GNOME login handover that cannot be completed in time is restarted from the beginning instead of being abandoned. Authentication, certificate and server-initiated disconnects are still not retried.

## v0.4.36 — 2026-10-22 — Stable source release

- Updating RUWAC on a remote machine ("Install / update remote RUWAC CLI…" and agent launches that install RUWAC) now works where the RUWAC desktop is also installed for that user: the command-line update brings the existing desktop along instead of refusing with "Retain the desktop installation with --desktop". A refused installation no longer leaves a partial version folder behind that blocked the next attempt with "Existing immutable installation differs".

## v0.4.35 — 2026-10-22 — Stable source release

- RDP connections no longer drop when the link is briefly slow. A full input queue used to close the whole connection with "CLIPRDR queue full; transfer not sent"; now pointer motion is skipped while the queue is busy and other commands wait in order, and only a connection that accepts nothing for 30 seconds is closed. Reconnection after network loss retries for about a minute and a half with growing backoff (previously three tries in about 3.5 seconds, once per tab lifetime); the budget renews after a connection has been up for 30 seconds, and GNOME login handovers no longer use it. RDP sockets use TCP keepalive, and write, reactivation and clipboard response time limits are longer.
- GNOME automatic sign-in no longer stalls silently. Clicking the "Signing in to GNOME…" cover gives it focus (clicks there used to be ignored entirely, so a sign-in opened from the sidebar waited until it timed out), and a selected tab takes focus itself before any input is sent. The cover says what it is waiting for; why sign-in stopped, or which setting is missing (automatic sign-in off, no Linux username, no saved Linux password), is shown on the remote screen; and "Retry automatic sign-in" restarts it on the same login screen. GDM's "Not listed?" username page and greeters without a user list are recognized without the Fedora logo (only the login name is typed there; passwords still need the recognized sequence), and large HiDPI framebuffers are analysed in a full-resolution centred crop instead of being refused.
- Mouse selection in a nested `ruwac` session (inside a RUWAC terminal tab, including over SSH) now reaches the desktop selection, and middle-click or three-finger tap pastes the desktop selection there; RUWAC's session buffer remains the fallback for terminals without OSC 52. The nested client uses OSC 52 PRIMARY; the RUWAC desktop accepts an offer only right after your own mouse release in that pane and answers a read only right after your own middle-click, so programs cannot silently read or replace your selection.

## v0.4.34 — 2026-10-21 — Stable source release

- Remote RUWAC installation (Install / update remote RUWAC CLI…, and agent launches with automatic RUWAC installation) installs its own dependencies first: missing `python3` or GitHub CLI (`gh`, which verifies the signed prebuilt package) come from the system's signed repositories via `sudo dnf install`, which may ask for the password in the SSH terminal. The verified prebuilt per-user installation then runs unchanged. If dependencies cannot be installed, nothing is installed.

- Windows RDP no longer drops with "RDP decode failed … Progressive: SRL stream → SRL: missing terminator": Progressive (RemoteFX) upgrade passes accept SRL streams without the trailing zero byte, and empty streams, as Windows sends them; bits past the end read as zero, matching FreeRDP. Previously the whole connection failed and later input was refused.

## v0.4.33 — 2026-10-21 — Stable source release

- GNOME automatic sign-in works on login screens without the Fedora logo: GDM's own user list ("Not listed?" exactly once and no focused input field of any kind) now identifies the login screen and leads only to the "Not listed?" click; the username and password fields are then accepted within that same sign-in. Screens with the logo work as before; nothing is typed into an unrecognized screen.

- Password boxes in the machine editor show whether a password (RDP, GNOME, SSH key passphrase) is already saved in Secret Service, and update after Set or Forget, so nothing has to be re-entered to find out.

- SSH login choice per endpoint: saved key / agent / `~/.ssh`, saved password, or "Install my SSH key on this machine now…", which starts the confirmed `ssh-copy-id` action for the endpoint's saved, explicitly chosen key (choose one under Detected keys first). The login method itself stays unchanged, so cancelling changes nothing.

- Mouse-wheel scrollback in `ruwac -r`/`-S` sessions works in wide terminals (over 200 columns, up to the engine's 1024) and long-lived sessions: it opens the newest 50,000 lines / 8 MiB instead of silently refusing; failures are shown in the status bar.

- Vivid terminal colours (Controls, on by default) render the 16 standard colours with GNOME Terminal's palette, so programs such as btm look as they do elsewhere; switch it off for the scheme's softer colours.

## v0.4.32 — 2026-10-20 — Stable source release

- Replace startup/periodic SSH discovery with explicit public-key checks. Require selection, separate confirmation and fresh source/key/trust inputs before adding endpoints; preserve existing credentials and manual identities. Bound output, cancellation, jump helpers and batch lifetime.
- Show published, eligible, installed and running versions separately, including mixed RPM/per-user installations and precise skip reasons. Hold source, bundle and RPM updates for failed exact-commit checks or incomplete assets; verify the installed receipt, command links and binary hashes before reporting success.
- Require frozen technical acceptance, current source/binary/evidence hashes and staged-source agreement before publication. Wait for hosted quality/privacy before creating a source release; dependency advisories remain separate.
- Set RPM directory permissions explicitly, and vary inherited default ACLs in repeat-assembly checks so the build directory cannot alter package bytes.
- Reconcile historical preflight evidence and the accepted RDP reconnect-after-reopening boundary; measure idle redraw/CPU/memory and recovery with several terminal and RDP views.

## v0.4.31 — 2026-10-20 — Stable source release

- Assemble Fedora RPMs with the source commit timestamp and verify complete package bytes match across separate assembly directories before publication.

## v0.4.30 — 2026-10-20 — Stable source release

- Verify pending SFTP and rsync recovery jobs' original server key before authentication, including when an SSH alias redirects to another trusted server.
- Bind rsync's additional SSH channels to the server already verified by SFTP using a sealed, anonymous host-key file; changed trust cannot silently redirect an active transfer, and no temporary trust files remain after exit or a crash.

## v0.4.29 — 2026-10-20 — Stable source release

- Install Fedora CI SSH transport dependencies in the initial transaction so OpenSSH selects full systemd before desktop dependencies can select conflicting standalone tmpfiles.

## v0.4.28 — 2026-10-20 — Stable source release

- Remove the final completed window/session after normal attached shell exit; let independent clients finish cleanly when another client removes it.
- Add `ruwac -wipe [NAME_OR_SESSION_ID] [--json]` for dead sessions/windows while preserving live processes, and label dead sessions in human-readable listings.
- Start a fresh shell for `-S`/`-R` when an old named session ended, and explain ended sessions before `-r` enters a dead terminal.
- Show reconnect controls when an SSH session ended before the frontend opened; retire completed local GUI shells after acknowledgement while keeping remote output available for reconnect.
- Explain machine settings written by a newer frontend and verify machine editing preserves selection-sharing options and credential references.

## v0.4.27 — 2026-10-19 — Stable source release

- Show foreground commands in terminal status labels and window titles, retain custom names, and refresh quiet commands without reading command arguments.
- Track running windows across navigation and remove completed windows when a live sibling remains, preserving final-session history for deliberate reattachment.
- Prevent launcher-inherited `NO_COLOR` from disabling terminal application colours, including new GUI/CLI shells on older running engines. Explicit application-level colour preferences remain available.
- Move live tabs and mixed splits into new windows by dragging or through the tab menu, retaining terminal processes and RDP transports when the original window closes.
- Default new group display settings to dynamic resolution and add `--dynamic-rdp` to update existing RDP endpoint and group display settings without changing credentials.
- Give terminal tabs and terminal context menus working Copy/Paste/Select text actions, including copying retained output after a session ends. Keep connection and display actions on RDP tabs.
- Add opt-in, confirmed middle-click selection paste from local/SSH terminal tabs into a focused remote RUWAC terminal over the existing trusted Linux route, preserving the separate Ctrl+C/V clipboard and cancelling stale clicks.
- Soften the CLI status bar to a pleasant light blue with dark, readable text.

## v0.4.26 — 2026-10-18 — Stable source release

- Add opt-in Linux RDP mouse-selection sharing over the endpoint’s trusted SSH route, preserving the separate Ctrl+C clipboard and stopping helpers on disconnect.

## v0.4.25 — 2026-10-18 — Stable source release

- Prepare the isolated hosted test container's private user runtime before desktop unit tests, so agent launch reconstruction exercises its protected SSH transport instead of the terminal-only fallback.

## v0.4.24 — 2026-10-18 — Stable source release

- Copy available Remmina service passwords from Secret Service during reviewed import, including missing passwords for matching existing profiles, while retaining saved RUWAC credentials. Prompt for certificate review on the selected foreground connection and show connection failures clearly during Fedora automatic sign-in.
- Publish mouse selections from history to PRIMARY, clear displaced highlights when terminal contents or selection ownership change, preserve logical command lines when selecting wrapped terminal text, remove hard-line grid padding that breaks shell continuations, and diagnose the missing GitHub verifier before downloading an automatic installation.

- Bind implicit SSH image uploads to the original authenticated connection and pane process across configuration edits and frontend restarts; require an authorized publication commit so a cancelled pending image cannot publish later. Legacy terminals remain open and require explicit reconnect for protected uploads.

- Install source builds from an explicit external Cargo target directory while keeping verified bundle paths fixed; correct public build/install, Map keys and Files instructions and distinguish current behavior from historical notes.

## v0.4.23 — 2026-10-18 — Stable source release

- Drain final terminal output before reporting process exit, keep ended history immutable against inherited background writers, and preserve output/exit status when programs close and reopen their terminal streams.

## v0.4.22 — 2026-10-18 — Stable source release

- Fix agent-menu navigation to update the GUI input target from the authenticated pane acknowledgement, queue typing while selection is pending and ignore stale acknowledgements after a later local pane selection. Deliver that acknowledgement through the worker reply queue.

## v0.4.21 — 2026-10-18 — Stable source release

- Persist explicit SSH/agent endpoint, pane and provider associations across frontend restarts and windows; reconstruct reconnect actions without storing generated SSH commands or credentials.
- Keep image paste bound to the original SSH process and pane, excluding newly split local shells; list saved agent sessions by explicit provider metadata instead of name substrings and focus their recorded pane/window.

## v0.4.20 — 2026-10-18 — Stable source release

- Preserve working SSH image paste while binding asynchronous completion to the original endpoint and pane, with cancellable guarded delivery, private hash-verified upload and bounded full-duplex SSH transport.
- Keep typing during upload and existing local/explicit remote attachment paths; stop background authentication dialogs and wait for complete PTY status-row bytes in the regression test.


## v0.4.19 — 2026-10-17 — Stable source release

- Fix a reviewed TUI shell-exit regression that removed the last session before querying it, producing STALE_ID and losing ended history. Exit the view cleanly while preserving history and live sibling windows.

- Install a recognizable workspace icon, launcher search keywords and `man ruwac`; add a tighter font-spacing option and automatic verified-key SSH bookmarks without copying RDP credentials or changing remote host trust.
- Add Alt+M Machines and Alt+A Agents access, hostname-labelled terminal tabs, coloured connection states and a stronger blue terminal status bar; reduce RDP tab-status inspection overhead and document the functionality/recent-change audit.

## v0.4.18 — 2026-10-17 — Stable source release

- Map keys (AltGr tap; shown as a Map keys badge) replaces Local keys with the opposite default: normally Alt+1…9/0 switch RUWAC tabs directly and system shortcuts such as Alt+Tab stay with this computer; with Map keys on, they go to the terminal program or remote desktop (a RUWAC session switches its windows with Alt+N).

- `exit` ends a terminal even when a background process it started (for example an SSH ControlPersist master) still holds the terminal open: a pane now ends when its own process exits. Takes effect after the daemon restarts. In `ruwac -r`/`-S`, a window whose programs exited is removed like in Screen, and the session ends with its last window.

- Lower idle CPU: tabs redraw only when their content changes, hidden tabs and quiet screens are polled less often (input still wakes them at once), tab decorations are no longer reset ten times a second, and idle nested clients stop re-requesting full frames after changes in other sessions.

- Machine actions connect directly: Terminal, Codex, Claude and Kimi on a machine without an SSH endpoint add one from its address (account, port and route from `~/.ssh/config`) and connect; the pen is the only editor. A Files icon opens the SFTP browser; the Terminal icon no longer opens it alongside. Codex uses a `</>` glyph.

- Optional per machine: install the RUWAC CLI automatically (verified prebuilt package) when an agent launch finds it missing.

- `ruwac-rpm-update -i` and `-s` are short for `--install` and `--status`.

## v0.4.17 — 2026-10-17 — Stable source release

- Alt+1…9/0 always reach the program and are never taken for RUWAC tab switching (use Ctrl+PgUp/PgDn for tabs). A RUWAC session client (`ruwac -r`/`-S`) now switches to window N on Alt+N, matching the numbers in its status bar, so Alt+# works inside RDP, SSH or nested RUWAC sessions. Local keys (AltGr tap) only decide whether system shortcuts such as Alt+Tab go to this computer.

- SSH and agent tabs whose connection or program ends stay open with a Session ended card: Reconnect opens the same connection again in a new tab, Close tab closes it. Typing `exit` in a local terminal still closes the tab.

- Remote RUWAC CLI installation uses the verified prebuilt Fedora bundle (no compiler needed) and builds from source only where no bundle exists.

## v0.4.16 — 2026-10-16 — Stable source release

- Stop nested sessions flickering: the render revision counts changes in every session of the daemon, so a `ruwac -r`/`-S` client inside a RUWAC terminal redrew its whole screen (and hid and re-showed the cursor) about 30 times a second, even when idle, since at least v0.4.8. It now redraws only when its own content changes.

- Standard terminal line spacing (1.00) is the default and replaces the previous 1.30 default in existing preferences; Compact, Normal and Relaxed spacing remain selectable and are kept once chosen.

- The scrollback (history) view uses the terminal's font, size, line spacing and colours and opens at the newest output instead of a blank or top position.

## v0.4.15 — 2026-10-16 — Stable source release

- Nested `ruwac -r`/`-S` sessions handle the mouse like tmux with mouse mode on: over a program without mouse support (such as a shell) the wheel scrolls the session's history and scrolling back down returns to the live view, a drag selects text into the session's paste buffer, and a middle-click (touchpad three-finger tap) pastes it. Programs that use the mouse still receive every event in their own coordinates. Shift keeps the outer terminal's own selection.

## v0.4.14 — 2026-10-16 — Stable source release

- Updates: install the newest complete release (quality and privacy checks passed on its commit, every installed package attached) instead of waiting on the newest tag; newer releases still in qualification are listed as skipped, nothing is ever downgraded, and an unavailable package is never reported as installed. Applies to RPM and testing-bundle updates.

- SSH key access: no network checks at startup and no automatically added endpoints (both shipped in v0.4.12–v0.4.13). Machine tools → Check SSH key access… asks first, checks with your existing keys in OpenSSH batch mode (no password, no new or changed host keys, no agent forwarding, one connection per machine, at most 4 at a time), shows Verified key login / Not verified / Unknown host trust / Host key changed / Needs interaction / Unreachable per machine, names the accepted key when OpenSSH reports it, and adds SSH endpoints only for the rows you select. Results are cached against the resolved OpenSSH configuration, known_hosts and your public keys and become stale when those change. The SSH setup has a Check key login button instead of an automatic probe, and no longer pre-fills a detected key.

- SSH endpoints without an explicit `:port` now use OpenSSH's configuration (Port, HostName, User, ProxyJump…) instead of forcing port 22; SFTP, rsync and jump routes use OpenSSH's resolved host and port. `host:port` still overrides.

- AltGr: a clean AltGr tap toggles local keys (shown in the header): Alt+1…9/0 switch tabs and system shortcuts such as Alt+Tab stay on this computer; the next tap returns keys to the terminal program or remote desktop. AltGr held with another key always types layout characters. Replaces the 0.5 s AltGr leader.

- The history view opened by the mouse wheel starts at the newest output.

- The application launcher offers Install verified update (runs `ruwac update --install`).

## v0.4.13 — 2026-10-16 — Stable source release

- RDP tabs receive system shortcuts (Alt+Tab, Super and similar) while focused, so they act on the remote desktop; GNOME asks once for permission. Pressing AltGr hands the next shortcut back to this computer's desktop for 0.5 s.

- Ctrl+V with an image on the clipboard in an SSH terminal or agent tab copies the image over that tab's SSH connection into a private folder on the host (`~/.cache/ruwac/images`) and pastes its path, which remote Codex and Claude attach as an image. Text pastes are unchanged.

## v0.4.12 — 2026-10-16 — Stable source release

- Nested `ruwac` sessions: send each mouse event to the pane under the pointer in that pane's own coordinates (clicks in split panes previously landed on the wrong row), let the engine encode it for the program, focus a pane by clicking it, and request drag/motion reports when programs such as btm or Codex use them.

- Stop the nested client blinking: repaint frames in place instead of clearing the whole screen on every update; the screen is cleared only on the first frame and after a resize.

- AltGr shortcuts now count from the AltGr press: AltGr held or tapped, then a key within 0.5 s (1…9/0 tabs, T, R, K, Return, Home). Later AltGr chords type layout characters.

- A session that ends without any input from you (for example a failed SSH or agent launch) stays open with its output instead of closing; exits you cause still close the tab.

- Show one sidebar row per machine (its RDP endpoint, with other endpoints such as `+ SSH` in the subtitle) and use icon actions with tooltips (SSH terminal, Codex, Claude, Kimi).

- Detect which hosts already accept your SSH key: each machine is checked once with a batch-mode probe (no prompts, no passwords), RDP machines that accept the key get an SSH endpoint automatically, and Machine tools offers Re-check SSH key access. The SSH setup shows whether key login already works.

- Choose SSH key or saved-password login per SSH endpoint; saved passwords are supplied from Secret Service through a built-in askpass helper, with dialogs for host-key confirmation and other prompts.

- Right-click New term to open a terminal on a saved SSH host.

## v0.4.11 — 2026-10-16 — Stable source release

- Typing into a restored or ended pane in `ruwac -r`/`-S` (for example after a reboot) opens a fresh shell window in the same session and delivers the keys there, instead of failing with `PANE_EXITED`; ended windows remain readable history.

- The CLI client requests terminal mouse reports only while the active program uses the mouse, restoring native selection and middle-click (touchpad three-finger tap) paste inside nested RUWAC sessions.

- Add conventional CLI short forms: `-v`/`-V` version, `-h` help, `ls`/`list`/`-l` session list, `new NAME` for `-S NAME`, and `kill NAME` for `kill-session`.

- Apply Rust formatting so the source quality check passes again; the RPM updater waits for that check before installing.

## v0.4.10 — 2026-10-16 — Stable source release

- Detect the SSH keys you already have (keys in `~/.ssh` with a public half, and `IdentityFile` entries in `~/.ssh/config`) in machine SSH setup: a Detected keys menu, and an empty key field is filled with the preferred key (ed25519, then ECDSA, then RSA). Only public keys and the SSH config are read.

- Colour machines in the sidebar by what is ready to use: violet for AI agents (Codex/Claude/Kimi), green for SSH, plain otherwise.

## v0.4.9 — 2026-10-16 — Stable source release

- Open new desktop windows at the size of the last closed window instead of the default; each window still keeps its own geometry. The desktop-menu New window action now supplies the daemon socket.

- Close terminal tabs, and the window when it was the last tab, when their session is removed by `kill-session` or another client, exactly as after `exit`. A lost daemon connection now shows a centred Reconnect panel that restarts the daemon if needed and reattaches, instead of leaving a dead tab with a warning icon.

- Show running Codex/Claude/Kimi sessions at the top of the agent menu, opening or switching to them in one click.

- Place machine Terminal/Codex/Claude/Kimi actions as labelled buttons on their own line under each machine, clear of the sidebar scrollbar.

- Send a valid RDP desktop scale factor so servers honour the display scale, keep the terminal RDP failure reason for diagnostics, name Progressive/SRL decode failures, and track display-control channel readiness.

- Replace Alt+1…9/0 tab switching (including libadwaita's built-in Alt-digit shortcuts) with an AltGr leader: tap AltGr, then within 0.5 s press 1…9/0, T, R, K, Return or Home. Every tap re-arms; AltGr chords and plain Alt+digit reach applications.

## v0.4.8 — 2026-10-16 — Stable source release

- Correct the independent-client sizing fixture for the reserved split-divider column and include that fixture in local release qualification.

## v0.4.7 — 2026-10-16 — Stable source release

- Accept exact session IDs in reattach and Screen-control aliases, provide scoped `kill-session`, and refuse ambiguous duplicate names.

- Keep native split separators above pane repaint, reserve divider cells in new-engine split layouts, and render CLI split lines without covering application cells. Use a light-blue CLI status bar.

- Remember normal window dimensions, maximized/fullscreen state and existing saved workspace tabs across desktop restarts, with independent geometry for separate windows.

- Add Kimi (`kimi --session`) machine/host actions and an opt-in official per-user installer for missing Codex, Claude or Kimi, without provisioning agent credentials.

- Offer one shared remote RUWAC session with independent named Codex/Claude/Kimi windows, same-process reattachment and explicit initial-window CLI attachment. Preserve the separate-session default.

- Show configured SSH/agent actions on RDP bookmarks, with separate SSH setup and compact single-row machine actions. Add saved-host agent dropdown and terminal context menus with recent SSH connections.
- Expose remote CLI installation from linked bookmarks, editable current-user SSH defaults, key chooser and confirmed public-key authorization. Add Ctrl+Shift+T terminal and Ctrl+Shift+R RDP selection.
- Credit Denis Zvegelj in About and keep one support-page destination.

## v0.4.6 — 2026-10-16 — Stable source release

- Wait for stable GTK viewport allocation before authenticating a new Dynamic RDP connection, preventing a temporary smaller desktop. Preserve background tab focus, explicit cancellation and hidden reconnect sizes.

## v0.4.5 — 2026-10-16 — Stable source release

- Use advertised terminal size limits up to 512 rows, 1024 columns and 262144 cells, with explicit bounded large-render replies and versioned checkpoint support. Preserve running older daemons.
- Keep clipped Unicode window labels clickable, reveal the selected window and navigate hidden windows through status-bar arrows.
- Avoid redundant large-frame copies and repeated blank-glyph layout work in the native terminal renderer.

## v0.4.4 — 2026-10-16 — Stable source release

- Load the selected login shell for CLI-created sessions, Screen control aliases and TUI windows/splits, including with preserved older daemons. Resolve remote defaults at the authenticated SSH endpoint; leave custom raw RPC argv literal.

## v0.4.3 — 2026-10-15 — Stable source release

- Collapse completed split panes in CLI and GTK through the established pane-close operation, repairing focus and expanding the surviving layout without restarting shells or daemons. Keep final-session and restored history inspectable.
- Update the desktop TLS lockfile to rustls 0.23.46 for RUSTSEC-2026-0346 through RUSTSEC-2026-0349. Keep the independent RSA advisory unsuppressed.


## v0.4.2 — 2026-10-15 — Stable source release

- Forward plain Ctrl+A to GTK terminal applications; use Ctrl+Alt+A for local multiplexer commands. Load the login shell explicitly in new GUI windows and splits with older daemons.
- Close live terminal views when their last shell exits, retaining ended history and selecting surviving session windows.
- Use actual terminal allocations without the former two-row reservation or 180-column cap; defer attaching unmapped tabs.
- Make CLI window status labels clickable and support lowercase horizontal-split aliases.
- Discard stale input queued during authentication before activating the replacement RDP transport.
- Preserve desktop dimensions through RDP reconnects; add contextual tab controls, connection icons, concise titles and centered reconnect feedback.
- Release profile-editor text focus before Save & Connect switches tabs, then open the saved connection after the response unwinds.
- Add native About / Support with an offline QR code linking to the dedicated RUWAC donation page.


## v0.4.1 — 2026-10-15 — Stable source release

- Validate compiler-generated bounded relative branch tables during artifact privacy scans; retain rejection of private literals and unknown instruction forms.
- Reattach remote Codex/Claude buttons to their existing RUWAC sessions, serialize simultaneous SSH launches, preserve the running command and refuse ambiguous duplicate names.

## v0.4.0 — 2026-10-15 — Stable source release

- Deliver the compact desktop, scrollable machine library, remote-agent launch configuration, dynamic RDP defaults, fullscreen/local-focus shortcuts and visible CLI status bar as the minor release.

## v0.2.43 — 2026-10-15 — Stable source release

- Keep the CLI status bar visible across the full terminal width and at its physical bottom after resizing, including terminals larger than the daemon PTY limit.
- Replace floating toolbars with a 32px header, adaptive action labels, session Controls popover and optional dock; add six live palettes and terminal font/spacing preferences.
- List all machines in one scrollable grouped list, with persistent collapse state and icon actions for configured endpoints and remote agents.
- Add configurable Codex/Claude resume commands and RUWAC, Screen, tmux or direct remote sessions, plus explicit headless remote RUWAC setup.
- Default new RDP profiles to Dynamic resolution; add local-focus and fullscreen keyboard chords while retaining AltGr input and saved display overrides.

## v0.2.42 — 2026-10-15 — Stable source release

- Add collapsible machine groups with remembered state; search temporarily reveals matching machines.

- Preserve the RDP viewport position across tab focus changes by disabling automatic scrolling of the framebuffer.

- End remote mouse drags when released outside the desktop image or cancelled by GTK, preserving the last valid pointer position; stop held drags before local zoom or allocation changes remap the pointer.

## v0.2.41 — 2026-10-15 — Stable source release

- Make both auto-hide toolbars unmeasured overlays, preserving terminal and RDP viewport dimensions when controls appear.
- Add reviewed bulk import from discovered Remmina profiles with duplicate skipping, stale-review checks and credential separation.

## v0.2.40 — 2026-10-14 — Stable source release

- Add readable Screen-style session listing, detached/shared attachment and explicit session control aliases.
- Add Screen title, chooser, navigation, copy and confirmed window-kill shortcuts; preserve nested literal-prefix routing.
- Collapse workspace/RDP controls behind centered disclosures and keep RDP status in one row.
- Keep the Remmina import action outside scrolling options and wrap narrow-sidebar controls.

## v0.2.39 — 2026-10-14 — Stable source release

- Add `ruwac -r NAME`, independent concurrent attachments, explicit session/window titles and a window status list.
- Forward double Ctrl+A into nested terminals in both TUI and GTK; retain previous-window navigation through Ctrl+A then o and the legacy configurable action.
- Prevent recursive self-attachment and stop new GTK workspaces from silently borrowing CLI sessions.
- Remove the single-pane edge border and preserve ANSI/bold color behavior; report shared viewport dimensions.
- Discover standard Remmina profile locations and preserve explicit, credential-free metadata import.

- Named terminal shortcut: `ruwac -S NAME` creates or reconnects to an exact session name, with duplicate-name and noninteractive safeguards.

## v0.2.38 — 2026-10-14 — Stable source release

- Add administrator-authorized native RPM update controls and a disabled-by-default system timer; verify all installed component assets before one DNF upgrade, preserve sessions and refresh saved opt-out state after timer failures.

## v0.2.37 — 2026-10-14 — Stable source release

- Enforce signed hosted-workflow provenance before extracting or installing compiled updates; hold missing or invalid attestations without changing running sessions.

## v0.2.36 — 2026-10-14 — Stable source release

- Validate exact workflow shell commands and numeric ELF unwind address fields while keeping real private strings and malformed artifact metadata blocked.

## v0.2.35 — 2026-10-13 — Stable source release

- Bound transient publication API retries, measure real RDP pixels against FreeRDP, and reduce connection latency with TCP_NODELAY while preserving authentication and clipboard policy.

## v0.2.34 — 2026-10-13 — Stable source release

- Deliver large native clipboard paste as one bounded PTY operation, with connection-owned staging, cancellation, expiry and legacy-daemon compatibility.

## v0.2.33 — 2026-10-13 — Stable source release

- Qualify clean Fedora desktop RPM dependency resolution and rollback while preserving the daemon, PTY child and private configuration; select the supported Codex browser or prompt-free TUI without overriding user cx commands.

## v0.2.32 — 2026-10-13 — Stable source release

- Bind the archive and both Fedora testing RPMs to one explicit checksum subject manifest, refusing incomplete or mixed-version artifact sets before signing.

## v0.2.31 — 2026-10-13 — Stable source release

- Build separate Fedora 44 testing RPMs through the existing build/release workflow, preserve sessions during package transactions, and keep system RPM updates separate from per-user automatic installations.

## v0.2.30 — 2026-10-13 — Stable source release

- Keep attached terminal clients’ window, pane and zoom selection independent, repair closed targets safely, and preserve shared PTY sizing and mutation deduplication.

## v0.2.29 — 2026-10-13 — Stable source release

- Qualify native PRIMARY copy and middle-click dispatch on a disposable Mutter display, including SGR application mouse routing, Shift bypass and independent desktop/Screen buffers.

## v0.2.28 — 2026-10-13 — Stable source release

- Generate signed GitHub build provenance for Fedora testing archives and verify the repository, workflow, ref and source commit before publication.

## v0.2.27 — 2026-10-13 — Stable source release

- Install the OpenSSL command-line test prerequisite in the hosted Fedora testing-bundle job and verify its availability before compiling certificate fixtures.

## v0.2.26 — 2026-10-13 — Stable source release

- Publish Fedora testing bundles through the existing tagged release workflow and add explicit checked-bundle update delivery without local compilation, preserving source builds, rollback and running sessions.

## v0.2.25 — 2026-10-13 — Stable source release

- Bound text paste and PNG staging reads from stalled desktop clipboard providers, cancel blocked reads on focus/ownership changes, remove partial staging files and keep fresh clipboard reads usable after cancellation.

## v0.2.24 — 2026-10-13 — Stable source release

- Add explicit SSH jump aliases for native SFTP, resumable transfers, rsync and terminal/remote-agent launch, with independent gateway trust and authentication, group inheritance and preserved direct-transfer recovery identities.

## v0.2.23 — 2026-10-13 — Stable source release

- Handle monitor-layout notifications during activation and active sessions, advertise support independently of the graphics pipeline, and correct ClearCodec short-bar coordinates and terminal SRL zero chunks with conformance regressions.

## v0.2.22 — 2026-10-13 — Stable source release

- Add deterministic Fedora testing-bundle assembly with preserved dependency notices, private-content checks and a Git-free installer that verifies every file before running an artifact.

## v0.2.21 — 2026-10-13 — Stable source release

- Prevent unattended updates from undoing a deliberate rollback; preserve running sessions and notification settings, and refuse artifact changes if saving the recovery preference fails.

## v0.2.20 — 2026-10-13 — Stable source release

- Add opt-in unattended Fedora source updates, a reversible per-user timer, exact-commit quality/privacy gates and a first-install bootstrap command while preserving running sessions.

## v0.2.19 — 2026-10-13 — Stable source release

- Sanitize public operational evidence and synthetic examples; enforce privacy checks before publication and in CI while preserving historical release references.

## v0.2.18 — 2026-10-13 — Stable source release

- Reformat the native machine editor with basic connection and login fields first, collapsible advanced settings and a bounded vertical scroll area.
- Add Save & Connect for new and edited profiles using the existing credential and connection path.
- Shield automatic GNOME sign-in from pointer input, hide the greeter until authenticated handover, add Cancel/Escape, and defer dynamic viewport resizing during sign-in.
- Add Open remote agent for Codex/cx or Claude Code/cc in a persistent remote RUWAC session over a selected SSH endpoint.
- Omit CLIPRDR negotiation when desktop clipboard integration is globally disabled.

## v0.2.17 — 2026-10-13 — Stable source release

- Restore automatic RDP clipboard delivery after window focus and clipboard ownership changes, and receive the newest offer when it supersedes an in-flight read.

## v0.2.16 — 2026-10-13 — Stable source release

- Support GNOME image/png CLIPRDR offers, preserve newly copied local content against repeated Receive actions, and prevent compositor-consumed Super chords from opening the remote overview.

## v0.2.15 — 2026-10-13 — Stable source release

- Add authenticated GUI metadata event subscriptions with typed filters, cursor paging, expiry, unsubscribe and explicit history-gap errors, without exposing terminal or clipboard contents.

## v0.2.14 — 2026-10-13 — Stable source release

- Add authenticated remote Codex pane discovery and selection, with a read-only bounded process probe for launcher wrappers and foreground PID checks before attachment.

## v0.2.13 — 2026-10-13 — Stable source release

- Add native clipboard PNG attachment to a confirmed registered SSH/Codex destination, reusing profile credentials and the verified binary transfer with focus/profile cancellation and bounded workers.

## v0.2.12 — 2026-10-13 — Stable source release

- Add opt-in real-host RDP workspace acceptance proving repeated restoration preserves authenticated transport and receives fresh display frames; correct outdated backend/readiness descriptions.

## v0.2.11 — 2026-10-13 — Stable source release

- Add a native saved-workspace picker and restore reopened terminal sessions through their current view identities without restarting shells or enabling automation access.

## v0.2.10 — 2026-10-13 — Stable source release

- Add private named workspace snapshots and authenticated open/restore operations that preserve live views and terminal processes, with lifecycle permissions, deadlines and GUI-instance idempotency.

## v0.2.9 — 2026-10-13 — Stable source release

- Add verified PNG attachment over authenticated SSH to an explicit remote Codex pane, reject malformed transfer receipts, and include temporary source/final copies in the native staging peak quota.

## v0.2.8 — 2026-10-13 — Stable source release

- Add native clipboard PNG attachment to the selected Codex draft, preserve existing daemons through checked legacy input, and retain bounded staged images across frontend close with expiry and honest attachment receipts.

## v0.2.7 — 2026-10-13 — Stable source release

- Add explicit local Codex PNG draft attachment through the authenticated terminal protocol, with foreground/process guards and observed attachment receipts; never submit the user’s draft automatically.

## v0.2.6 — 2026-10-12 — Stable source release

- Add opt-in native GNOME user selection and sign-in using separate saved credentials, fail-closed screen recognition and focus guards; preserve dynamic resolution across the authenticated desktop handover.

## v0.2.5 — 2026-10-12 — Stable source release

- Fix GNOME dynamic-resolution completion status and stale zoom, remove overlapping profile-editor credential controls, and add read-only paged machine discovery through the authenticated CLI/desktop API.

## v0.2.4 — 2026-10-12 — Stable source release

- Add durable endpoint-bound rsync interruption records and native recovery controls, with verified backup restoration, changed-data refusal and actual process-kill acceptance.

## v0.2.3 — 2026-10-12 — Stable source release

- Add native rsync-over-SSH file and folder transfers with literal file lists, verified staging, permissions/timestamps, progress, cancellation and explicit whole-destination replacement.

## v0.2.2 — 2026-10-12 — Stable source release

- Integrate the upstream non-AVC Graphics Pipeline compositor for GNOME RDP modes, preserving the standard Windows renderer and existing clipboard channels.
- Support bounded same-endpoint GNOME RDSTLS handover under verified TLS; validate real Fedora GDM rendering, resolution changes and fresh-transport reconnects.

## v0.2.1 — 2026-10-12 — Stable source release

- Add Fedora 44 GitHub source updates, automatic desktop update checks, explicit local build/install and verified rollback while preserving active sessions.

## v0.2.0 — 2026-10-12 — Stable source release

- Add separate GNOME Remote Login credentials and explicitly confirmed GDM field input, preserving Windows authentication and Secret Service separation.
- Add independent desktop windows, a hideable overlay sidebar with persisted width/pinning, and RDP resize jitter suppression.
- Load the user login shell for new terminals, including when using an older persistent daemon; preserve explicit commands and active sessions.
- Add connection-stage diagnostics and repeat isolated GTK, Secret Service, authenticated RDP and headless regressions; real GNOME GDM and distribution/security qualification remain open.

## v0.1.29 — 2026-10-12 — Prerelease

- Add a private transactional SQLite desktop machine inventory with revision guards, indexed metadata, legacy JSON compatibility and interrupted-publication recovery.
- Preserve readable profiles while refusing writes to malformed or foreign storage; validate editor/recovery behavior with six native GTK acceptance groups and six SQL unit tests.
- Declare Fedora SQLite desktop prerequisites and enforce SQLite-free headless dependencies.

## v0.1.28 — 2026-10-12 — Prerelease

- Preserve canonical italic, dim, conceal, strike, five underline styles and underline colors across GTK/TUI rendering and schema-5 checkpoints with legacy compatibility.
- Avoid redundant terminal paints for unchanged frames and capture private fixture snapshots after paint; validate real fullscreen fzf Unicode and Alt navigation.

## v0.1.27 — 2026-10-12 — Prerelease

- Compact repeated terminal cells in bounded schema-4 checkpoints while retaining plain fallback, legacy recovery and unchanged canonical rendering; validate 128 dense dual-buffer PTYs under a verified memory cap.
- Apply the reader's validation before checkpoint replacement so oversized live glyphs preserve the last readable file and report a recoverable error.
- Strengthen 30-trial CLI benchmark idle qualification with per-operation unrelated CPU accounting, preserving contaminated measurements as unqualified evidence.

## v0.1.26 — 2026-10-12 — Prerelease

- Reload machine profiles saved from another window without typing in search; retain the last valid list on invalid metadata and recover automatically.
- Preserve canonical primary and alternate terminal buffers in bounded checkpoints and expose primary history in native copy controls and the authenticated CLI/API.

## v0.1.25 — 2026-10-12 — Prerelease

- Add native endpoint-bound RDP certificate review, volatile tab approval and saved SHA-256 leaf trust with expiry, changed-certificate confirmation and stale-profile guards. Retain system CA validation by default and keep credential transfer behind the validated peer.
- Add safe certificate/authentication stage metadata and an explicit separate desktop-instance option, preserving older windows and the normal activation behavior.

## v0.1.24 — 2026-10-12 — Prerelease

- Refresh machine rows immediately after Save, Edit and Delete when Tools is collapsed, without typing in search or resetting pagination. Show the compiled version in the window title so older running windows are identifiable.

## v0.1.23 — 2026-10-12 — Prerelease

- Fix Fedora container checkout ownership for subsequent Git checks with an exact workspace trust entry; separate inventory, format, lint, unit tests and each dependency advisory scan in hosted results.

## v0.1.22 — 2026-10-12 — Prerelease

- Repair hosted workflow context validation by initializing temporary build paths through the step environment.
- Add checksum-pinned actionlint validation to catch invalid GitHub workflow contexts before publication.

## v0.1.21 — 2026-10-12 — Prerelease

- Add contributor/security/conduct policies, CODEOWNERS and issue/pull-request templates.
- Add pinned Fedora pull-request checks for formatting, Clippy, unit and real headless fixtures, locked declarations and graphical dependency exclusion.
- Add separately scoped base-only contribution labels with event, checkout and incomplete-file guards.
- Format both Rust workspaces so the full format check can be enforced.

## v0.1.20 — 2026-10-11 — Prerelease

- Bound serialized process metadata pages to 64 KiB, preventing valid long Unicode metadata from exceeding IPC framing limits.
- Read all metadata pages in the native monitor with membership, cursor, count and duplicate-ID guards.
- Validate the full 128-pane capacity and native navigation/search across more than two transport pages.

## v0.1.19 — 2026-10-11 — Prerelease

- Add persistent SFTP pause/resume and explicit partial discard with source, prefix, destination and authenticated host identity verification.
- Recover interrupted overwrite commits and lost rename replies while preserving the original backup.
- Add native pending-transfer management and encrypted SSH key passphrases through Secret Service.
- Validate real OpenSSH transfer loss, concurrent jobs, tamper guards and native credential/resume actions on isolated fixtures.

## v0.1.18 — 2026-10-11 — Prerelease

- Add bounded daemon-owned process metadata and CLI job list/query without arguments, environment values or inferred agent state.
- Add a native searchable process monitor, exact persistent-pane jump and opt-in observed-exit notifications.
- Validate real foreground processes, exit status, crash history, native GTK search/jump and private D-Bus notification delivery.

## v0.1.17 — 2026-10-11 — Prerelease

- Qualify sixteen real mixed GNU Screen/tmux/RUWAC chains at two-to-five layers with common and native tmux prefix configurations.
- Verify Unicode input, literal prefixes, deep window commands, exact resize and same-process detach/reattach using owned PTYs.
- Add a separate verified GNU Screen paste-prefix limitation instead of claiming arbitrary-control paste safety.
- Include the reproducible mixed compatibility matrix in normal tests when test-only tools are available.

## v0.1.16 — 2026-10-11 — Prerelease

- Give saved machines most of the sidebar; group maintenance and Remmina import controls in native collapsed panels.
- Keep creation, search, favorites and paging visible; bound expanded import controls and wrap/select status messages.
- Preserve native CRUD, import review/Cancel/Undo and connection handlers through the grouped layout.
- Validate the allocated layout, all 1,000 endpoints and full authenticated local RDP clipboard/display regressions.

## v0.1.15 — 2026-10-11 — Prerelease

- Add permission-scoped native RDP list/query/open/reconnect/disconnect with stable typed targets, independent focus consent and runtime revocation.
- Resolve saved-profile credentials privately and preserve inactive clipboard/focus ownership during background lifecycle changes.
- Route authenticated GUI commands over the existing verified SSH transport without GUI/daemon autostart or fallback.
- Validate actual local CredSSP/framebuffer/private Secret Service and real SSH GUI forwarding alongside existing native/headless regressions.

## v0.1.14 — 2026-10-11 — Prerelease

- Add accessible 50-profile paging with complete search, preserving endpoint-specific connect/edit/favorite handlers across 1,000 profiles.
- Add native Remmina metadata merge review into an explicitly selected endpoint, preserving identity, credential references and local policy.
- Reject active or changed merge targets; support cancellation, preview invalidation and existing Undo without touching original Remmina files.
- Validate real GTK paging, review/merge persistence, guards and restart alongside full authenticated local RDP and headless regressions.

## v0.1.13 — 2026-10-11 — Prerelease

- Complete bounded headless canonical copy/search with Unicode grapheme marks, rectangular selection, wrapped-line joining, paging and word movement.
- Add terminal chooser, prompt, help, zoom and configurable Control prefixes; contain modal paste/mouse input and preserve buffers on cancellation or limit errors.
- Add immutable authenticated history snapshots with connection ownership, bounded memory, expiry and disconnect reclamation; reuse them for native GTK copy.
- Validate real PTYs and two-to-five-layer RUWAC nesting, history isolation, native GTK and authenticated local RDP regressions through the standard workflow.

## v0.1.12 — 2026-10-11 — Prerelease

- Add a Remmina-inspired native machine sidebar and visible RDP connection/display toolbar with accessible theme icons, retaining existing connection and clipboard handlers.
- Correct Screen double-prefix previous-window navigation in both clients, persist its previous-window metadata with backward checkpoint migration, and expose canonical physical history rows.
- Expand tab placeholders once in copied canonical text while preserving raw cells; validate native RDP reconnect/neighbor ownership, real PTY prefix behavior, recovery and authorized SSH regressions.

## v0.1.11 — 2026-10-11 — Prerelease

- Add private atomic daemon checkpoints and restore the same terminal IDs, split metadata, bounded canonical Unicode/tab/cell history and known exit codes as ended resources.
- Persist ordinary daemon state outside runtime directories; surface checkpoint progress/errors and reject corrupt, unsafe, oversized or concurrently owned archives without discarding them.
- Label restored history in the desktop/TUI/session chooser, preserve internal-buffer separation and allow new live terminals alongside recovered history.
- Validate real daemon SIGKILL/default-state recovery, native GTK history copy, persistence boundaries, checkpoint write timings and headless/GDK/authenticated-RDP regressions; keep full-product and stable qualification gates explicit.

## v0.1.10 — 2026-10-11 — Prerelease

- Add persistent tab/view IDs, exact mixed-view focus, native Focus actions and schema-three workspace migration with preserved original metadata.
- Add authenticated local workspace discovery/listing and explicit ID-based focus/activation with revocable desktop permission, bounded transport and content-free audit records.
- Gate RDP clipboard ownership on native window activation, including authorized background API focus changes.
- Validate graphical identity/crash restoration, permission and malformed/stalled-peer boundaries, CLI-to-GTK latency and terminal/GDK/authenticated-RDP regressions; external/physical acceptance and remaining product gates stay open.

## v0.1.9 — 2026-10-11 — Prerelease

- Add global Alt+1…9/0 workspace tab navigation across terminal, RDP and Files, with modifier guards and missing-tab no-op behavior.
- Introduce a dark native desktop default, optional dark/light/system theme and a compact header with separate workspace actions.
- Add bounded read-only local/SSH diagnostics and explicit foreground-only terminal attribute repair; preserve other terminals and running sessions.
- Validate compositor-delivered shortcuts, isolated terminal repair, actual SSH diagnostics and workspace/clipboard/RDP regressions; external and physical acceptance remain open.

## v0.1.8 — 2026-10-11 — Prerelease

- Add authenticated OpenSSH host control, literal local-file/JSON request delivery and real remote terminal attachment through explicit --host targeting.
- Add read-only API discovery with compatibility for older live daemons; enforce remote response bounds, correlation and process deadlines without graphical dependencies.
- Validate actual Fedora SSH control and persistent PTYs, synthetic malformed/stalled transport cases and existing headless regressions; preserve running sessions.

## v0.1.7 — 2026-10-11 — Prerelease

- Add native individual mixed-view move and close actions; collapse dividers while preserving neighboring terminals, RDP transports and profile protection.
- Validate closing all views, checkpointing an empty workspace and creating a terminal again; preserve daemon processes and same-PID session reopening.
- Validate real authenticated RDP move/close/reconnect clipboard behavior and native SFTP panel movement, refresh and close; retain terminal and headless regressions.

## v0.1.6 — 2026-10-11 — Prerelease

- Add nested native terminal/RDP/Files splits and separation while preserving live PTYs, RDP transports and profile leases; compact RDP controls into a popup.
- Checkpoint and restore all view kinds and split geometry after GUI crashes; preserve schema-1 backups and isolate workspaces by daemon socket.
- Route clipboard ownership by focused canvas and fix toolbar PNG staging with persistent feedback; validate mixed authenticated RDP, workspace, terminal and SFTP regressions.
- Apply owner-approved Apache-2.0 to original source and retain dependency license boundaries.

## v0.1.5 — 2026-10-11 — Prerelease

- Add on-demand authenticated daemon startup, single-instance native desktop activation, terminal session reopening and private workspace restoration.
- Release profile deletion protection when all associated views close; validate sidebar RDP credential lookup and preference persistence with an isolated Secret Service provider.
- Add an immutable per-user installer and desktop launcher through the existing build workflow; validate terminal persistence and exact Unicode input through an authorized Fedora SSH host.
- Preserve full terminal, RDP clipboard and SFTP regressions; publish the complete product acceptance audit and remaining limitations.

## v0.1.4 — 2026-10-11 — Prerelease

- Add native machine/profile editing and Secret Service credential actions, negotiated embedded RDP display control/reactivation with zoom and panning, and verified in-window SSH/SFTP files.
- Validate stable metadata migration/restart, a 1,000-profile GTK library, 20 server-driven resizes, isolated keyring and loopback file integrity/failure handling; preserve terminal and CLIPRDR regressions.

## v0.1.3 — 2026-10-11 — Prerelease

- Complete authenticated embedded IronRDP lifecycle, bidirectional CLIPRDR Unicode/PNG transfer and active-tab ownership gates; add the native machine sidebar and read-only Remmina metadata preview.
- Validate disposable loopback authentication/frame decoding, separate GDK clipboard consumers, directional policies, terminal regressions and matched CLI benchmarks; preserve the independent headless engine.

## v0.1.2 — 2026-10-10 — Prerelease

- Complete native Screen navigation, keyboard marking/search, persistent nested horizontal/vertical splits, ratio resize and zoom; retain daemon-owned sessions.
- Add bounded 50,000-line paged scrollback, coherent history checks, legacy/motion mouse routing and crash-safe PNG staging.
- Embed an asynchronous IronRDP tab with verified TLS, Secret Service reference, native framebuffer, scaling and input/send-key actions; real-host and CLIPRDR acceptance remain open.
- Validate native Vim/less/htop handlers, independent GDK clipboard transfers, lifecycle, startup races and local TLS trust behavior.

## v0.1.1 — 2026-10-10 — Prerelease

- Correct release numbering to bare numeric versions: odd minor beta, even minor stable; preserve earlier suffix tags as historical publications.
- Validate same-channel patch increments, lane transitions, minor promotion and rollover against the S015 reference.

## v0.1.0-beta.2 — 2026-10-10 — Prerelease

- Recover missing GitHub source prerelease pages with a main-branch release trigger and validated earlier-tag recovery.

## v0.1.0-beta.1 — 2026-10-10 — Prerelease

- Add separately compiled Rust GTK4/adwaita frontend, canonical renderer and bounded authenticated workers.
- Connect real GDK clipboard text and image streaming, cancellation, ownership metadata and private staging.
- Validate native Wayland/GNOME fixtures, headless regressions and qualified-scope benchmark observations.
- Restore the standard project build/release workflow, deterministic version assignment, and automated tagged source prerelease.

## Prepared prior candidate — never published

- Add a native GTK canonical-grid feasibility client with independent IPC workers, stable pane targeting and detach-preserving tabs.
- Add named JSON CLI commands, bounded UTF-8 file sends, local package version, and explicit unavailable desktop/RDP errors.
- Audit pinned reusable GTK/session candidates and record measured integration and licensing limits.

- Rename the product, eight crates, CLI, runtime environment and TUI to ruwac.
- Prepare a portable product inventory excluding machine-specific wrappers and private operational files.
- Prepare an unpublished source candidate; desktop, RDP and binary packaging remain unfinished.

## Initial spike — 2026-10-10

- Implement daemon-owned PTYs, framed JSON control, stable targets, Screen prefix navigation and two-client views.
- Validate literal input, startup races, socket isolation, client detach and measured API performance.
- Preserve the previous multiplexer evaluation unchanged.
