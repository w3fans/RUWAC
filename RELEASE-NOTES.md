RUWAC v0.4.61

A visible + button after the tab strip opens and focuses a new terminal. It stays
available when toolbar actions are collapsed and while many tabs are open, and
uses the existing terminal-creation handler and busy state.

Package updates preserve running terminals and engines. Reopen the GUI to load
this interface change. The installed CLI and running engine may have different
versions; ruwac engine status --json reports the actual engine.

This development patch does not claim completion of the broader physical-input,
external-host or security qualification. The existing RSA advisory remains open
and unsuppressed.
