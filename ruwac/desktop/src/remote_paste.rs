//! Ctrl+V with an image in an SSH tab: a program on the remote host (Codex,
//! Claude) cannot read this computer's clipboard, so the PNG is copied over
//! the tab's own SSH endpoint into a private remote folder and its remote
//! path is pasted, which those agents attach as an image.
use crate::{profiles::Profile, view::View};
use gtk::{gdk, glib, prelude::*};
use std::{
    cell::RefCell,
    collections::HashMap,
    rc::Rc,
    time::{Duration, Instant},
};

thread_local! {
    // Explicitly configured fixtures only. Product launches use the private catalog.
    static TARGETS: RefCell<HashMap<(String,String),(u32,Profile)>> = RefCell::new(HashMap::new());
}
fn pane_pid(view: &View, pane: &str) -> Option<u32> {
    let frame = view.frame.borrow();
    let frame = frame.as_ref()?;
    let row = frame["panes"]
        .as_array()?
        .iter()
        .find(|p| p["id"] == pane && p["ended"] != true && p["restored"] != true)?;
    u32::try_from(row["pid"].as_u64()?).ok()
}
fn target_profile(view: &View, pane: &str) -> Option<Profile> {
    let pid = pane_pid(view, pane)?;
    crate::launches::image_profile(&view.session, pane, pid).or_else(|| {
        TARGETS.with(|t| {
            t.borrow()
                .get(&(view.session.clone(), pane.into()))
                .filter(|(p, _)| *p == pid)
                .map(|(_, p)| p.clone())
        })
    })
}

