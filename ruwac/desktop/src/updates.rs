//! GitHub checks, opt-in unattended updates and reversible source installation off the GTK thread.
use adw::prelude::*;
use gtk::glib;
use serde_json::Value;
use std::{cell::Cell, rc::Rc, sync::mpsc, time::Duration};

fn request(args: &[&str]) -> Result<Value, String> {
    let managed =
        std::env::current_exe().is_ok_and(|exe| ruwac_protocol::managed_rpm_installation(&exe));
    let mut helper = std::process::Command::new("python3");
    helper
        .arg("-c")
        .arg(include_str!("../../../scripts/update-user.py"));
    if managed {
        helper.arg("--managed-rpm");
    }
    if let Ok(executable) = std::env::current_exe() {
        helper.arg("--running-executable").arg(executable);
    }
    let output = helper
        .args(["--current", env!("CARGO_PKG_VERSION")])
        .args(args)
        .stderr(std::process::Stdio::null())
        .output()
        .map_err(|_| "Unable to start the update helper (Python 3 is required)".to_owned())?;
    let value: Value = serde_json::from_slice(&output.stdout)
        .map_err(|_| "Update helper returned an invalid response".to_owned())?;
    if output.status.success() {
        Ok(value)
    } else {
        Err(value["error"]
            .as_str()
            .unwrap_or("Update failed")
            .to_owned())
    }
}

fn readiness_label(value: &Value) -> String {
    let mut text = format!(
        "Latest published tag: {}\nLatest verified installable: {}\nInstalled version: {}\nRunning frontend: {}",
        value["latest_published"].as_str().unwrap_or("unknown"),
        value["latest_verified"].as_str().unwrap_or("none"),
        value["installed_version"]
            .as_str()
            .unwrap_or("not installed"),
        value["running"]
            .as_str()
            .unwrap_or(env!("CARGO_PKG_VERSION"))
    );
    if let Some(path) = value["running_executable"].as_str() {
        text.push_str(&format!("\nRunning binary: {path}"));
    }
    if value["mixed_installation"] == true {
        text.push_str(&format!(
            "\nBoth installations detected: system RPM {} · per-user {}",
            value["system_installed"].as_str().unwrap_or("unknown"),
            value["user_installed"].as_str().unwrap_or("unknown")
        ));
    }
    if let Some(pending) = value["skipped_releases"].as_array() {
        for release in pending {
            text.push_str(&format!(
                "\nSkipped {}: {}",
                release["tag"].as_str().unwrap_or("release"),
                release["reason"].as_str().unwrap_or("not ready")
            ));
        }
    }
    if let Some(reason) = value["skipped"].as_str() {
        text.push_str(&format!("\n{reason}"));
    }
    text.push_str(if value["available"] == true {
        "\nVerified update available."
    } else {
        "\nNo newer eligible update."
    });
    text.push_str("\nEligibility checks build results and complete published assets. Downloaded hashes and provenance are verified during installation.");
    text
}

fn task(args: &'static [&'static str], done: impl FnOnce(Result<Value, String>) + 'static) {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(request(args));
    });
    let mut done = Some(done);
    glib::timeout_add_local(Duration::from_millis(200), move || match rx.try_recv() {
        Ok(result) => {
            done.take().unwrap()(result);
            glib::ControlFlow::Break
        }
        Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
        Err(_) => {
            done.take().unwrap()(Err("Update worker stopped".into()));
            glib::ControlFlow::Break
        }
    });
}

