//! Local, deliberate companion setup. All resources travel inside this binary.
use std::{
    io::{Read, Write},
    os::unix::{
        fs::{DirBuilderExt, MetadataExt},
        process::CommandExt,
    },
    path::PathBuf,
    process::{Command, Stdio},
    time::{Duration, Instant},
};
const SCRIPT: &str = include_str!("../../../scripts/primary_setup.py");
const FILES: &[(&str, &str)] = &[
    ("README.md", include_str!("../primary-companion/README.md")),
    ("LICENSE", include_str!("../primary-companion/LICENSE")),
    ("NOTICE", include_str!("../primary-companion/NOTICE")),
    (
        "extension.js",
        include_str!("../primary-companion/extension.js"),
    ),
    ("prefs.js", include_str!("../primary-companion/prefs.js")),
    (
        "metadata.json",
        include_str!("../primary-companion/metadata.json"),
    ),
    (
        "consent_gate.js",
        include_str!("../primary-companion/consent_gate.js"),
    ),
    (
        "consent_state.js",
        include_str!("../primary-companion/consent_state.js"),
    ),
    (
        "consent_preferences.js",
        include_str!("../primary-companion/consent_preferences.js"),
    ),
    (
        "enrollment.js",
        include_str!("../primary-companion/enrollment.js"),
    ),
    (
        "lease_receiver.js",
        include_str!("../primary-companion/lease_receiver.js"),
    ),
    (
        "socket_guard.js",
        include_str!("../primary-companion/socket_guard.js"),
    ),
    (
        "schemas/org.gnome.shell.extensions.ruwac.primary.gschema.xml",
        include_str!(
            "../primary-companion/schemas/org.gnome.shell.extensions.ruwac.primary.gschema.xml"
        ),
    ),
];
struct Stage {
    path: PathBuf,
    device: u64,
    inode: u64,
}
impl Stage {
    fn new() -> Result<Self, String> {
        let runtime = std::env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .ok_or("Run setup from the GNOME desktop")?;
        let info =
            std::fs::symlink_metadata(&runtime).map_err(|_| "Desktop runtime is unavailable")?;
        if !runtime.is_absolute()
            || !info.is_dir()
            || info.uid() != unsafe { libc::geteuid() }
            || info.mode() & 0o077 != 0
        {
            return Err("Desktop runtime must be owned and private".into());
        }
        let path = runtime.join(ruwac_protocol::id("primary_setup"));
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&path)
            .map_err(|_| "Cannot create private setup stage")?;
        let info = std::fs::symlink_metadata(&path).map_err(|_| "Cannot inspect setup stage")?;
        Ok(Self {
            path,
            device: info.dev(),
            inode: info.ino(),
        })
    }
}
impl Drop for Stage {
    fn drop(&mut self) {
        if std::fs::symlink_metadata(&self.path).is_ok_and(|m| {
            m.is_dir()
                && m.uid() == unsafe { libc::geteuid() }
                && m.dev() == self.device
                && m.ino() == self.inode
        }) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }
}
pub fn run(action: &str) -> Result<serde_json::Value, String> {
    if !["install", "status", "preferences", "uninstall"].contains(&action) {
        return Err("Use install, status, preferences or uninstall".into());
    }
    let stage = Stage::new()?;
    let files: std::collections::BTreeMap<_, _> = FILES.iter().copied().collect();
    let packet = serde_json::to_vec(&serde_json::json!({"protocol":1,"version":env!("CARGO_PKG_VERSION"),"files":files,"identity_source":include_str!("../../../scripts/compositor_identity.py")})).map_err(|_| "Invalid embedded companion resources")?;
    let mut child = Command::new("python3")
        .args(["-c", SCRIPT, action])
        .arg(&stage.path)
        .process_group(0)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| "Python3 is required for GNOME companion setup")?;
    let pid = child.id();
    let written = child.stdin.take().unwrap().write_all(&packet);
    let deadline = Instant::now() + Duration::from_secs(75);
    loop {
        // Inspect without reaping: the child PID cannot be reused before we
        // terminate its owned process group, including pack/compiler children.
        let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
        let result = unsafe {
            libc::waitid(
                libc::P_PID,
                pid,
                &mut info,
                libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
            )
        };
        if result != 0 {
            if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            return Err("Cannot observe companion setup process".into());
        }
        if unsafe { info.si_pid() } == pid as i32 || written.is_err() || Instant::now() >= deadline
        {
            unsafe {
                libc::kill(-(pid as i32), libc::SIGKILL);
            }
            let status = child.wait().map_err(|_| "Cannot finish companion setup")?;
            if written.is_err() || Instant::now() >= deadline {
                return Err("Companion setup stopped or timed out".into());
            }
            let mut data = Vec::new();
            child
                .stdout
                .take()
                .unwrap()
                .take(16385)
                .read_to_end(&mut data)
                .map_err(|_| "Cannot read companion setup result")?;
            if data.len() > 16384 {
                return Err("Companion setup response exceeds its bound".into());
            }
            let value: serde_json::Value = serde_json::from_slice(&data)
                .map_err(|_| "Companion setup returned no valid result")?;
            if !status.success() || value["ok"] != true {
                return Err(value["message"]
                    .as_str()
                    .unwrap_or("Companion setup failed")
                    .to_string());
            }
            return Ok(value);
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

pub fn controls() -> gtk::Box {
    use gtk::prelude::*;
    let root = gtk::Box::new(gtk::Orientation::Vertical, 6);
    let description = gtk::Label::new(Some(
        "GNOME 50: install support on this desktop to receive deliberate selection pastes in any app. A new login may be required. Desktop permission and each connection’s approval stay separate.",
    ));
    description.set_wrap(true);
    root.append(&description);
    let status = gtk::Label::new(None);
    status.set_wrap(true);
    status.set_widget_name("primary-setup-status");
    root.append(&status);
    let actions = [
        ("install", "Install on this desktop"),
        ("preferences", "Desktop permission…"),
        ("status", "Check support"),
        ("uninstall", "Remove support"),
    ];
    let buttons: Vec<_> = actions
        .iter()
        .map(|(action, label)| {
            let button = gtk::Button::with_label(label);
            button.set_widget_name(&format!("primary-setup-{action}"));
            root.append(&button);
            button
        })
        .collect();
    for ((action, _), button) in actions.into_iter().zip(&buttons) {
        let all = buttons.clone();
        let status = status.clone();
        button.connect_clicked(move |_| {
            for button in &all { button.set_sensitive(false); }
            status.set_text("Checking this desktop…");
            let mut hold = gtk::gio::Application::default().map(|app| app.hold());
            let (tx, rx) = std::sync::mpsc::sync_channel(1);
            std::thread::spawn(move || {
                // A separate bounded CLI supervises setup even if this window
                // closes. It owns and reaps the entire Python/tool process group.
                let result = std::env::current_exe().map_err(|_| "Cannot locate RUWAC setup command".to_string()).and_then(|exe| {
                    let output = Command::new(exe).args(["--primary-companion", action]).stderr(Stdio::null()).output().map_err(|_| "Cannot start companion setup".to_string())?;
                    serde_json::from_slice::<serde_json::Value>(&output.stdout).map_err(|_| "Companion setup returned no result".to_string())
                });
                let _ = tx.send(result);
            });
            let all = all.clone(); let status = status.clone();
            gtk::glib::timeout_add_local(Duration::from_millis(50), move || {
                let result = match rx.try_recv() {
                    Ok(result) => result,
                    Err(std::sync::mpsc::TryRecvError::Empty) => return gtk::glib::ControlFlow::Continue,
                    Err(_) => Err("Companion setup stopped".into()),
                };
                let message = match result {
                    Err(message) => message,
                    Ok(value) if value["ok"] != true => value["message"].as_str().unwrap_or("Companion setup failed").to_string(),
                    Ok(value) => match value["state"].as_str() {
                        Some("active") => "Support is active. Open Desktop permission to review publication permission.".into(),
                        Some("new-login-required" | "login-or-enable-required") => "Installed. Start a new GNOME login when convenient, then open Desktop permission. This session stays running.".into(),
                        Some("preferences-opened") => "Review permission in the GNOME preferences window.".into(),
                        Some("removed" | "not-installed") => "Support is not installed on this desktop.".into(),
                        _ => "Companion operation completed.".into(),
                    },
                };
                status.set_text(&message); for button in &all { button.set_sensitive(true); }
                hold.take(); gtk::glib::ControlFlow::Break
            });
        });
    }
    root
}

/// Metadata only for the explicitly enabled isolated GUI fixture. Actions are
/// supplied through the existing owned fixture directory, never workspace IPC.
pub fn fixture(parent: &gtk::Window, close: bool) -> serde_json::Value {
    use gtk::prelude::*;
    let dialog = gtk::Window::list_toplevels()
        .into_iter()
        .filter_map(|w| w.downcast::<gtk::Window>().ok())
        .find(|w| {
            w.title().as_deref() == Some("Preferences")
                && w.transient_for().as_ref() == Some(parent)
        });
    let Some(dialog) = dialog else {
        return serde_json::json!({"mapped":false});
    };
    fn scan(widget: &gtk::Widget, window: &gtk::Window, rows: &mut Vec<serde_json::Value>) {
        let name = widget.widget_name();
        if name.starts_with("primary-setup-") {
            let point = widget.compute_point(window, &gtk::graphene::Point::new(0., 0.));
            rows.push(serde_json::json!({"name":name.as_str(),"mapped":widget.is_mapped(),
                "sensitive":widget.is_sensitive(),"size":[widget.width(),widget.height()],
                "origin":point.map(|p|[p.x(),p.y()]),
                "text":widget.downcast_ref::<gtk::Label>().map(|l|l.text().to_string()),
                "label":widget.downcast_ref::<gtk::Button>().and_then(|b|b.label()).map(|s|s.to_string())}));
        }
        let mut child = widget.first_child();
        while let Some(w) = child {
            scan(&w, window, rows);
            child = w.next_sibling();
        }
    }
    let mut rows = Vec::new();
    scan(dialog.upcast_ref(), &dialog, &mut rows);
    let result = serde_json::json!({"mapped":dialog.is_mapped(),"controls":rows,"size":[dialog.width(),dialog.height()]});
    if close {
        dialog.close();
    }
    result
}