#[derive(Clone)]
struct Configuration {
    stage: Option<std::path::PathBuf>,
    reader: crate::profiles::ImageProfileReader,
}
thread_local! { static CONFIGURATION: RefCell<Option<Configuration>> = const { RefCell::new(None) }; }
pub fn configure(stage: Option<std::path::PathBuf>, reader: crate::profiles::ImageProfileReader) {
    CONFIGURATION.with(|v| *v.borrow_mut() = Some(Configuration { stage, reader }));
}
static WORKERS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
struct Permit;
impl Permit {
    fn acquire() -> Result<Self, String> {
        WORKERS
            .fetch_update(
                std::sync::atomic::Ordering::AcqRel,
                std::sync::atomic::Ordering::Acquire,
                |n| (n < 2).then_some(n + 1),
            )
            .map_err(|_| "Two image pastes are already in progress")?;
        Ok(Self)
    }
}
impl Drop for Permit {
    fn drop(&mut self) {
        WORKERS.fetch_sub(1, std::sync::atomic::Ordering::AcqRel);
    }
}
fn image_only(clipboard: &gdk::Clipboard) -> bool {
    let f = clipboard.formats();
    f.contain_mime_type("image/png")
        && !f.contain_mime_type("text/plain")
        && !f.contain_mime_type("text/plain;charset=utf-8")
}
fn same_endpoint(a: &Profile, b: &Profile) -> bool {
    crate::launch_context::binding(a) == crate::launch_context::binding(b)
}
fn quote(path: &str) -> String {
    if path
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b"/._-".contains(&b))
    {
        return path.into();
    }
    format!("'{}'", path.replace('\'', "'\\''"))
}
/// Keep Ctrl+V behavior: upload the PNG, then paste its path into the same
/// focused terminal. Never infer a different destination from terminal text.
pub fn try_paste(view: &Rc<View>) -> bool {
    use std::sync::atomic::Ordering;
    let pane = view.target.borrow().clone();
    let Some(profile) = target_profile(view, &pane) else {
        let stage = CONFIGURATION.with(|c| c.borrow().as_ref().and_then(|c| c.stage.clone()));
        return crate::image_attach::try_clipboard_paste(view, stage);
    };
    let clipboard = view.area.display().clipboard();
    if !image_only(&clipboard) {
        return false;
    }
    let original_pid = pane_pid(view, &pane).unwrap_or(0);
    let transport = crate::launches::image_transport(&view.session, &pane, original_pid);
    let explicit_fixture = TARGETS.with(|t| {
        t.borrow()
            .contains_key(&(view.session.clone(), pane.clone()))
    });
    if transport.is_none() && !explicit_fixture {
        view.warn("This SSH terminal has no protected upload connection. Reopen its bookmark before pasting an image; the running terminal is preserved.");
        return true;
    }
    let Some(configuration) = CONFIGURATION.with(|c| c.borrow().clone()) else {
        view.warn("Image paste configuration unavailable");
        return true;
    };
    let Some(stage) = configuration.stage.clone() else {
        view.warn("Private image staging unavailable");
        return true;
    };
    let permit = match Permit::acquire() {
        Ok(v) => v,
        Err(e) => {
            view.warn(&e);
            return true;
        }
    };
    let epoch = view.broker.borrow().epoch();
    let allowed = view.begin_image_paste();
    let weak = Rc::downgrade(view);
    let read_generation = Rc::new(std::cell::Cell::new(None));
    let current: Rc<dyn Fn() -> bool> = Rc::new({
        let read_generation = read_generation.clone();
        let (weak, pane, profile, reader, allowed) = (
            weak.clone(),
            pane.clone(),
            profile.clone(),
            configuration.reader.clone(),
            allowed.clone(),
        );
        move || {
            weak.upgrade().is_some_and(|v| {
                allowed.load(Ordering::Acquire)
                    && !v.detached.get()
                    && !v.finished.get()
                    && v.area.has_focus()
                    && v.area.is_mapped()
                    && *v.target.borrow() == pane
                    && v.image_paste_destination_alive(&pane)
                    && target_profile(&v, &pane).is_some_and(|p| same_endpoint(&profile, &p))
                    && v.broker.borrow().epoch() == epoch
                    && read_generation.get().is_none_or(|generation| {
                        v.broker
                            .borrow()
                            .last_transfer
                            .as_ref()
                            .is_some_and(|t| t.generation == generation)
                    })
                    && v.broker.borrow().active() == Some(v.session.as_str())
                    && v.root
                        .root()
                        .and_downcast::<gtk::Window>()
                        .is_some_and(|w| w.is_active())
                    && reader(&profile.id).is_ok_and(|p| same_endpoint(&profile, &p))
            })
        }
    });
    if !current() {
        view.end_image_paste(&allowed);
        view.warn("Select the original SSH pane and unchanged endpoint before pasting an image");
        return true;
    }
    let limit = match crate::staging::available(&stage) {
        Ok(n) => n,
        Err(e) => {
            view.end_image_paste(&allowed);
            view.warn(&e.to_string());
            return true;
        }
    };
    view.status
        .set_text(&format!("Copying clipboard image to {}…", profile.name));
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    let (valid, ticket) = (current.clone(), allowed.clone());
    crate::clipboard::ingest_png(
        &clipboard,
        stage,
        view.broker.clone(),
        epoch,
        limit,
        Duration::ZERO,
        move |result| {
            let image = match result {
                Ok(v) => v,
                Err(e) => {
                    let _ = tx.send(Err(e));
                    return;
                }
            };
            if !valid() {
                let _ = std::fs::remove_file(&image.path);
                let _ = tx.send(Err("Image paste cancelled before upload".into()));
                return;
            }
            std::thread::spawn(move || {
                let _permit = permit;
                let result = std::fs::read(&image.path)
                    .map_err(|_| "Staged PNG unavailable".to_string())
                    .and_then(|bytes| {
                        if let Some(token) = transport.as_deref() {
                            crate::remote_paste_upload::upload_original(
                                &profile,
                                &bytes,
                                &ticket,
                                Duration::from_secs(60),
                                token,
                                original_pid,
                            )
                        } else {
                            crate::remote_paste_upload::upload(
                                &profile,
                                &bytes,
                                &ticket,
                                Duration::from_secs(60),
                            )
                        }
                    });
                let _ = std::fs::remove_file(&image.path);
                let _ = tx.send(result);
            });
        },
    );
    read_generation.set(
        view.broker
            .borrow()
            .last_transfer
            .as_ref()
            .map(|t| t.generation),
    );
    let deadline = Instant::now() + Duration::from_secs(75);
    let mut delivery: Option<std::sync::mpsc::Receiver<Result<(), String>>> = None;
    glib::timeout_add_local(Duration::from_millis(16), move || {
        let Some(view) = weak.upgrade() else {
            allowed.store(false, Ordering::Release);
            return glib::ControlFlow::Break;
        };
        if !current() || Instant::now() >= deadline {
            if view.end_image_paste(&allowed) {
                view.status.set_text("Image paste cancelled: focus, pane, endpoint or clipboard changed. No alternate destination used.");
            }
            return glib::ControlFlow::Break;
        }
        if let Some(receive) = delivery.as_ref() {
            match receive.try_recv() {
                Ok(result) => {
                    if view.end_image_paste(&allowed) {
                        match result{Ok(())=>view.status.set_text("Image path pasted into the original SSH pane"),Err(e)=>view.warn(&format!("Image copied but path paste failed: {e}. Inspect the original pane before retrying."))}
                    }
                    return glib::ControlFlow::Break;
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    if view.end_image_paste(&allowed) {
                        view.warn("Image paste worker disconnected; inspect the original pane before retrying");
                    }
                    return glib::ControlFlow::Break;
                }
                Err(_) => return glib::ControlFlow::Continue,
            }
        }
        match rx.try_recv() {
            Ok(Ok(path)) => {
                match view.guarded_image_path(pane.clone(), quote(&path), allowed.clone()) {
                    Ok(receive) => delivery = Some(receive),
                    Err(e) => {
                        if view.end_image_paste(&allowed) {
                            view.warn(&e);
                        }
                        return glib::ControlFlow::Break;
                    }
                }
            }
            Ok(Err(e)) => {
                if view.end_image_paste(&allowed) {
                    view.warn(&format!("Image not sent: {e}"));
                }
                return glib::ControlFlow::Break;
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                if view.end_image_paste(&allowed) {
                    view.warn("Image upload worker disconnected");
                }
                return glib::ControlFlow::Break;
            }
            Err(_) => (),
        }
        glib::ControlFlow::Continue
    });
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn endpoint_binding_ignores_recency_but_rejects_retargeting_or_credentials() {
        let p:Profile=serde_json::from_value(serde_json::json!({"id":"endpoint_generated","machine_id":"machine_generated","name":"Generated","group":"","protocol":"SSH","server":"127.0.0.1:22222","username":"","secret_reference":"generated","favorite":false,"last_used":0,"tags":[]})).unwrap();
        let mut other = p.clone();
        other.last_used = 42;
        other.favorite = true;
        assert!(same_endpoint(&p, &other));
        other.server = "127.0.0.1:22223".into();
        assert!(!same_endpoint(&p, &other));
        other = p.clone();
        other.secret_reference = "other-generated".into();
        assert!(!same_endpoint(&p, &other));
        other = p.clone();
        other.preferences.identity_file = "/generated/another-key".into();
        assert!(!same_endpoint(&p, &other));
        assert_eq!(
            quote("/generated/image_abc.png"),
            "/generated/image_abc.png"
        );
    }
    #[test]
    fn quotes_spaces_and_metacharacters_without_shell_expansion() {
        assert_eq!(quote("/tmp/a b'c.png"), "'/tmp/a b'\\''c.png'");
    }
}

/// Available only through an explicitly configured private GUI fixture.
pub fn fixture(view: &Rc<View>, q: &serde_json::Value) -> serde_json::Value {
    if let Some(p) = q.get("profile") {
        match serde_json::from_value::<Profile>(p.clone()) {
            Ok(p) if p.protocol == "SSH" && p.validate().is_ok() => {
                let pane = view.target.borrow().clone();
                if let Some(pid) = pane_pid(view, &pane) {
                    TARGETS.with(|t| {
                        t.borrow_mut()
                            .insert((view.session.clone(), pane), (pid, p));
                    });
                } else {
                    return serde_json::json!({"error":"Fixture pane identity unavailable"});
                }
            }
            _ => return serde_json::json!({"error":"Invalid fixture SSH profile"}),
        }
    }
    if q["reconnect"] == true {
        view.reconnect();
    }
    let handled = q["paste"] == true && try_paste(view);
    let pane = view.target.borrow().clone();
    serde_json::json!({"handled":handled,"pending":view.image_paste_pending(),"status":view.status.text().as_str(),"warning":*view.warning.borrow(),"pane":pane,"pid":pane_pid(view,&pane),"target_bound":target_profile(view,&pane).is_some(),"image_offer":image_only(&view.area.display().clipboard())})
}
