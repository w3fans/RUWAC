# GNOME Remote Login and desktop windows

In Machines → Edit, choose Standard / Windows RDP, GNOME Remote Login, or GNOME Desktop Sharing. RDP Service Login retains the transport username/password. GNOME User Login stores the Linux username and a separate Secret Service password reference. Set/Replace/Forget applies only to its corresponding password. Switching modes retains nonsecret metadata; Windows and Desktop Sharing ignore the second credential.

For supported English Fedora GDM screens, enable **Sign in automatically to the saved Linux account** under GNOME User Login, save, and Connect. Automatic sign-in stays bound to that tab's own verified connection, including in background tabs, and pauses your input to that connection until it finishes. RUWAC uses Not listed to select the saved Linux account, recognizes the focused password field, sends its saved password and submits through native RDP input. It reports completion only after a subsequent authenticated desktop handover and newly rendered framebuffer. This is a client input sequence; it does not alter GDM or enable system-wide autologin.

Automatic mode defaults off. Its optional runtime tools on Fedora 44 are `python3-pillow`, `tesseract` and `tesseract-langpack-eng`; install them through the machine's ordinary package workflow. They are needed only by desktop login recognition, not the headless daemon or CLI. OCR runs locally on a bounded, private canonical remote image and returns geometry only. Missing tools, unknown/ambiguous fields, changed profiles and explicit cancellation stop sign-in. Switching tabs does not redirect automatic input. A changed connection generation before password submission restarts recognition on the new screen. The Linux password is fetched only after recognizing the intended focused password field. No password enters OCR, profile JSON/SQLite, logs, CLI arguments, clipboard or workspace responses. One password attempt is sent; a prompt without a new authenticated desktop is not success. Already sent input cannot be recalled.

RDP recovery counts setup time and backoff against a one-minute foreground window. An unavailable server then gets quiet retries every 15 seconds, each with at most 10 seconds of setup. **Reconnect** starts a fresh attempt using the saved endpoint without recreating the tab. Authentication or certificate failures still require review rather than automatic retries. The login-recognition timer starts with a connected greeter, not time spent waiting for the network. A completed automatic login can run again only on the same transport after a later generation and a new authenticated GNOME service handover, with the saved profile still current and automatic login still enabled. A cancelled/failed sequence does not acquire this restart permission. A stable connection renews the handover loop limit; immediate redirect loops remain bounded.

A real authorized Fedora 44 host passed automatic Linux user selection/sign-in and desktop rendering, with isolated synthetic local GTK/Wayland controls. GNOME Remote Desktop is 50.2; the installed GDM package is 50.3 and the still-running GDM Manager reports 50.1. Other locales/themes, broader real-host background/reconnection behavior, wrong real GDM passwords, physical input and external clipboard remain unqualified. Closing older profile-editor windows before enabling the new preference avoids strict older readers rejecting newly introduced metadata. Existing profiles without the preference remain compatible.

For unsupported screens, use **Assisted GNOME Sign-in** in the toolbar or RDP controls. Place the caret in the intended GDM field, verify the selected endpoint/tab/login screen, confirm Username or Password, and submit/advance yourself. Assistance does not independently recognize or complete login. Never send saved credentials to an unexpected screen.

[GNOME's configuration guide](https://github.com/GNOME/gnome-remote-desktop/blob/50.2/docs/configuration.md) describes the system-wide service login followed by graphical user login. IronRDP's single Client Info autologon credential is not this separately verified Linux login sequence. No general client-controlled GDM second-stage authentication API was verified; administrator CreateUserDisplay and server-wide autologin are not used.

Use New window for independent saved workspaces sharing the persistent daemon. Alt+1–9/0 switches tabs within each window. Live RDP transports stay in their process. On owner-authorized publication, the installed latest frontend replaces the older owner frontend after open dialogs close, preserving daemon PTYs and saved layouts.

Machines overlays content by default. Alt+M opens Machines search; Escape/tab selection hides it. Preferences controls width and pinning. Dynamic resolution starts at 100%; manual zoom switches to Fit, and received frames acknowledge pending resolution requests. New terminals use the configured user login shell; existing sessions are preserved.

A small “Not listed?” button with a low-confidence full-layout OCR result is
rechecked in a tight local crop. Only a single complete link meeting the original
confidence threshold is accepted. Low-confidence text only locates that crop;
it cannot authorize typing, bypass an unknown focused field, or skip the ordered
username/password sequence. This recognizer regression is separate from physical
acceptance on an owner’s remote login screen.

Read-only RDP metadata reports the automatic sign-in stage and last recognized
screen kind. It contains neither captured images nor OCR text or credentials,
so a stopped user-list/username/password transition can be diagnosed separately
from transport or credential availability.

The workspace description also reports `gui_version` for the running GUI.
An installed CLI version does not identify an already-open GUI process.

Explicit retry is also available from the RDP tab menu. A stopped attempt may be retried on a later verified connected greeter handover; this requires a decoded frame and verified TLS without transport authentication errors. It resets screen recognition and does not automatically repeat a submitted password. Reconnect reloads saved selection/sign-in options only for the same saved host, service user, credential reference and trust; changed transport identity requires reopening the saved machine.
