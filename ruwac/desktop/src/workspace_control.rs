//! Saved-workspace lifecycle. Storage/RPC run off GTK; existing widgets retain their processes.
use crate::{ipc, tiles, workspace, workspace_store};
use adw::prelude::*;
use serde_json::{Value, json};
use std::{
    cell::{Cell, RefCell},
    collections::{HashMap, HashSet},
    path::PathBuf,
    rc::Rc,
    sync::mpsc,
    time::{Duration, Instant},
};
type Create = Rc<dyn Fn(&workspace::Layout) -> gtk::Widget>;
type Answer = ruwac_protocol::Result<Value>;
type Reply = std::sync::mpsc::SyncSender<Answer>;
#[derive(Default)]
pub struct State {
    cache: HashMap<String, (String, Option<Answer>, Vec<Reply>)>,
}
pub struct Control {
    pub tabs: adw::TabView,
    pub memory: tiles::FocusMemory,
    pub permission: Rc<Cell<bool>>,
    pub gate: std::sync::Arc<std::sync::atomic::AtomicBool>,
    pub record: workspace::Recorder,
    pub create: Create,
    pub known_sessions: Rc<RefCell<HashSet<String>>>,
    pub socket: PathBuf,
    pub folder: PathBuf,
    pub state: RefCell<State>,
    pub workers: std::sync::Arc<std::sync::atomic::AtomicUsize>,
}
enum Prepared {
    Summary(Value),
    Layout(workspace_store::Snapshot, HashSet<String>),
}
fn error(code: &str, message: impl Into<String>) -> ruwac_protocol::Error {
    ruwac_protocol::Error::new(code, message)
}
fn walk(layout: &workspace::Layout, out: &mut Vec<workspace::Layout>) {
    if let workspace::Layout::Split { first, second, .. } = layout {
        walk(first, out);
        walk(second, out)
    } else {
        out.push(layout.clone());
    }
}
impl Control {
    pub fn submit(self: &Rc<Self>, task: ipc::Task) {
        self.submit_as(task, false);
    }
    fn submit_as(self: &Rc<Self>, task: ipc::Task, interactive: bool) {
        let method = task.request.method.clone();
        let params = task.request.params.as_object();
        let parse =
            (|| -> ruwac_protocol::Result<(Option<String>, Option<workspace_store::Snapshot>)> {
                let p = params.ok_or_else(|| {
                    error("INVALID_REQUEST", "Workspace operation requires an object")
                })?;
                match method.as_str() {
                    "workspace.saved" if p.is_empty() => Ok((None, None)),
                    "workspace.save" if p.len() == 1 => {
                        let name = p
                            .get("name")
                            .and_then(Value::as_str)
                            .filter(|s| {
                                !s.is_empty() && s.len() <= 128 && !s.chars().any(char::is_control)
                            })
                            .ok_or_else(|| {
                                error("INVALID_REQUEST", "Bounded snapshot name required")
                            })?;
                        if !interactive && !self.permission.get() {
                            return Err(error(
                                "PERMISSION_DENIED",
                                "Workspace lifecycle control is disabled",
                            ));
                        }
                        Ok((
                            None,
                            Some(workspace_store::Snapshot {
                                id: ruwac_protocol::id("workspace"),
                                name: name.into(),
                                workspace: (self.record)()
                                    .map_err(|e| error("WORKSPACE_UNAVAILABLE", e))?,
                            }),
                        ))
                    }
                    "workspace.open" | "workspace.restore" if p.len() == 1 => {
                        let id = p
                            .get("id")
                            .and_then(Value::as_str)
                            .filter(|s| ruwac_protocol::valid_id(s, "workspace"))
                            .ok_or_else(|| {
                                error("INVALID_TARGET", "Explicit saved workspace ID required")
                            })?;
                        if !interactive && !self.permission.get() {
                            return Err(error(
                                "PERMISSION_DENIED",
                                "Workspace lifecycle control is disabled",
                            ));
                        }
                        Ok((Some(id.into()), None))
                    }
                    _ => Err(error(
                        "INVALID_REQUEST",
                        "Workspace operation parameters rejected",
                    )),
                }
            })();
        let (id, snapshot) = match parse {
            Ok(v) => v,
            Err(e) => {
                audit(&task, "denied");
                let _ = task.reply.try_send(Err(e));
                return;
            }
        };
        let key = task.request.idempotency_key.clone();
        if let Some(key) = key.as_ref() {
            let signature = format!("{}:{}", method, task.request.params);
            let mut state = self.state.borrow_mut();
            if let Some((old, result, waiters)) = state.cache.get_mut(key) {
                if old != &signature {
                    let _ = task.reply.try_send(Err(error(
                        "IDEMPOTENCY_CONFLICT",
                        "Key belongs to another workspace request",
                    )));
                } else if let Some(result) = result {
                    let _ = task.reply.try_send(result.clone());
                } else if waiters.len() < 8 {
                    waiters.push(task.reply.clone());
                } else {
                    let _ = task
                        .reply
                        .try_send(Err(error("BUSY", "Idempotent waiter cap")));
                }
                audit(&task, "replayed");
                return;
            }
            if state.cache.len() >= 128 {
                let _ = task
                    .reply
                    .try_send(Err(error("LIMIT", "GUI-instance idempotency cache full")));
                return;
            }
            state
                .cache
                .insert(key.clone(), (signature, None, Vec::new()));
        }
        if self
            .workers
            .fetch_update(
                std::sync::atomic::Ordering::AcqRel,
                std::sync::atomic::Ordering::Acquire,
                |n| (n < 8).then_some(n + 1),
            )
            .is_err()
        {
            if let Some(key) = &key {
                self.state.borrow_mut().cache.remove(key);
            }
            let _ = task
                .reply
                .try_send(Err(error("BUSY", "Workspace preparation worker cap")));
            return;
        }
        let (folder, socket, expires, method_worker) = (
            self.folder.clone(),
            self.socket.clone(),
            task.expires,
            method.clone(),
        );
        let (tx, rx) = mpsc::sync_channel(1);
        let workers = self.workers.clone();
        let gate = if interactive {
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true))
        } else {
            self.gate.clone()
        };
        std::thread::spawn(move || {
            struct Count(std::sync::Arc<std::sync::atomic::AtomicUsize>);
            impl Drop for Count {
                fn drop(&mut self) {
                    self.0.fetch_sub(1, std::sync::atomic::Ordering::AcqRel);
                }
            }
            let _count = Count(workers);
            let result = (|| -> Result<Prepared, std::io::Error> {
                if Instant::now() >= expires {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::TimedOut,
                        "Request expired",
                    ));
                }
                if let Some(snapshot) = snapshot {
                    return workspace_store::save(&folder, &snapshot, expires, &gate)
                        .map(Prepared::Summary);
                }
                if method_worker == "workspace.saved" {
                    return workspace_store::list(&folder, &socket).map(Prepared::Summary);
                }
                let snapshot = workspace_store::read(&folder, id.as_deref().unwrap(), &socket)?;
                let mut rpc = crate::worker::Rpc::connect(&socket)?;
                let sessions = rpc.call("session.list", json!({}))?;
                let known = sessions
                    .as_array()
                    .ok_or_else(|| std::io::Error::other("Daemon session inventory rejected"))?
                    .iter()
                    .filter_map(|v| v["id"].as_str().map(str::to_owned))
                    .collect();
                Ok(Prepared::Layout(snapshot, known))
            })();
            let _ = tx.send(result);
        });
        let control = self.clone();
        gtk::glib::timeout_add_local(Duration::from_millis(10), move || {
            let result = match rx.try_recv() {
                Ok(Ok(Prepared::Summary(v))) => Ok(v),
                Ok(Ok(Prepared::Layout(snapshot, known))) => {
                    if Instant::now() >= task.expires {
                        Err(error(
                            "GUI_TIMEOUT",
                            "Expired workspace request was not applied",
                        ))
                    } else if !interactive && !control.permission.get() {
                        Err(error(
                            "PERMISSION_DENIED",
                            "Workspace permission was revoked",
                        ))
                    } else {
                        *control.known_sessions.borrow_mut() = known;
                        control.apply(snapshot, method == "workspace.restore")
                    }
                }
                Ok(Err(e)) => Err(error(
                    if e.kind() == std::io::ErrorKind::TimedOut {
                        "GUI_TIMEOUT"
                    } else if e.kind() == std::io::ErrorKind::PermissionDenied {
                        "PERMISSION_DENIED"
                    } else if e.kind() == std::io::ErrorKind::WouldBlock {
                        "BUSY"
                    } else {
                        "WORKSPACE_UNAVAILABLE"
                    },
                    "Saved workspace unavailable or rejected",
                )),
                Err(mpsc::TryRecvError::Empty) if Instant::now() < task.expires => {
                    return gtk::glib::ControlFlow::Continue;
                }
                _ => Err(error("GUI_TIMEOUT", "Workspace preparation deadline")),
            };
            audit(
                &task,
                if result.is_ok() {
                    "allowed"
                } else {
                    "rejected"
                },
            );
            if let Some(key) = &key {
                if let Some((_, cached, waiters)) = control.state.borrow_mut().cache.get_mut(key) {
                    *cached = Some(result.clone());
                    for reply in waiters.drain(..) {
                        let _ = reply.try_send(result.clone());
                    }
                }
            }
            let _ = task.reply.try_send(result);
            gtk::glib::ControlFlow::Break
        });
    }
    pub fn user(self: &Rc<Self>, method: &str, params: Value, done: impl FnOnce(Answer) + 'static) {
        let (tx, rx) = mpsc::sync_channel(1);
        self.submit_as(
            ipc::Task {
                request: ruwac_protocol::request(method, params),
                reply: tx,
                actor_uid: unsafe { libc::geteuid() },
                actor_pid: std::process::id() as i32,
                expires: Instant::now() + Duration::from_secs(2),
            },
            true,
        );
        let mut done = Some(done);
        let end = Instant::now() + Duration::from_secs(3);
        gtk::glib::timeout_add_local(Duration::from_millis(10), move || {
            let result = match rx.try_recv() {
                Ok(r) => r,
                Err(mpsc::TryRecvError::Empty) if Instant::now() < end => {
                    return gtk::glib::ControlFlow::Continue;
                }
                _ => Err(error("GUI_TIMEOUT", "Workspace action deadline")),
            };
            if let Some(done) = done.take() {
                done(result);
            }
            gtk::glib::ControlFlow::Break
        });
    }
    fn apply(
        &self,
        mut snapshot: workspace_store::Snapshot,
        restore: bool,
    ) -> ruwac_protocol::Result<Value> {
        if !self
            .tabs
            .root()
            .and_downcast::<gtk::Window>()
            .is_some_and(|w| w.is_visible())
        {
            return Err(error("GUI_UNAVAILABLE", "Workspace window closed"));
        }
        let original_selected = self.tabs.selected_page();
        let current = (self.record)().map_err(|e| error("WORKSPACE_UNAVAILABLE", e))?;
        if current.tabs.iter().any(|tab| {
            !(0..self.tabs.n_pages())
                .any(|i| self.tabs.nth_page(i).child().widget_name().as_str() == tab.id)
        }) {
            return Err(error(
                "WORKSPACE_CONFLICT",
                "Return detached tabs to this window before opening or restoring a layout",
            ));
        }
        let roots = tiles::views(&self.tabs);
        let mut live: HashMap<String, gtk::Widget> = roots
            .iter()
            .map(|w| (w.widget_name().to_string(), w.clone()))
            .collect();
        let mut bound = HashMap::new();
        for t in &current.tabs {
            let mut leaves = Vec::new();
            walk(&t.layout, &mut leaves);
            for l in leaves {
                if let Some(id) = l.view_id() {
                    bound.insert(id.to_owned(), l);
                }
            }
        }
        let aliases = resolve_aliases(&mut snapshot.workspace, &bound);
        let mut wanted = Vec::new();
        for t in &snapshot.workspace.tabs {
            walk(&t.layout, &mut wanted);
        }
        if roots.len()
            + wanted
                .iter()
                .filter(|l| l.view_id().is_some_and(|id| !live.contains_key(id)))
                .count()
            > 256
            || self.tabs.n_pages() as usize + snapshot.workspace.tabs.len() > 64
        {
            return Err(error("LIMIT", "Workspace view or tab limit exceeded"));
        }
        let browser_views = bound
            .values()
            .filter(|l| matches!(l, workspace::Layout::Web { .. }))
            .count();
        let new_browsers = wanted
            .iter()
            .filter(|l| {
                matches!(l, workspace::Layout::Web { .. })
                    && l.view_id().is_none_or(|id| !bound.contains_key(id))
            })
            .count();
        if browser_views + new_browsers > 8 {
            return Err(error(
                "LIMIT",
                "Close a Web view before restoring more (limit 8)",
            ));
        }
        // Existing IDs must keep their semantic target; snapshots cannot retarget a live connection.
        for l in &wanted {
            if let Some(id) = l.view_id() {
                if let Some(existing) = bound.get(id) {
                    let same = match (existing, l) {
                        (
                            workspace::Layout::Terminal { session: a, .. },
                            workspace::Layout::Terminal { session: b, .. },
                        ) => a == b,
                        (
                            workspace::Layout::Rdp { profile: a, .. },
                            workspace::Layout::Rdp { profile: b, .. },
                        ) => a.as_ref().map(|p| &p.id) == b.as_ref().map(|p| &p.id),
                        (
                            workspace::Layout::Files { profile: a, .. },
                            workspace::Layout::Files { profile: b, .. },
                        ) => a.id == b.id,
                        (
                            workspace::Layout::Web { endpoint: a, .. },
                            workspace::Layout::Web { endpoint: b, .. },
                        ) => a == b,
                        _ => false,
                    };
                    if !same {
                        return Err(error(
                            "WORKSPACE_CONFLICT",
                            "Saved view ID conflicts with its live target",
                        ));
                    }
                }
            }
        }
        let ids: HashSet<_> = wanted
            .iter()
            .filter_map(|l| l.view_id().map(str::to_owned))
            .collect();
        let mut tabs_to_restore = Vec::new();
        let mut preserved = Vec::new();
        for tab in &snapshot.workspace.tabs {
            let mut leaves = Vec::new();
            walk(&tab.layout, &mut leaves);
            let overlap = leaves
                .iter()
                .any(|l| l.view_id().is_some_and(|id| live.contains_key(id)));
            if !restore
                && !overlap
                && (0..self.tabs.n_pages())
                    .any(|i| self.tabs.nth_page(i).child().widget_name() == tab.id)
            {
                return Err(error(
                    "WORKSPACE_CONFLICT",
                    "Saved tab ID is in use; restore its layout explicitly",
                ));
            }
            if !restore && overlap {
                if leaves
                    .iter()
                    .all(|l| l.view_id().is_some_and(|id| live.contains_key(id)))
                {
                    preserved.push(tab.id.clone());
                    continue;
                }
                return Err(error(
                    "WORKSPACE_CONFLICT",
                    "Partly open saved tab; use restore to rebuild its layout",
                ));
            }
            tabs_to_restore.push(tab.clone());
        }
        let target_tabs: HashSet<_> = tabs_to_restore.iter().map(|t| t.id.as_str()).collect();
        let extras: Vec<_> = roots
            .iter()
            .filter(|root| {
                !ids.contains(root.widget_name().as_str())
                    && tiles::page(&self.tabs, *root)
                        .is_some_and(|p| target_tabs.contains(p.child().widget_name().as_str()))
            })
            .cloned()
            .collect();
        if restore {
            for root in roots
                .iter()
                .filter(|root| ids.contains(root.widget_name().as_str()))
                .chain(extras.iter())
            {
                tiles::detach(&self.tabs, root).map_err(|e| error("WORKSPACE_CONFLICT", e))?;
            }
        }
        tiles::restore(&self.tabs, &tabs_to_restore, &self.memory, |layout| {
            if let Some(root) = layout.view_id().and_then(|id| live.remove(id)) {
                root
            } else {
                (self.create)(layout)
            }
        });
        for root in &extras {
            let page = tiles::append(&self.tabs, root);
            page.set_title(
                root.tooltip_text()
                    .as_deref()
                    .unwrap_or("Additional live view"),
            );
        }
        // Opening is additive and keeps focus; restoration selects only the explicitly saved tab.
        if !restore {
            if let Some(page) = original_selected {
                if self.tabs.page_position(&page) >= 0 {
                    self.tabs.set_selected_page(&page);
                }
            }
        } else if let Some(index) = snapshot.workspace.selected_tab {
            if let Some(tab) = snapshot.workspace.tabs.get(index) {
                if let Some(page) = (0..self.tabs.n_pages())
                    .map(|i| self.tabs.nth_page(i))
                    .find(|p| p.child().widget_name() == tab.id)
                {
                    self.tabs.set_selected_page(&page);
                }
            }
        }
        Ok(
            json!({"workspace_id":snapshot.id,"view_identity_aliases":aliases,"opened_tabs":tabs_to_restore.iter().map(|t|&t.id).collect::<Vec<_>>(),"already_open_tabs":preserved,"restored":restore,"additional_views_preserved":extras.len(),"remote_connections_started":false,"terminal_processes_started":false}),
        )
    }
}
fn audit(task: &ipc::Task, verdict: &str) {
    let target = task.request.params["id"]
        .as_str()
        .filter(|s| ruwac_protocol::valid_id(s, "workspace"));
    eprintln!(
        "{}",
        json!({"audit":"workspace_ipc","actor_uid":task.actor_uid,"actor_pid":task.actor_pid,"operation":task.request.method,"target":target,"verdict":verdict})
    );
}

