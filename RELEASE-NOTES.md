RUWAC v0.4.79 — full-width tabs and bounded test data

Flexible tabs share the whole available row, even with one or two tabs open.
Pinned tabs retain their icon width. Existing font and status-icon sizes,
inactive close buttons and stable geometry during hover, title, status and
selection changes are preserved. Readable minimum widths and overflow arrows
still apply when a row cannot fit all tabs.

The binary privacy scanner recognizes a second bounds-checked LLVM jump-table
form, fixing a numeric-address false positive observed in the published 0.4.78
desktop executable. The full guard, instruction form, read-only table bounds
and every branch target must validate. Actual private text and unknown data
remain subject to the unchanged policy; no artifact or term exception is added.

Successful SFTP regression fixtures remove their generated transfer payloads
while retaining receipts and diagnostics, preventing repeated runs from
accumulating hundreds of megabytes per fixture. Failed fixtures keep their
payloads for diagnosis. Application terminal history is unaffected.

The desktop RSA advisory and maintenance warnings remain visible. Windows
acceptance remains pending on the established R7 path. This patch follows the
existing source, runtime, package and publication gates.
Installing packages preserves running engines and terminal programs; existing
sessions are not migrated or restarted.
