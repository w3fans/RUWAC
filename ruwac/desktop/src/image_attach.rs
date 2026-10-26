//! Native draft attachment runs on the existing authenticated terminal worker.
use crate::worker::Rpc;
#[path = "../../crates/ruwac-pty/src/attachment.rs"]
mod guard;
use serde_json::{Value, json};
use std::{
    io,
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

fn allowed(ticket: &AtomicBool) -> io::Result<()> {
    if ticket.load(Ordering::Acquire) {
        Ok(())
    } else {
        Err(io::Error::other(
            "Image attachment cancelled: focus, tab or target changed",
        ))
    }
}
fn target(rpc: &mut Rpc, session: &str, pane: &str, pid: Option<u64>) -> io::Result<u64> {
    let job = rpc.call("job.query", json!({"pane":pane}))?;
    let current = job["foreground"]["pid"]
        .as_u64()
        .ok_or_else(|| io::Error::other("Live Codex foreground unavailable"))?;
    if job["session"] != session || job["selected"] != true || pid.is_some_and(|pid| pid != current)
    {
        return Err(io::Error::other("Image attachment target changed"));
    }
    let frame = rpc.call("client.render", json!({}))?;
    let selected = frame["panes"]
        .as_array()
        .and_then(|p| p.iter().find(|p| p["id"] == pane));
    if frame["active_pane"] != pane
        || !selected.is_some_and(|p| p["grid"]["bracketed_paste"] == true && p["ended"] != true)
    {
        return Err(io::Error::other(
            "Select a live Codex pane with bracketed paste enabled",
        ));
    }
    let screen = rpc.call("pane.capture", json!({"pane":pane,"lines":100}))?;
    if !guard::model_ready(screen["text"].as_str().unwrap_or("")) {
        return Err(io::Error::other(
            "Wait until Codex has loaded an image-capable model",
        ));
    }
    Ok(current)
}
pub fn attach(
    rpc: &mut Rpc,
    session: &str,
    pane: &str,
    path: &Path,
    ticket: &AtomicBool,
    native_api: bool,
) -> io::Result<Value> {
    allowed(ticket)?;
    let pid = target(rpc, session, pane, None)?;
    let path = path
        .to_str()
        .ok_or_else(|| io::Error::other("Image path is not UTF-8"))?;
    guard::validate(
        path,
        pid.try_into()
            .map_err(|_| io::Error::other("Invalid foreground PID"))?,
    )?;
    let before = rpc.call("pane.capture", json!({"pane":pane,"lines":100}))?;
    let before = guard::highest_image(before["text"].as_str().unwrap_or(""));
    target(rpc, session, pane, Some(pid))?;
    allowed(ticket)?;
    if native_api {
        rpc.call(
            "pane.attachImage",
            json!({"pane":pane,"path":path,"foreground_pid":pid}),
        )?;
    } else {
        // Preserve old persistent engines. Same local process/file checks apply;
        // the already-authenticated attached client routes a literal path to the explicit pane.
        rpc.call(
            "client.input",
            json!({"pane":pane,"kind":"paste","text":path}),
        )?;
    }
    let end = Instant::now() + Duration::from_secs(3);
    let mut observed = false;
    while Instant::now() < end {
        if allowed(ticket).is_err() {
            break;
        }
        let job = rpc.call("job.query", json!({"pane":pane}))?;
        if job["foreground"]["pid"].as_u64() != Some(pid) {
            break;
        }
        let capture = rpc.call("pane.capture", json!({"pane":pane,"lines":100}))?;
        if guard::highest_image(capture["text"].as_str().unwrap_or("")) > before {
            observed = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(40));
    }
    Ok(
        json!({"pane":pane,"path":path,"queued":true,"draft_attachment_observed":observed,"submitted":false,"consumed":false,"legacy_daemon":!native_api}),
    )
}

/// GDK reads are asynchronous; the GTK thread continuously revokes worker delivery on focus changes.
pub fn clipboard(
    view: &std::rc::Rc<crate::view::View>,
    directory: std::path::PathBuf,
    selected: std::rc::Rc<dyn Fn() -> bool>,
    delay: Duration,
    done: impl FnOnce(Result<Value, String>) + 'static,
) {
    clipboard_route(view, directory, selected, delay, None, done);
}
pub fn clipboard_route(
    view: &std::rc::Rc<crate::view::View>,
    directory: std::path::PathBuf,
    selected: std::rc::Rc<dyn Fn() -> bool>,
    delay: Duration,
    remote: Option<crate::remote_image::Target>,
    done: impl FnOnce(Result<Value, String>) + 'static,
) {
    use gtk::prelude::*;
    use std::sync::{Arc, mpsc};
    view.focus();
    let pane = view.target.borrow().clone();
    let epoch = view.broker.borrow().epoch();
    let valid = move |v: &crate::view::View| {
        selected()
            && !v.detached.get()
            && v.area.has_focus()
            && v.root
                .root()
                .and_downcast::<gtk::Window>()
                .is_some_and(|w| w.is_active())
            && *v.target.borrow() == pane
            && v.broker.borrow().active() == Some(v.session.as_str())
            && v.broker.borrow().epoch() == epoch
    };
    if !valid(view) {
        done(Err(
            "Select and focus the intended Codex pane before attaching an image".into(),
        ));
        return;
    }
    let limit =
        match crate::staging::available_copies(&directory, if remote.is_some() { 3 } else { 2 }) {
            Ok(v) => v,
            Err(e) => {
                done(Err(e.to_string()));
                return;
            }
        };
    let weak = std::rc::Rc::downgrade(view);
    crate::clipboard::ingest_png(
        &view.area.display().clipboard(),
        directory,
        view.broker.clone(),
        epoch,
        limit,
        delay,
        move |result| {
            let image = match result {
                Ok(image) => image,
                Err(error) => {
                    done(Err(error));
                    return;
                }
            };
            let Some(view) = weak.upgrade() else {
                let _ = std::fs::remove_file(&image.path);
                done(Err("Terminal view closed".into()));
                return;
            };
            if !valid(&view) {
                let _ = std::fs::remove_file(&image.path);
                done(Err(
                    "Image attachment cancelled: focus or tab changed".into()
                ));
                return;
            }
            if let Err(error) = crate::staging::retain(&image.path) {
                let _ = std::fs::remove_file(&image.path);
                done(Err(error.to_string()));
                return;
            }
            let allowed = Arc::new(AtomicBool::new(true));
            let (send, receive) = mpsc::sync_channel(1);
            let remote_delivery = remote.is_some();
            let queued = if let Some(target) = remote {
                let (path, ticket) = (image.path.clone(), allowed.clone());
                std::thread::spawn(move || {
                    let result = crate::remote_image::send(&target, &path, &ticket);
                    let _ = send.send(result);
                });
                Ok(())
            } else {
                view.worker
                    .borrow()
                    .send(crate::worker::Command::AttachImage {
                        pane: view.target.borrow().clone(),
                        path: image.path.clone(),
                        allowed: allowed.clone(),
                        done: send,
                    })
            };
            if let Err(error) = queued {
                done(Err(error));
                return;
            }
            let end = Instant::now() + Duration::from_secs(if remote_delivery { 75 } else { 15 });
            let mut done = Some(done);
            gtk::glib::timeout_add_local(Duration::from_millis(16), move || {
                let valid = weak.upgrade().is_some_and(|view| valid(&view));
                if !valid {
                    allowed.store(false, Ordering::Release);
                }
                match receive.try_recv() {
                    Ok(result) => {
                        allowed.store(false, Ordering::Release);
                        let result = result
                            .map_err(|error| {
                                format!("{error}. Staged PNG retained at {}", image.path.display())
                            })
                            .map(|mut receipt| {
                                receipt["sha256"] = json!(image.sha256);
                                receipt["bytes"] = json!(image.bytes);
                                receipt["width"] = json!(image.width);
                                receipt["height"] = json!(image.height);
                                receipt["retention_hours"] = json!(24);
                                receipt["focus_current"] = json!(valid);
                                receipt
                            });
                        if let Some(done) = done.take() {
                            done(result);
                        }
                        gtk::glib::ControlFlow::Break
                    }
                    Err(mpsc::TryRecvError::Disconnected) => {
                        if let Some(done) = done.take() {
                            done(Err("Attachment worker disconnected; staged image retained, inspect the draft before retrying".into()));
                        }
                        gtk::glib::ControlFlow::Break
                    }
                    Err(mpsc::TryRecvError::Empty) if Instant::now() >= end => {
                        allowed.store(false, Ordering::Release);
                        if let Some(done) = done.take() {
                            done(Err("Attachment result timed out; staged image retained, inspect the draft before retrying".into()));
                        }
                        gtk::glib::ControlFlow::Break
                    }
                    _ => gtk::glib::ControlFlow::Continue,
                }
            });
        },
    );
}

/// Local Codex image keys use the same guarded route as the explicit button.
/// Screen text is only a routing hint; worker process/file/model checks remain
/// authoritative before a path can reach a draft. Other programs keep their keys.
pub fn try_clipboard_paste(
    view: &std::rc::Rc<crate::view::View>,
    directory: Option<std::path::PathBuf>,
) -> bool {
    use gtk::prelude::*;
    let formats = view.area.display().clipboard().formats();
    if !formats.contain_mime_type("image/png")
        || formats.contain_mime_type("text/plain")
        || formats.contain_mime_type("text/plain;charset=utf-8")
    {
        return false;
    }
    let hinted = view.frame.borrow().as_ref().is_some_and(|frame| {
        let pane = view.target.borrow();
        frame["panes"]
            .as_array()
            .and_then(|panes| panes.iter().find(|p| p["id"] == pane.as_str()))
            .is_some_and(|p| local_model_hint(&p["grid"]))
    });
    if !hinted {
        return false;
    }
    let Some(directory) = directory else {
        view.warn("Private image staging is unavailable");
        return true;
    };
    if view.image_paste_pending() {
        view.warn("An image paste is already in progress");
        return true;
    }
    let ticket = view.begin_image_paste();
    let (valid_ticket, valid_view) = (ticket.clone(), std::rc::Rc::downgrade(view));
    let selected: std::rc::Rc<dyn Fn() -> bool> = std::rc::Rc::new(move || {
        valid_ticket.load(Ordering::Acquire)
            && valid_view.upgrade().is_some_and(|v| v.root.is_mapped())
    });
    let weak = std::rc::Rc::downgrade(view);
    view.warn("Reading clipboard PNG for the selected Codex draft…");
    clipboard(view, directory, selected, Duration::ZERO, move |result| {
        let Some(view) = weak.upgrade() else {
            return;
        };
        if !view.end_image_paste(&ticket) {
            return;
        }
        match result {
            Ok(receipt)
                if receipt["draft_attachment_observed"] == true
                    && receipt["focus_current"] == true =>
            {
                view.warn("PNG attached to the Codex draft; submit it in Codex when ready.")
            }
            Ok(_) => {
                view.warn("Image attachment not confirmed; inspect the draft before retrying.")
            }
            Err(error) => view.warn(&error),
        }
    });
    true
}

fn local_model_hint(grid: &Value) -> bool {
    if grid["bracketed_paste"] != true {
        return false;
    }
    let Some(rows) = grid["cells"].as_array() else {
        return false;
    };
    let mut text = String::new();
    for row in rows.iter().skip(rows.len().saturating_sub(100)) {
        if let Some(cells) = row.as_array() {
            for cell in cells {
                text.push_str(cell[0].as_str().unwrap_or(""));
            }
        }
        text.push('\n');
    }
    guard::model_ready(&text)
}

#[cfg(test)]
mod local_paste_tests {
    use super::*;
    #[test]
    fn routing_hint_requires_ready_grid_but_never_authorizes_a_process() {
        let mut grid = json!({"bracketed_paste":true,"cells":[[["GPT-6.1-Sol"]]]});
        assert!(local_model_hint(&grid));
        grid["bracketed_paste"] = json!(false);
        assert!(!local_model_hint(&grid));
        grid["bracketed_paste"] = json!(true);
        grid["cells"] = json!([[["shell prompt"]]]);
        assert!(!local_model_hint(&grid));
        assert!(!local_model_hint(&json!({})));
    }
}