pub fn attach(header: &adw::HeaderBar, window: &adw::ApplicationWindow, automatic: bool) {
    let button = gtk::Button::from_icon_name("software-update-available-symbolic");
    button.set_widget_name("ruwac-updates");
    button.set_tooltip_text(Some("Check GitHub for RUWAC updates"));
    header.pack_end(&button);
    if std::env::current_exe().is_ok_and(|exe| ruwac_protocol::distribution_installation(&exe)) {
        // Distribution packages never contact GitHub or start RUWAC's updater.
        button.set_tooltip_text(Some(ruwac_protocol::DISTRIBUTION_UPDATE_HINT));
        let weak_window = window.downgrade();
        button.connect_clicked(move |_| {
            if let Some(window) = weak_window.upgrade() {
                let dialog = gtk::Window::builder()
                    .title("RUWAC updates")
                    .transient_for(&window)
                    .modal(true)
                    .default_width(480)
                    .build();
                let label = gtk::Label::new(Some(ruwac_protocol::DISTRIBUTION_UPDATE_HINT));
                label.set_wrap(true);
                label.set_widget_name("update-status");
                label.set_margin_top(18);
                label.set_margin_bottom(18);
                label.set_margin_start(18);
                label.set_margin_end(18);
                dialog.set_child(Some(&label));
                dialog.present();
            }
        });
        return;
    }
    let weak_window = window.downgrade();
    button.connect_clicked(move |_| {
        let Some(window) = weak_window.upgrade() else { return };
        let dialog = gtk::Window::builder().title("RUWAC updates").transient_for(&window)
            .modal(true).default_width(480).build();
        let content = gtk::Box::new(gtk::Orientation::Vertical, 12);
        content.set_margin_top(18); content.set_margin_bottom(18); content.set_margin_start(18); content.set_margin_end(18);
        let status = gtk::Label::new(Some("Checking GitHub…"));
        status.set_wrap(true); status.set_widget_name("update-status");
        content.append(&status);
        if std::env::current_exe().is_ok_and(|exe| ruwac_protocol::managed_rpm_installation(&exe)) {
            status.set_text("Checking GitHub for RPM updates…");
            let detail = gtk::Label::new(Some("Installed with RPM. System updates verify the GitHub release build before using DNF. These are upstream testing packages; the known dependency advisory remains open. Administrator authorization is required to install or enable updates. Running sessions remain open."));
            detail.set_wrap(true); content.append(&detail);
            content.append(&gtk::LinkButton::with_label("https://github.com/w3fans/RUWAC/releases/latest", "Open RPM releases"));
            let install = gtk::Button::with_label("Install verified RPM update…");
            install.set_widget_name("update-install"); install.set_sensitive(false);
            content.append(&install);
            let auto = gtk::CheckButton::with_label("Automatically install verified upstream testing RPMs");
            auto.set_widget_name("update-auto-install"); auto.set_sensitive(false);
            content.append(&auto);
            let changing = Rc::new(Cell::new(false));
            let initial_guard = changing.clone();
            let weak_auto = auto.downgrade(); let weak_status = status.downgrade();
            task(&["--auto-install", "status"], move |result| {
                if let Some(auto) = weak_auto.upgrade() {
                    match result {
                        Ok(value) => { initial_guard.set(true); auto.set_active(value["automatic_install"] == true); initial_guard.set(false); auto.set_sensitive(true); }
                        Err(error) => { if let Some(status) = weak_status.upgrade() { status.set_text(&error); } }
                    }
                }
            });
            let switch_guard = changing.clone(); let weak_status = status.downgrade();
            auto.connect_toggled(move |auto| {
                if switch_guard.replace(true) { return; }
                let enabled = auto.is_active(); let weak_auto = auto.downgrade();
                let guard = switch_guard.clone(); let weak_status = weak_status.clone();
                auto.set_sensitive(false);
                task(if enabled { &["--auto-install", "on"] } else { &["--auto-install", "off"] }, move |result| {
                    if let Err(error) = &result {
                        if let Some(status) = weak_status.upgrade() { status.set_text(error); }
                        task(&["--auto-install", "status"], move |saved| {
                            if let Some(auto) = weak_auto.upgrade() {
                                if let Ok(value) = saved {
                                    auto.set_active(value["automatic_install"] == true);
                                    auto.set_sensitive(true);
                                } else {
                                    auto.set_tooltip_text(Some("Unable to determine saved system update state"));
                                }
                            }
                            guard.set(false);
                        });
                        return;
                    }
                    if let Some(auto) = weak_auto.upgrade() {
                        match &result {
                            Ok(value) => { auto.set_active(value["automatic_install"] == true); }
                            Err(_) => { auto.set_active(!enabled); }
                        }
                        auto.set_sensitive(true);
                    }
                    if let Some(status) = weak_status.upgrade() {
                        status.set_text(match &result {
                            Ok(_) if enabled => "System RPM updates enabled. Checks every six hours; running sessions are preserved.",
                            Ok(_) => "System RPM automatic updates disabled.",
                            Err(error) => error,
                        });
                    }
                    guard.set(false);
                });
            });
            let weak_status = status.downgrade();
            install.connect_clicked(move |button| {
                button.set_sensitive(false); let weak_button = button.downgrade();
                let weak_status = weak_status.clone();
                task(&["--install"], move |result| {
                    if let Some(status) = weak_status.upgrade() {
                        status.set_text(&match result {
                            Ok(value) if value["installed"].is_string() => format!("Installed RPM {}. Existing sessions remain open; restart the frontend when ready.", value["installed"].as_str().unwrap_or("update")),
                            Ok(value) => value["skipped"].as_str().unwrap_or("The system RPM installation is current.").into(),
                            Err(error) => error,
                        });
                    }
                    if let Some(button) = weak_button.upgrade() { button.set_sensitive(true); }
                });
            });
            let label = status.downgrade();
            let weak_install = install.downgrade();
            task(&["--check"], move |result| {
                if let Some(install) = weak_install.upgrade() { install.set_sensitive(result.as_ref().is_ok_and(|value| value["available"] == true)); }
                if let Some(label) = label.upgrade() {
                    label.set_text(&match result {
                        Ok(value) => readiness_label(&value),
                        Err(error) => format!("Unable to check RPM releases: {error}"),
                    });
                }
            });
            dialog.set_child(Some(&content)); dialog.present(); return;
        }
        content.append(&gtk::Label::new(Some("Fedora 44 · builds published source locally\nPrevious installation and running sessions are retained.")));
        let install = gtk::Button::with_label("Build and install update");
        install.set_widget_name("update-install"); install.set_sensitive(false);
        content.append(&install);
        let busy = Rc::new(Cell::new(false));
        let busy_install = busy.clone(); let label = status.clone();
        install.connect_clicked(move |button| {
            if busy_install.replace(true) { return; }
            button.set_sensitive(false);
            label.set_text("Downloading or building the selected update… Existing windows keep running.");
            let label = label.downgrade(); let busy = busy_install.clone(); let retry = button.downgrade();
            task(&["--install"], move |result| {
                busy.set(false);
                if result.is_err() || result.as_ref().is_ok_and(|v| v["skipped"].is_string()) { if let Some(button) = retry.upgrade() { button.set_sensitive(true); } }
                if let Some(label) = label.upgrade() { label.set_text(&match result {
                    Ok(value) if value["skipped"].is_string() => format!("Update pending: {}", value["skipped"].as_str().unwrap()),
                    Ok(value) if value["installed"].is_string() => format!("Installed {}. Close and reopen the frontend when ready; terminal sessions remain running.", value["installed"].as_str().unwrap()),
                    Ok(value) => readiness_label(&value),
                    Err(error) => format!("Update failed: {error}\nPrevious installation remains available. Use ruwac update --install in a terminal for build diagnostics."),
                }); }
            });
        });
        let auto_install = gtk::CheckButton::with_label("Install GitHub updates automatically (Fedora 44)");
        let changing = Rc::new(Cell::new(false));
        let rollback = gtk::Button::with_label("Restore previous installation");
        rollback.set_widget_name("update-rollback"); rollback.set_sensitive(false);
        let rollback_auto = auto_install.downgrade();
        let rollback_changing = changing.clone();
        let label = status.clone(); let busy_rollback = busy.clone();
        rollback.connect_clicked(move |button| {
            if rollback_auto.upgrade().is_some_and(|check| !check.is_sensitive()) { return; }
            if busy_rollback.replace(true) { return; }
            button.set_sensitive(false);
            if let Some(check) = rollback_auto.upgrade() { check.set_sensitive(false); }
            let label = label.downgrade(); let busy = busy_rollback.clone();
            let rollback_auto = rollback_auto.clone(); let changing = rollback_changing.clone();
            let rollback_button = button.downgrade();
            task(&["--rollback"], move |result| {
                busy.set(false);
                if let Some(button) = rollback_button.upgrade() { button.set_sensitive(true); }
                if let Some(check) = rollback_auto.upgrade() { check.set_sensitive(true); }
                if result.is_ok() {
                    if let Some(check) = rollback_auto.upgrade() {
                        changing.set(true); check.set_active(false); changing.set(false);
                    }
                }
                if let Some(label) = label.upgrade() { label.set_text(&match result {
                    Ok(value) => format!("Restored {}. Automatic installation disabled; re-enable it after resolving the regression. Running windows and sessions remain unchanged.", value["installed"].as_str().unwrap_or("previous version")),
                    Err(error) => format!("Rollback refused: {error}"),
                }); }
            });
        });
        content.append(&rollback);
        let auto = gtk::CheckButton::with_label("Check GitHub automatically at startup and daily");
        auto.set_widget_name("update-auto-check"); auto.set_sensitive(false);
        content.append(&auto);
        let a = auto.downgrade();
        task(&["--auto-check", "status"], move |result| {
            if let (Some(a), Ok(value)) = (a.upgrade(), result) {
                a.set_active(value["automatic_check"] == true); a.set_sensitive(true);
                a.connect_toggled(|a| {
                    let weak = a.downgrade();
                    task(if a.is_active() { &["--auto-check", "on"] } else { &["--auto-check", "off"] }, move |result| {
                        if result.is_err() { if let Some(a) = weak.upgrade() { a.set_sensitive(false); a.set_tooltip_text(Some("Unable to save update preference; existing settings were preserved")); } }
                    });
                });
            }
        });
        auto_install.set_widget_name("update-auto-install");
        auto_install.set_sensitive(false);
        content.append(&auto_install);
        let testing = gtk::CheckButton::with_label("Use Fedora testing bundles (skip local compilation)");
        testing.set_widget_name("update-testing-bundles"); testing.set_sensitive(false);
        testing.set_tooltip_text(Some("Fedora 44 x86_64 testing artifacts; source/quality checks and hashes are verified. These are unsigned and retain the known dependency advisory. Source builds remain the default."));
        content.append(&testing);
        let weak_testing = testing.downgrade(); let testing_status = status.downgrade();
        let testing_changing = Rc::new(Cell::new(false));
        task(&["--testing-bundles", "status"], move |result| {
            if let (Some(check), Ok(value)) = (weak_testing.upgrade(), result) {
                check.set_active(value["testing_bundles"] == true); check.set_sensitive(true);
                check.connect_toggled(move |check| {
                    if testing_changing.get() { return; }
                    check.set_sensitive(false); let weak = check.downgrade(); let label = testing_status.clone();
                    let changing = testing_changing.clone();
                    let args: &'static [&'static str] = if check.is_active() { &["--testing-bundles", "on"] } else { &["--testing-bundles", "off"] };
                    task(args, move |result| {
                        if let Some(label) = label.upgrade() { label.set_text(&match result {
                            Ok(value) if value["testing_bundles"] == true => "Fedora testing bundle delivery enabled. Updates wait for checked assets; source compilation is skipped.".into(),
                            Ok(_) => "Source-build delivery enabled.".into(),
                            Err(error) => format!("Unable to save update delivery: {error}"),
                        }); }
                        task(&["--testing-bundles", "status"], move |saved| {
                            if let Some(check) = weak.upgrade() {
                                if let Ok(value) = saved { changing.set(true); check.set_active(value["testing_bundles"] == true); changing.set(false); check.set_sensitive(true); }
                            }
                        });
                    });
                });
            }
        });
        let detail = gtk::Label::new(Some("Checks every six hours while logged in. Uses your selected delivery mode; the next application restart uses the update. Running windows and terminal sessions stay open."));
        detail.set_wrap(true);
        content.append(&detail);
        let weak_auto = auto_install.downgrade();
        let update_status = status.downgrade();
        let ready_rollback = rollback.downgrade();
        task(&["--auto-install", "status"], move |result| {
            if let (Some(check), Ok(value)) = (weak_auto.upgrade(), result) {
                check.set_active(value["automatic_install"] == true);
                check.set_sensitive(true);
                if let Some(button) = ready_rollback.upgrade() { button.set_sensitive(true); }
                check.connect_toggled(move |check| {
                    if changing.get() { return; }
                    check.set_sensitive(false);
                    let weak_check = check.downgrade();
                    let changing = changing.clone();
                    let label = update_status.clone();
                    let args: &'static [&'static str] = if check.is_active() { &["--auto-install", "on"] } else { &["--auto-install", "off"] };
                    task(args, move |result| {
                        if let Some(label) = label.upgrade() {
                            label.set_text(&match &result {
                                Ok(value) if value["automatic_install"] == true => "Automatic installation enabled. Checks every six hours; running sessions are preserved.".into(),
                                Ok(_) => "Automatic installation disabled. Existing sessions and installations are preserved.".into(),
                                Err(error) => format!("Unable to configure automatic installation: {error}"),
                            });
                        }
                        // Query persisted state even after a scheduler error: opting
                        // out remains committed when a stopped manager rejects disable.
                        task(&["--auto-install", "status"], move |saved| {
                            if let Some(check) = weak_check.upgrade() {
                                if let Ok(value) = saved {
                                    changing.set(true);
                                    check.set_active(value["automatic_install"] == true);
                                    changing.set(false);
                                    check.set_sensitive(true);
                                }
                            }
                        });
                    });
                });
            }
        });
        dialog.set_child(Some(&content)); dialog.present();
        let label = status.downgrade(); let install = install.downgrade();
        task(&["--check"], move |result| { if let Some(label) = label.upgrade() { match result {
            Ok(value) => {
                let available = value["available"] == true;
                label.set_text(&readiness_label(&value));
                if let Some(install) = install.upgrade() { install.set_sensitive(available); }
            }
            Err(error) => label.set_text(&format!("Unable to check: {error}")),
        } } });
    });
    if automatic {
        let check_button = button.downgrade();
        let check = move || {
            let weak = check_button.clone();
            task(&["--check-auto"], move |result| {
                if let (Some(button), Ok(value)) = (weak.upgrade(), result) {
                    if value["available"] == true {
                        button.add_css_class("suggested-action");
                        button.set_tooltip_text(Some(&format!(
                            "RUWAC {} is available — check updates",
                            value["latest"].as_str().unwrap_or("update")
                        )));
                    } else {
                        button.remove_css_class("suggested-action");
                    }
                }
            });
        };
        check();
        glib::timeout_add_local(Duration::from_secs(86400), move || {
            check();
            glib::ControlFlow::Continue
        });
    }
}