fn resolve_aliases(
    saved: &mut workspace::Workspace,
    live: &HashMap<String, workspace::Layout>,
) -> HashMap<String, String> {
    let mut aliases = HashMap::new();
    fn remap(
        layout: &mut workspace::Layout,
        live: &HashMap<String, workspace::Layout>,
        aliases: &mut HashMap<String, String>,
    ) {
        match layout {
            workspace::Layout::Terminal { view_id, session } if !live.contains_key(view_id) => {
                if let Some((id, _)) = live.iter().find(
                    |(_, l)| matches!(l,workspace::Layout::Terminal{session:s,..}if s==session),
                ) {
                    aliases.insert(view_id.clone(), id.clone());
                    *view_id = id.clone();
                }
            }
            workspace::Layout::Split { first, second, .. } => {
                remap(first, live, aliases);
                remap(second, live, aliases);
            }
            _ => (),
        }
    }
    for tab in &mut saved.tabs {
        remap(&mut tab.layout, live, &mut aliases);
        if let Some(id) = tab.focused_view.as_ref() {
            if let Some(actual) = aliases.get(id) {
                tab.focused_view = Some(actual.clone());
            }
        }
    }
    aliases
}

#[cfg(test)]
mod alias_tests {
    use super::*;
    fn terminal(view: &str, session: &str) -> workspace::Layout {
        workspace::Layout::Terminal {
            view_id: view.into(),
            session: session.into(),
        }
    }
    fn saved(layout: workspace::Layout) -> workspace::Workspace {
        workspace::Workspace {
            tabs: vec![workspace::Tab {
                id: "tab_saved".into(),
                title: "Saved".into(),
                focused_view: Some("view_old".into()),
                layout,
            }],
            ..Default::default()
        }
    }
    #[test]
    fn reopened_session_remaps_nested_layout_and_focus() {
        let mut w = saved(workspace::Layout::Split {
            vertical: true,
            ratio: 0.5,
            first: Box::new(terminal("view_old", "session_same")),
            second: Box::new(terminal("view_other", "session_other")),
        });
        let live = HashMap::from([("view_new".into(), terminal("view_new", "session_same"))]);
        assert_eq!(
            resolve_aliases(&mut w, &live),
            HashMap::from([("view_old".into(), "view_new".into())])
        );
        assert_eq!(w.tabs[0].focused_view.as_deref(), Some("view_new"));
        if let workspace::Layout::Split { first, second, .. } = &w.tabs[0].layout {
            assert_eq!(first.view_id(), Some("view_new"));
            assert_eq!(second.view_id(), Some("view_other"));
        } else {
            panic!("split lost");
        }
    }
    #[test]
    fn existing_identity_is_not_retargeted_and_other_sessions_do_not_alias() {
        let live = HashMap::from([
            ("view_old".into(), terminal("view_old", "session_conflict")),
            ("view_new".into(), terminal("view_new", "session_same")),
        ]);
        let mut w = saved(terminal("view_old", "session_same"));
        assert!(resolve_aliases(&mut w, &live).is_empty());
        assert_eq!(w.tabs[0].layout.view_id(), Some("view_old"));
        let mut other = saved(terminal("view_missing", "session_unknown"));
        assert!(resolve_aliases(&mut other, &live).is_empty());
        assert_eq!(other.tabs[0].layout.view_id(), Some("view_missing"));
    }
}
