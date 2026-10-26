Automatic GNOME sign-in now sends its recognized user-list click while keeping
manual input paused. Failed one-use session handover setup stops instead of silently
starting another service login. Cancelling sign-in also stops transport retries;
reconnecting waits for the previous connection to close. Late credential lookups
do not cancel a newer attempt. Redirect setup has a
10-second limit. Established desktops retain ordinary network recovery, preserving
existing remote applications.

Machines search uses Alt+M; Ctrl+K reaches terminal applications such as nano.
Mouse selection preserves matching live-screen colours and text attributes,
repaints without erasing the whole terminal, and keeps the light-blue footer.
Older scrollback without style metadata remains plain text. GUI terminal cell
backgrounds have continuous edges at fractional font widths and display scales.

Contribution labeling now reports bounded GitHub error diagnostics while retaining
trusted-base execution, existing token permissions and the original HTTP failure.
Recognized error messages, request IDs and accepted permission fields help diagnose
refusals; arbitrary response bodies and authorization headers are never printed.
This does not claim to resolve the observed labeling403 before the updated workflow
has actually run.

The acceptance ledger now distinguishes delivered v0.4.43 results from remaining
physical, external-host, endurance and security qualification. Known
RUSTSEC-2023-0071 remains unsuppressed; isolated tests are not a complete product
or platform qualification claim.