/// Widget callbacks used only by the explicitly private desktop fixture dispatcher.
pub fn fixture(window: &adw::ApplicationWindow, command: &Value) -> Value {
    fn find(root: &gtk::Widget, name: &str) -> Option<gtk::Widget> {
        if root.widget_name() == name {
            return Some(root.clone());
        }
        let mut child = root.first_child();
        while let Some(widget) = child {
            if let Some(found) = find(&widget, name) {
                return Some(found);
            }
            child = widget.next_sibling();
        }
        None
    }
    if command["operation"] == "open" {
        if let Some(button) = find(window.upcast_ref(), "ruwac-updates")
            .and_then(|w| w.downcast::<gtk::Button>().ok())
        {
            button.emit_clicked();
        }
    }
    for widget in gtk::Window::list_toplevels() {
        if let Ok(dialog) = widget.downcast::<gtk::Window>() {
            if dialog.title().as_deref() != Some("RUWAC updates") {
                continue;
            }
            if let Some(name) = command["click"].as_str() {
                if let Some(button) =
                    find(dialog.upcast_ref(), name).and_then(|w| w.downcast::<gtk::Button>().ok())
                {
                    if button.is_sensitive() {
                        button.emit_clicked();
                    }
                }
            }
            if let Some(enabled) = command["automatic"].as_bool() {
                if let Some(button) = find(dialog.upcast_ref(), "update-auto-check")
                    .and_then(|w| w.downcast::<gtk::CheckButton>().ok())
                {
                    if button.is_sensitive() {
                        button.set_active(enabled);
                    }
                }
            }
            if let Some(enabled) = command["testing_bundles"].as_bool() {
                if let Some(check) = find(dialog.upcast_ref(), "update-testing-bundles")
                    .and_then(|w| w.downcast::<gtk::CheckButton>().ok())
                {
                    check.set_active(enabled);
                }
            }
            if let Some(enabled) = command["automatic_install"].as_bool() {
                if let Some(button) = find(dialog.upcast_ref(), "update-auto-install")
                    .and_then(|w| w.downcast::<gtk::CheckButton>().ok())
                {
                    if button.is_sensitive() {
                        button.set_active(enabled);
                    }
                }
            }
            let automatic_install = find(dialog.upcast_ref(), "update-auto-install")
                .and_then(|w| w.downcast::<gtk::CheckButton>().ok())
                .map(|w| w.is_active());
            let automatic_install_enabled =
                find(dialog.upcast_ref(), "update-auto-install").is_some_and(|w| w.is_sensitive());
            let rollback_enabled =
                find(dialog.upcast_ref(), "update-rollback").is_some_and(|w| w.is_sensitive());
            let status = find(dialog.upcast_ref(), "update-status")
                .and_then(|w| w.downcast::<gtk::Label>().ok())
                .map(|label| label.text().to_string());
            let install =
                find(dialog.upcast_ref(), "update-install").is_some_and(|w| w.is_sensitive());
            let automatic = find(dialog.upcast_ref(), "update-auto-check")
                .and_then(|w| w.downcast::<gtk::CheckButton>().ok())
                .map(|w| w.is_active());
            let testing_bundles = find(dialog.upcast_ref(), "update-testing-bundles")
                .and_then(|w| w.downcast::<gtk::CheckButton>().ok())
                .is_some_and(|w| w.is_active());
            let testing_bundles_enabled = find(dialog.upcast_ref(), "update-testing-bundles")
                .is_some_and(|w| w.is_sensitive());
            let result = serde_json::json!({"testing_bundles":testing_bundles,"testing_bundles_enabled":testing_bundles_enabled,"dialog":true,"status":status,"install_enabled":install,"automatic":automatic,"automatic_install":automatic_install,"automatic_install_enabled":automatic_install_enabled,"rollback_enabled":rollback_enabled});
            if command["operation"] == "close" {
                dialog.close();
            }
            return result;
        }
    }
    serde_json::json!({"dialog":false})
}
