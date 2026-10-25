# RUWAC mouse selection companion

This optional GNOME Shell 50 extension delivers text selections to native
Wayland applications on an explicitly selected Linux desktop. It uses PRIMARY,
the selection normally pasted with middle-click or a three-finger tap. The
Ctrl+C clipboard and image attachments remain separate.

## Setup on the receiving desktop

Run the matching RUWAC desktop release from that desktop's normal user session:

```sh
ruwac-desktop -G install
ruwac-desktop -G status
ruwac-desktop -G preferences
```

The same actions are in RUWAC's main menu under Preferences. Installation uses
the resources bundled with RUWAC and GNOME's extension tools. It does not download
code, restart Shell, enable publication permission or approve a connection.
If status reports `new-login-required`, save your work and sign out and back in
when convenient. Installation preserves existing unmanaged or modified copies;
use GNOME Extensions to manage those copies.

In companion preferences, enable publication permission for this desktop account.
In the source RUWAC's saved RDP machine, enable **Share mouse selections with this
Linux desktop**, specify the SSH account owning the desktop and its exact Wayland
session, and use a trusted SSH host and key. The RDP service account/password is
separate. Choose **Authorize selection connection…** in the RDP controls and
approve the displayed connection on this receiving desktop. There is no waiting
paste while the approval dialog is open. A new helper needs fresh approval.

Select text normally on the source, focus the destination application in the RDP
desktop, and middle-click. Native publication does not require Xwayland. Receiving
selections back from that desktop additionally needs its configured Xwayland
bridge, Python 3, GTK 4 PyGObject and Xwayland. Both directions follow the RDP tab's
clipboard policy and focus. Native publication targets the approved desktop's
PRIMARY; applications still control whether they support middle-click paste.

## Recovery and removal

**Retry selection connection** replaces a failed selection helper while keeping
RDP connected. Correct the SSH account, trusted host, desktop or temporary session
restriction first, then authorize the new helper. Publication is refused while
the desktop is locked or showing a greeter; returning to the desktop may require
a new helper and approval. Disabling account permission revokes current grants.

```sh
ruwac-desktop -G uninstall
```

Managed removal preserves account preference values and unrelated files. Modified
or unmanaged installations are refused rather than removed. GNOME policy may
prevent installation; readable status and managed removal remain available.

## Boundaries

No clipboard history or selection text is written to disk. The extension does not
read selection contents; it exposes origin revisions and accepts approved text
publications. The client verifies the compositor, account and selected desktop.
Changing tab, focus, source, policy or connection cancels pending input. A newer
remote selection cancels stale publication or its input confirmation. Confirmation
is a point-in-time check; SSH and subsequent RDP input are not an atomic operation.

This companion supports GNOME Shell 50 only. Other Linux desktops use their
available compatibility route; Windows PRIMARY is not supported. A missing
optional Origin method allows publication without observation; permission errors,
endpoint loss, invalid replies and timeouts require retry. Ordinary RDP and Ctrl+C/V
remain independently usable. See the main RUWAC documentation for current release
and qualification status; source presence alone does not mean a binary is released.

The archive includes the Apache-2.0 license and project notice. GNOME Shell, GJS,
GLib and GTK are runtime dependencies supplied by the operating system.
