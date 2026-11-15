# Screen and tmux command review — 2026-11-04

Status: **isolated implementation candidate; not released or integrated into
v0.4.57**. Reviewed against source `338c53e`, tmux 3.8 (`list-commands`) and GNU
Screen 5.0.1 (installed Info Command Index). The complete installed command names
are classified below. Newer upstream manuals can contain additional commands;
this is a pinned inventory, not a claim of all-version or full-option parity.

References: [tmux manual](https://man.openbsd.org/tmux),
[Screen manual](https://www.gnu.org/software/screen/manual/).
The online Screen fetch was unavailable during review; the matching installed
manual was used. Command names and independently written scope notes are recorded,
not copies of the manual text.

## First useful increment

These commands reuse existing RPC methods. They add no daemon protocol, render
polling or clipboard synchronization. Local and authenticated `--host` paths use
the same implementation. They never read the invoking shell's tmux target variables
or select a global session implicitly. All replies are RUWAC JSON, not tmux's
format-string or shell-output language.

| RUWAC command | Supported scope |
| --- | --- |
| `list-sessions` | List existing sessions; no options. |
| `has-session -t SESSION` | Exact name or stable session ID; missing/ambiguous names fail. |
| `list-windows -t SESSION` | JSON window IDs, names and panes. |
| `new-window -t SESSION [-n NAME]` | New default shell window; no embedded shell command. |
| `next-window`, `previous-window`, `last-window` | Require `-t SESSION`. |
| `select-window -t window_ID` | Select only that stable window. |
| `rename-window -t window_ID NAME` | One quoted literal name. |
| `split-window -t window_ID [-h|-v]` | `-h` gives side-by-side panes; `-v` (default) gives top/bottom. |
| `select-pane -t pane_ID` | Focus only that stable pane. |
| `kill-pane -t pane_ID` | Close that pane and its program. No broad `-a` flag. |
| `send-keys -t pane_ID -l [--] TEXT...` | Literal text only; arguments concatenate with no added spaces. Nothing appends Enter. Use existing `pane send-key` for a named key. |
| `window rename --window window_ID --name NAME` | Native spelling of rename. |
| `window select --window window_ID` | Native spelling of selection. |
| `pane select`, `pane close` | Require `--id pane_ID`. |
| `buffer set --session session_ID --text TEXT` | Replace the existing single per-session UTF-8 buffer. |
| `buffer set --session session_ID --file FILE` | Read a local UTF-8 file, at most 256 KiB. `--text` and `--file` are exclusive. |
| `buffer show --session session_ID` | JSON text; does not change desktop clipboard. |
| `buffer paste --id pane_ID` | Paste that pane's session buffer; preserve bracketed-paste behavior. |
| `-S SESSION -X stuff TEXT` | Send one quoted literal argument to the active window's active pane. |
| `-S SESSION -X paste` | Paste the active pane's session buffer. |
| `-S SESSION -X split [-v]` | Screen orientation: default top/bottom, `-v` side-by-side. |
| `-S SESSION -X info` | JSON metadata for the selected session/window/pane; no input. |

Native buffer commands support existing short options `-s/-t/-f/-i/-j` where
applicable. Tmux-style commands have only the flags listed above; they do not pass
through the native short-option normalizer. Unknown, repeated, conflicting or
missing flags fail. Window and pane targets must be stable RUWAC IDs, not `%0`,
`@0`, `session:0.1`, fuzzy matches or indices. Screen `-X` still uses its documented
active-window context; targeted Screen `-p` and arbitrary registers are not added.

`send-keys -l` validates the complete UTF-8 payload (no NUL, at most 256 KiB)
before sending one RPC. Text containing newlines/control characters is literal
program input, so review it before sending. Large file input uses native
`pane send-text --file`. Duplicate session names require a stable session ID.
No command silently starts a daemon, recreates an ended target or changes SSH
credentials. New windows/splits use the destination's normal terminal-shell setup.

## Ordered next command increments

1. Bounded `capture-pane`/Screen `hardcopy` and buffer load/save: explicit history
   range, canonical wraps/Unicode, private file creation, no partial export and
   predictable local-file behavior over SSH. Do not silently truncate to 2,000
   rows or substitute workstation localhost/paths.
2. Session rename and pane/window reordering: typed API, persisted checkpoint
   behavior, stable identities, duplicate-name refusal and old-daemon discovery.
3. Useful resize/layout controls and pane move/join/break/swap: reconcile with the
   explicit non-sizing viewer/controller design before changing canonical PTYs.
4. Optional monitor/activity/silence notifications: consume existing revisions,
   bounded overhead, disabled by default; activity does not prove agent completion.
5. Capture/pipe/logging and environment controls only with bounded storage,
   concurrency/cleanup contracts and an explicit real workflow. Never enable
   synchronized typing, shell hooks or persistent logging as a default.

Configuration languages, format expressions, hooks, menus, multiuser ACLs, serial
ports, terminal emulation switches and daemon/session destruction are distinct
projects. Deferred below means intentionally unsupported in this increment, not
that the command was overlooked. Existing features marked equivalent/partial
retain their own qualification boundaries. No complete Screen/tmux emulation claim.

## Complete installed command inventory

Legend: **candidate** = implemented in this isolated increment; **equivalent** =
existing native equivalent; **partial** = related behavior with differing scope or
syntax; **planned** = ordered useful follow-up; **deferred** = outside this increment.

Installed inventory: **91 tmux commands and 184 unique Screen command names**. Aliases shown by tmux and repeated Screen index entries are not counted twice.

### tmux

| Command | Status | RUWAC scope / disposition |
| --- | --- | --- |
| `attach-session` | equivalent | Existing attach/-S/-r/-dmS, detach, kill-session and start; syntax differs. |
| `bind-key` | deferred | No implementation in this increment; separate workflow/design required. |
| `break-pane` | planned | Typed/persistent targets and viewer-safe layout or bounded history contract required. |
| `capture-pane` | partial | Native pane capture/wait, copy/search and one per-session buffer; no named tmux buffers or format parser. |
| `choose-buffer` | deferred | No implementation in this increment; separate workflow/design required. |
| `choose-client` | deferred | No implementation in this increment; separate workflow/design required. |
| `choose-tree` | deferred | No implementation in this increment; separate workflow/design required. |
| `clear-history` | planned | Typed/persistent targets and viewer-safe layout or bounded history contract required. |
| `clear-prompt-history` | deferred | No implementation in this increment; separate workflow/design required. |
| `clock-mode` | deferred | No implementation in this increment; separate workflow/design required. |
| `command-prompt` | deferred | No implementation in this increment; separate workflow/design required. |
| `confirm-before` | deferred | No implementation in this increment; separate workflow/design required. |
| `copy-mode` | partial | Native pane capture/wait, copy/search and one per-session buffer; no named tmux buffers or format parser. |
| `customize-mode` | deferred | No implementation in this increment; separate workflow/design required. |
| `delete-buffer` | planned | Bounded file/buffer/logging design; preserve independent desktop clipboard. |
| `detach-client` | equivalent | Existing attach/-S/-r/-dmS, detach, kill-session and start; syntax differs. |
| `display-menu` | deferred | No implementation in this increment; separate workflow/design required. |
| `display-message` | partial | Native pane/RPC metadata, TUI help/navigation and pane.layout/viewer controls; no full tmux flags. |
| `display-popup` | deferred | No implementation in this increment; separate workflow/design required. |
| `display-panes` | partial | Native pane/RPC metadata, TUI help/navigation and pane.layout/viewer controls; no full tmux flags. |
| `find-window` | planned | Typed/persistent targets and viewer-safe layout or bounded history contract required. |
| `has-session` | candidate | Explicit-target subset above; unsupported flags fail. |
| `if-shell` | deferred | No implementation in this increment; separate workflow/design required. |
| `join-pane` | planned | Typed/persistent targets and viewer-safe layout or bounded history contract required. |
| `kill-pane` | candidate | Explicit-target subset above; unsupported flags fail. |
| `kill-server` | deferred | No implementation in this increment; separate workflow/design required. |
| `kill-session` | equivalent | Existing attach/-S/-r/-dmS, detach, kill-session and start; syntax differs. |
| `kill-window` | deferred | No implementation in this increment; separate workflow/design required. |
| `last-pane` | deferred | No implementation in this increment; separate workflow/design required. |
| `last-window` | candidate | Explicit-target subset above; unsupported flags fail. |
| `link-window` | deferred | No implementation in this increment; separate workflow/design required. |
| `list-buffers` | planned | Bounded file/buffer/logging design; preserve independent desktop clipboard. |
| `list-clients` | partial | Native pane/RPC metadata, TUI help/navigation and pane.layout/viewer controls; no full tmux flags. |
| `list-commands` | deferred | No implementation in this increment; separate workflow/design required. |
| `list-keys` | partial | Native pane/RPC metadata, TUI help/navigation and pane.layout/viewer controls; no full tmux flags. |
| `list-panes` | partial | Native pane/RPC metadata, TUI help/navigation and pane.layout/viewer controls; no full tmux flags. |
| `list-sessions` | candidate | Explicit-target subset above; unsupported flags fail. |
| `list-windows` | candidate | Explicit-target subset above; unsupported flags fail. |
| `load-buffer` | planned | Bounded file/buffer/logging design; preserve independent desktop clipboard. |
| `lock-client` | deferred | No implementation in this increment; separate workflow/design required. |
| `lock-server` | deferred | No implementation in this increment; separate workflow/design required. |
| `lock-session` | deferred | No implementation in this increment; separate workflow/design required. |
| `move-pane` | planned | Typed/persistent targets and viewer-safe layout or bounded history contract required. |
| `move-window` | planned | Typed/persistent targets and viewer-safe layout or bounded history contract required. |
| `new-pane` | deferred | No implementation in this increment; separate workflow/design required. |
| `new-session` | equivalent | Existing attach/-S/-r/-dmS, detach, kill-session and start; syntax differs. |
| `new-window` | candidate | Explicit-target subset above; unsupported flags fail. |
| `next-layout` | planned | Typed/persistent targets and viewer-safe layout or bounded history contract required. |
| `next-window` | candidate | Explicit-target subset above; unsupported flags fail. |
| `paste-buffer` | partial | Native pane capture/wait, copy/search and one per-session buffer; no named tmux buffers or format parser. |
| `pipe-pane` | planned | Bounded file/buffer/logging design; preserve independent desktop clipboard. |
| `previous-layout` | planned | Typed/persistent targets and viewer-safe layout or bounded history contract required. |
| `previous-window` | candidate | Explicit-target subset above; unsupported flags fail. |
| `refresh-client` | partial | Native pane/RPC metadata, TUI help/navigation and pane.layout/viewer controls; no full tmux flags. |
| `rename-session` | planned | Typed/persistent targets and viewer-safe layout or bounded history contract required. |
| `rename-window` | candidate | Explicit-target subset above; unsupported flags fail. |
| `resize-pane` | partial | Native pane/RPC metadata, TUI help/navigation and pane.layout/viewer controls; no full tmux flags. |
| `resize-window` | partial | Native pane/RPC metadata, TUI help/navigation and pane.layout/viewer controls; no full tmux flags. |
| `respawn-pane` | planned | Existing-child identity/environment and explicit replacement contract required. |
| `respawn-window` | planned | Existing-child identity/environment and explicit replacement contract required. |
| `rotate-window` | planned | Typed/persistent targets and viewer-safe layout or bounded history contract required. |
| `run-shell` | deferred | No implementation in this increment; separate workflow/design required. |
| `save-buffer` | planned | Bounded file/buffer/logging design; preserve independent desktop clipboard. |
| `select-layout` | partial | Native pane/RPC metadata, TUI help/navigation and pane.layout/viewer controls; no full tmux flags. |
| `select-pane` | candidate | Explicit-target subset above; unsupported flags fail. |
| `select-window` | candidate | Explicit-target subset above; unsupported flags fail. |
| `send-keys` | candidate | Explicit-target subset above; unsupported flags fail. |
| `send-prefix` | deferred | No implementation in this increment; separate workflow/design required. |
| `server-access` | deferred | No implementation in this increment; separate workflow/design required. |
| `set-buffer` | partial | Native pane capture/wait, copy/search and one per-session buffer; no named tmux buffers or format parser. |
| `set-environment` | planned | Existing-child identity/environment and explicit replacement contract required. |
| `set-hook` | deferred | No implementation in this increment; separate workflow/design required. |
| `set-option` | deferred | No implementation in this increment; separate workflow/design required. |
| `set-window-option` | deferred | No implementation in this increment; separate workflow/design required. |
| `show-buffer` | partial | Native pane capture/wait, copy/search and one per-session buffer; no named tmux buffers or format parser. |
| `show-environment` | planned | Existing-child identity/environment and explicit replacement contract required. |
| `show-hooks` | deferred | No implementation in this increment; separate workflow/design required. |
| `show-messages` | deferred | No implementation in this increment; separate workflow/design required. |
| `show-options` | deferred | No implementation in this increment; separate workflow/design required. |
| `show-prompt-history` | deferred | No implementation in this increment; separate workflow/design required. |
| `show-window-options` | deferred | No implementation in this increment; separate workflow/design required. |
| `source-file` | deferred | No implementation in this increment; separate workflow/design required. |
| `split-window` | candidate | Explicit-target subset above; unsupported flags fail. |
| `start-server` | equivalent | Existing attach/-S/-r/-dmS, detach, kill-session and start; syntax differs. |
| `suspend-client` | deferred | No implementation in this increment; separate workflow/design required. |
| `swap-pane` | planned | Typed/persistent targets and viewer-safe layout or bounded history contract required. |
| `swap-window` | planned | Typed/persistent targets and viewer-safe layout or bounded history contract required. |
| `switch-client` | deferred | No implementation in this increment; separate workflow/design required. |
| `unbind-key` | deferred | No implementation in this increment; separate workflow/design required. |
| `unlink-window` | deferred | No implementation in this increment; separate workflow/design required. |
| `wait-for` | deferred | No implementation in this increment; separate workflow/design required. |

### Screen

| Command | Status | RUWAC scope / disposition |
| --- | --- | --- |
| `Menu` | deferred | No implementation in this increment; separate workflow/design required. |
| `acladd` | deferred | No implementation in this increment; separate workflow/design required. |
| `aclchg` | deferred | No implementation in this increment; separate workflow/design required. |
| `acldel` | deferred | No implementation in this increment; separate workflow/design required. |
| `aclgrp` | deferred | No implementation in this increment; separate workflow/design required. |
| `aclumask` | deferred | No implementation in this increment; separate workflow/design required. |
| `activity` | planned | Useful follow-up with typed targets, bounded data/lifetimes and persistence tests. |
| `addacl` | deferred | No implementation in this increment; separate workflow/design required. |
| `allpartial` | deferred | No implementation in this increment; separate workflow/design required. |
| `altscreen` | deferred | No implementation in this increment; separate workflow/design required. |
| `at` | deferred | No implementation in this increment; separate workflow/design required. |
| `attrcolor` | deferred | No implementation in this increment; separate workflow/design required. |
| `autodetach` | partial | Daemon-owned persistent PTYs and explicit retained/restored history; no Screen option emulation. |
| `autonuke` | deferred | No implementation in this increment; separate workflow/design required. |
| `backtick` | deferred | No implementation in this increment; separate workflow/design required. |
| `bce` | deferred | No implementation in this increment; separate workflow/design required. |
| `bell_msg` | deferred | No implementation in this increment; separate workflow/design required. |
| `bind` | deferred | No implementation in this increment; separate workflow/design required. |
| `bindkey` | deferred | No implementation in this increment; separate workflow/design required. |
| `blanker` | deferred | No implementation in this increment; separate workflow/design required. |
| `blankerprg` | deferred | No implementation in this increment; separate workflow/design required. |
| `break` | deferred | No implementation in this increment; separate workflow/design required. |
| `breaktype` | deferred | No implementation in this increment; separate workflow/design required. |
| `bufferfile` | deferred | No implementation in this increment; separate workflow/design required. |
| `bumpleft` | planned | Useful follow-up with typed targets, bounded data/lifetimes and persistence tests. |
| `bumpright` | planned | Useful follow-up with typed targets, bounded data/lifetimes and persistence tests. |
| `c1` | deferred | No implementation in this increment; separate workflow/design required. |
| `caption` | partial | Related native focus/zoom/viewer/renderer/history/status/UTF-8/keymap behavior; no arbitrary Screen settings. |
| `chacl` | deferred | No implementation in this increment; separate workflow/design required. |
| `charset` | deferred | No implementation in this increment; separate workflow/design required. |
| `chdir` | planned | Useful follow-up with typed targets, bounded data/lifetimes and persistence tests. |
| `cjkwidth` | deferred | No implementation in this increment; separate workflow/design required. |
| `clear` | deferred | No implementation in this increment; separate workflow/design required. |
| `collapse` | planned | Useful follow-up with typed targets, bounded data/lifetimes and persistence tests. |
| `colon` | deferred | No implementation in this increment; separate workflow/design required. |
| `command` | partial | Related native focus/zoom/viewer/renderer/history/status/UTF-8/keymap behavior; no arbitrary Screen settings. |
| `compacthist` | deferred | No implementation in this increment; separate workflow/design required. |
| `console` | deferred | No implementation in this increment; separate workflow/design required. |
| `copy` | equivalent | Existing Screen-style CLI/TUI subset; exact options and interaction differ. |
| `copy_reg` | planned | Useful follow-up with typed targets, bounded data/lifetimes and persistence tests. |
| `crlf` | deferred | No implementation in this increment; separate workflow/design required. |
| `defautonuke` | deferred | No implementation in this increment; separate workflow/design required. |
| `defbce` | deferred | No implementation in this increment; separate workflow/design required. |
| `defbreaktype` | deferred | No implementation in this increment; separate workflow/design required. |
| `defc1` | deferred | No implementation in this increment; separate workflow/design required. |
| `defcharset` | deferred | No implementation in this increment; separate workflow/design required. |
| `defencoding` | deferred | No implementation in this increment; separate workflow/design required. |
| `defescape` | deferred | No implementation in this increment; separate workflow/design required. |
| `defflow` | deferred | No implementation in this increment; separate workflow/design required. |
| `defgr` | deferred | No implementation in this increment; separate workflow/design required. |
| `defhstatus` | deferred | No implementation in this increment; separate workflow/design required. |
| `deflog` | deferred | No implementation in this increment; separate workflow/design required. |
| `deflogin` | deferred | No implementation in this increment; separate workflow/design required. |
| `defmode` | deferred | No implementation in this increment; separate workflow/design required. |
| `defmonitor` | deferred | No implementation in this increment; separate workflow/design required. |
| `defmousetrack` | deferred | No implementation in this increment; separate workflow/design required. |
| `defnonblock` | deferred | No implementation in this increment; separate workflow/design required. |
| `defobuflimit` | deferred | No implementation in this increment; separate workflow/design required. |
| `defscrollback` | deferred | No implementation in this increment; separate workflow/design required. |
| `defshell` | deferred | No implementation in this increment; separate workflow/design required. |
| `defsilence` | deferred | No implementation in this increment; separate workflow/design required. |
| `defslowpaste` | deferred | No implementation in this increment; separate workflow/design required. |
| `defutf8` | deferred | No implementation in this increment; separate workflow/design required. |
| `defwrap` | deferred | No implementation in this increment; separate workflow/design required. |
| `defwritelock` | deferred | No implementation in this increment; separate workflow/design required. |
| `detach` | equivalent | Existing Screen-style CLI/TUI subset; exact options and interaction differ. |
| `digraph` | deferred | No implementation in this increment; separate workflow/design required. |
| `dinfo` | deferred | No implementation in this increment; separate workflow/design required. |
| `displays` | deferred | No implementation in this increment; separate workflow/design required. |
| `dumptermcap` | deferred | No implementation in this increment; separate workflow/design required. |
| `echo` | deferred | No implementation in this increment; separate workflow/design required. |
| `encoding` | partial | Related native focus/zoom/viewer/renderer/history/status/UTF-8/keymap behavior; no arbitrary Screen settings. |
| `escape` | partial | Related native focus/zoom/viewer/renderer/history/status/UTF-8/keymap behavior; no arbitrary Screen settings. |
| `eval` | deferred | No implementation in this increment; separate workflow/design required. |
| `exec` | deferred | No implementation in this increment; separate workflow/design required. |
| `fit` | partial | Related native focus/zoom/viewer/renderer/history/status/UTF-8/keymap behavior; no arbitrary Screen settings. |
| `flow` | deferred | No implementation in this increment; separate workflow/design required. |
| `focus` | partial | Related native focus/zoom/viewer/renderer/history/status/UTF-8/keymap behavior; no arbitrary Screen settings. |
| `focusminsize` | deferred | No implementation in this increment; separate workflow/design required. |
| `gr` | deferred | No implementation in this increment; separate workflow/design required. |
| `group` | deferred | No implementation in this increment; separate workflow/design required. |
| `hardcopy` | planned | Useful follow-up with typed targets, bounded data/lifetimes and persistence tests. |
| `hardcopy_append` | planned | Useful follow-up with typed targets, bounded data/lifetimes and persistence tests. |
| `hardcopydir` | planned | Useful follow-up with typed targets, bounded data/lifetimes and persistence tests. |
| `hardstatus` | partial | Related native focus/zoom/viewer/renderer/history/status/UTF-8/keymap behavior; no arbitrary Screen settings. |
| `height` | partial | Existing viewport/canonical geometry API; respect viewer sizing exclusion. |
| `help` | equivalent | Existing Screen-style CLI/TUI subset; exact options and interaction differ. |
| `history` | deferred | No implementation in this increment; separate workflow/design required. |
| `hstatus` | deferred | No implementation in this increment; separate workflow/design required. |
| `idle` | deferred | No implementation in this increment; separate workflow/design required. |
| `ignorecase` | deferred | No implementation in this increment; separate workflow/design required. |
| `info` | candidate | Screen -X subset above; TUI equivalents remain supported. |
| `ins_reg` | planned | Useful follow-up with typed targets, bounded data/lifetimes and persistence tests. |
| `kill` | equivalent | Existing Screen-style CLI/TUI subset; exact options and interaction differ. |
| `lastmsg` | deferred | No implementation in this increment; separate workflow/design required. |
| `layout` | partial | Related native focus/zoom/viewer/renderer/history/status/UTF-8/keymap behavior; no arbitrary Screen settings. |
| `license` | deferred | No implementation in this increment; separate workflow/design required. |
| `lockscreen` | deferred | No implementation in this increment; separate workflow/design required. |
| `log` | planned | Useful follow-up with typed targets, bounded data/lifetimes and persistence tests. |
| `logfile` | planned | Useful follow-up with typed targets, bounded data/lifetimes and persistence tests. |
| `login` | deferred | No implementation in this increment; separate workflow/design required. |
| `logtstamp` | planned | Useful follow-up with typed targets, bounded data/lifetimes and persistence tests. |
| `mapdefault` | deferred | No implementation in this increment; separate workflow/design required. |
| `mapnotnext` | deferred | No implementation in this increment; separate workflow/design required. |
| `maptimeout` | deferred | No implementation in this increment; separate workflow/design required. |
| `markkeys` | deferred | No implementation in this increment; separate workflow/design required. |
| `meta` | deferred | No implementation in this increment; separate workflow/design required. |
| `monitor` | planned | Useful follow-up with typed targets, bounded data/lifetimes and persistence tests. |
| `mousetrack` | partial | Related native focus/zoom/viewer/renderer/history/status/UTF-8/keymap behavior; no arbitrary Screen settings. |
| `msgminwait` | deferred | No implementation in this increment; separate workflow/design required. |
| `msgwait` | deferred | No implementation in this increment; separate workflow/design required. |
| `multiuser` | deferred | No implementation in this increment; separate workflow/design required. |
| `next` | equivalent | Existing Screen-style CLI/TUI subset; exact options and interaction differ. |
| `nonblock` | deferred | No implementation in this increment; separate workflow/design required. |
| `number` | planned | Useful follow-up with typed targets, bounded data/lifetimes and persistence tests. |
| `obuflimit` | deferred | No implementation in this increment; separate workflow/design required. |
| `only` | partial | Related native focus/zoom/viewer/renderer/history/status/UTF-8/keymap behavior; no arbitrary Screen settings. |
| `other` | equivalent | Existing Screen-style CLI/TUI subset; exact options and interaction differ. |
| `parent` | deferred | No implementation in this increment; separate workflow/design required. |
| `partial` | deferred | No implementation in this increment; separate workflow/design required. |
| `password` | deferred | No implementation in this increment; separate workflow/design required. |
| `paste` | candidate | Screen -X subset above; TUI equivalents remain supported. |
| `pastefont` | deferred | No implementation in this increment; separate workflow/design required. |
| `pow_break` | deferred | No implementation in this increment; separate workflow/design required. |
| `pow_detach` | deferred | No implementation in this increment; separate workflow/design required. |
| `pow_detach_msg` | deferred | No implementation in this increment; separate workflow/design required. |
| `prev` | equivalent | Existing Screen-style CLI/TUI subset; exact options and interaction differ. |
| `printcmd` | deferred | No implementation in this increment; separate workflow/design required. |
| `process` | deferred | No implementation in this increment; separate workflow/design required. |
| `quit` | equivalent | Existing Screen-style CLI/TUI subset; exact options and interaction differ. |
| `readbuf` | planned | Useful follow-up with typed targets, bounded data/lifetimes and persistence tests. |
| `readreg` | planned | Useful follow-up with typed targets, bounded data/lifetimes and persistence tests. |
| `redisplay` | partial | Related native focus/zoom/viewer/renderer/history/status/UTF-8/keymap behavior; no arbitrary Screen settings. |
| `register` | planned | Useful follow-up with typed targets, bounded data/lifetimes and persistence tests. |
| `remove` | deferred | No implementation in this increment; separate workflow/design required. |
| `removebuf` | planned | Useful follow-up with typed targets, bounded data/lifetimes and persistence tests. |
| `rendition` | deferred | No implementation in this increment; separate workflow/design required. |
| `reset` | deferred | No implementation in this increment; separate workflow/design required. |
| `resize` | partial | Related native focus/zoom/viewer/renderer/history/status/UTF-8/keymap behavior; no arbitrary Screen settings. |
| `screen` | equivalent | Existing Screen-style CLI/TUI subset; exact options and interaction differ. |
| `scrollback` | partial | Related native focus/zoom/viewer/renderer/history/status/UTF-8/keymap behavior; no arbitrary Screen settings. |
| `select` | equivalent | Existing Screen-style CLI/TUI subset; exact options and interaction differ. |
| `sessionname` | planned | Useful follow-up with typed targets, bounded data/lifetimes and persistence tests. |
| `setenv` | planned | Useful follow-up with typed targets, bounded data/lifetimes and persistence tests. |
| `setsid` | deferred | No implementation in this increment; separate workflow/design required. |
| `shell` | deferred | No implementation in this increment; separate workflow/design required. |
| `shelltitle` | deferred | No implementation in this increment; separate workflow/design required. |
| `silence` | planned | Useful follow-up with typed targets, bounded data/lifetimes and persistence tests. |
| `silencewait` | planned | Useful follow-up with typed targets, bounded data/lifetimes and persistence tests. |
| `sleep` | deferred | No implementation in this increment; separate workflow/design required. |
| `slowpaste` | deferred | No implementation in this increment; separate workflow/design required. |
| `sorendition` | deferred | No implementation in this increment; separate workflow/design required. |
| `source` | deferred | No implementation in this increment; separate workflow/design required. |
| `split` | candidate | Screen -X subset above; TUI equivalents remain supported. |
| `startup_message` | deferred | No implementation in this increment; separate workflow/design required. |
| `stuff` | candidate | Screen -X subset above; TUI equivalents remain supported. |
| `su` | deferred | No implementation in this increment; separate workflow/design required. |
| `suspend` | deferred | No implementation in this increment; separate workflow/design required. |
| `term` | deferred | No implementation in this increment; separate workflow/design required. |
| `termcap` | deferred | No implementation in this increment; separate workflow/design required. |
| `termcapinfo` | deferred | No implementation in this increment; separate workflow/design required. |
| `terminfo` | deferred | No implementation in this increment; separate workflow/design required. |
| `title` | equivalent | Existing Screen-style CLI/TUI subset; exact options and interaction differ. |
| `umask` | deferred | No implementation in this increment; separate workflow/design required. |
| `unbindall` | deferred | No implementation in this increment; separate workflow/design required. |
| `unsetenv` | planned | Useful follow-up with typed targets, bounded data/lifetimes and persistence tests. |
| `utf8` | partial | Related native focus/zoom/viewer/renderer/history/status/UTF-8/keymap behavior; no arbitrary Screen settings. |
| `vbell` | deferred | No implementation in this increment; separate workflow/design required. |
| `vbell_msg` | deferred | No implementation in this increment; separate workflow/design required. |
| `vbellwait` | deferred | No implementation in this increment; separate workflow/design required. |
| `verbose` | deferred | No implementation in this increment; separate workflow/design required. |
| `version` | partial | Related native focus/zoom/viewer/renderer/history/status/UTF-8/keymap behavior; no arbitrary Screen settings. |
| `wall` | deferred | No implementation in this increment; separate workflow/design required. |
| `width` | partial | Existing viewport/canonical geometry API; respect viewer sizing exclusion. |
| `windowlist` | equivalent | Existing Screen-style CLI/TUI subset; exact options and interaction differ. |
| `windows` | equivalent | Existing Screen-style CLI/TUI subset; exact options and interaction differ. |
| `wrap` | deferred | No implementation in this increment; separate workflow/design required. |
| `writebuf` | planned | Useful follow-up with typed targets, bounded data/lifetimes and persistence tests. |
| `writelock` | deferred | No implementation in this increment; separate workflow/design required. |
| `xoff` | deferred | No implementation in this increment; separate workflow/design required. |
| `xon` | deferred | No implementation in this increment; separate workflow/design required. |
| `zmodem` | deferred | No implementation in this increment; separate workflow/design required. |
| `zombie` | partial | Daemon-owned persistent PTYs and explicit retained/restored history; no Screen option emulation. |
| `zombie_timeout` | partial | Daemon-owned persistent PTYs and explicit retained/restored history; no Screen option emulation. |
