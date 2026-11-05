//! Ephemeral first-class browser leaves. Nothing here runs in the daemon/TUI.
use crate::{
    view::View,
    web_route::{self, Destination},
    workspace::Layout,
};
use adw::prelude::*;
use gtk::{gdk, glib};
use std::{
    cell::{Cell, RefCell},
    rc::{Rc, Weak},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use webkit6::prelude::*;
thread_local! { static VIEWS:RefCell<Vec<Rc<Web>>> = const {RefCell::new(Vec::new())}; }
thread_local! {static TERMINALS:RefCell<Option<Weak<RefCell<Vec<Rc<View>>>>>>=const {RefCell::new(None)};}
pub fn configure(views: &Rc<RefCell<Vec<Rc<View>>>>) {
    TERMINALS.with(|v| *v.borrow_mut() = Some(Rc::downgrade(views)));
}
const MAX_VIEWS: usize = 8;
thread_local! {static SCRIPT_RESULT:RefCell<Option<serde_json::Value>>=const {RefCell::new(None)};}
#[derive(Clone)]
pub struct Source {
    view: Weak<View>,
    session: String,
    pane: String,
    pid: u32,
    endpoint: String,
    binding: String,
    token: String,
    label: String,
}
impl Source {
    pub fn capture(view: &Rc<View>) -> Option<Self> {
        let pane = view.target.borrow().clone();
        let pid = view
            .frame
            .borrow()
            .as_ref()?
            .get("panes")?
            .as_array()?
            .iter()
            .find(|p| p["id"] == pane && p["ended"] != true && p["restored"] != true)?["pid"]
            .as_u64()? as u32;
        let p = crate::launches::image_profile(&view.session, &pane, pid)?;
        let token = crate::launches::image_transport(&view.session, &pane, pid)?;
        Some(Self {
            view: Rc::downgrade(view),
            session: view.session.clone(),
            pane,
            pid,
            endpoint: p.id.clone(),
            binding: crate::launch_context::binding(&p),
            token,
            label: p.name.clone(),
        })
    }
    fn valid(&self) -> bool {
        self.view
            .upgrade()
            .is_some_and(|v| !v.detached.get() && v.image_paste_destination_alive(&self.pane))
            && crate::launches::image_profile(&self.session, &self.pane, self.pid).is_some_and(
                |p| p.id == self.endpoint && crate::launch_context::binding(&p) == self.binding,
            )
            && crate::launches::image_transport(&self.session, &self.pane, self.pid).as_deref()
                == Some(&self.token)
    }
}
struct Remote {
    source: Source,
    original: url::Url,
    forward: crate::ssh_transport::LocalForward,
}
#[derive(Clone)]
struct TlsFailure {
    generation: u64,
    uri: String,
    certificate: gtk::gio::TlsCertificate,
    errors: gtk::gio::TlsCertificateFlags,
}
struct Web {
    root: gtk::Box,
    browser: webkit6::WebView,
    address: gtk::Entry,
    status: gtk::Label,
    content: gtk::Stack,
    failure_title: gtk::Label,
    failure_detail: gtk::Label,
    certificate_button: gtk::Button,
    failed: Cell<bool>,
    generation: Cell<u64>,
    tls_failure: RefCell<Option<TlsFailure>>,
    remote: RefCell<Option<Remote>>,
    pending: RefCell<Option<Arc<AtomicBool>>>,
    binding: RefCell<Option<String>>,
    closed: Cell<bool>,
    endpoint: RefCell<Option<String>>,
}
fn certificate_issues(errors: gtk::gio::TlsCertificateFlags) -> String {
    use gtk::gio::TlsCertificateFlags as F;
    let mut messages = Vec::new();
    for (flag, message) in [
        (F::UNKNOWN_CA, "Issuer is not trusted"),
        (
            F::BAD_IDENTITY,
            "Certificate does not match the service address",
        ),
        (F::NOT_ACTIVATED, "Certificate is not valid yet"),
        (F::EXPIRED, "Certificate has expired"),
        (F::REVOKED, "Certificate has been revoked"),
        (F::INSECURE, "Certificate uses insecure cryptography"),
        (F::GENERIC_ERROR, "Certificate verification failed"),
    ] {
        if errors.contains(flag) {
            messages.push(message);
        }
    }
    messages.join("\n")
}
impl Web {
    fn new(endpoint: Option<String>) -> Rc<Self> {
        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.set_hexpand(true);
        root.set_vexpand(true);
        root.set_widget_name(&ruwac_protocol::id("view"));
        root.add_css_class("ruwac-web");
        let bar = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        bar.set_margin_start(4);
        bar.set_margin_end(4);
        let back = gtk::Button::from_icon_name("go-previous-symbolic");
        back.set_widget_name("web-back");
        back.set_tooltip_text(Some("Back"));
        let next = gtk::Button::from_icon_name("go-next-symbolic");
        next.set_widget_name("web-forward");
        next.set_tooltip_text(Some("Forward"));
        let reload = gtk::Button::from_icon_name("view-refresh-symbolic");
        reload.set_widget_name("web-reload");
        reload.set_tooltip_text(Some("Reload"));
        let address = gtk::Entry::new();
        address.set_hexpand(true);
        address.set_placeholder_text(Some("https://example.org — Ctrl+L to edit"));
        address.set_widget_name("web-address");
        let external = gtk::Button::from_icon_name("external-link-symbolic");
        external.set_tooltip_text(Some("Open current page in external browser"));
        for b in [&back, &next, &reload] {
            bar.append(b);
        }
        bar.append(&address);
        bar.append(&external);
        let reconnect = gtk::Button::from_icon_name("network-connect-symbolic");
        reconnect.set_widget_name("web-reconnect");
        reconnect.set_tooltip_text(Some(
            "Reconnect restored remote preview using an explicitly chosen current SSH terminal",
        ));
        bar.append(&reconnect);
        let session = webkit6::NetworkSession::new_ephemeral();
        let browser = webkit6::WebView::builder()
            .network_session(&session)
            .hexpand(true)
            .vexpand(true)
            .build();
        if let Some(settings) = webkit6::prelude::WebViewExt::settings(&browser) {
            settings.set_enable_developer_extras(true);
        }
        let status = gtk::Label::new(Some(if endpoint.is_some() {
            "Remote preview restored disconnected. Open its current SSH terminal and Ctrl+click the URL to reconnect."
        } else {
            "Enter a web address. Browsing storage is private to this view and discarded when it closes."
        }));
        status.set_xalign(0.0);
        status.set_ellipsize(gtk::pango::EllipsizeMode::End);
        let content = gtk::Stack::new();
        content.set_vexpand(true);
        content.set_hexpand(true);
        content.add_named(&browser, Some("page"));
        let failure = gtk::Box::new(gtk::Orientation::Vertical, 12);
        failure.set_halign(gtk::Align::Center);
        failure.set_valign(gtk::Align::Center);
        failure.set_margin_start(24);
        failure.set_margin_end(24);
        let failure_title = gtk::Label::new(None);
        failure_title.add_css_class("title-2");
        failure_title.set_widget_name("web-failure-title");
        let failure_detail = gtk::Label::new(None);
        failure_detail.set_widget_name("web-failure-detail");
        failure_detail.set_wrap(true);
        failure_detail.set_max_width_chars(70);
        let certificate_button = gtk::Button::with_label("Review certificate…");
        certificate_button.set_widget_name("web-review-certificate");
        certificate_button.set_halign(gtk::Align::Center);
        failure.append(&failure_title);
        failure.append(&failure_detail);
        failure.append(&certificate_button);
        content.add_named(&failure, Some("error"));
        root.append(&bar);
        root.append(&content);
        root.append(&status);
        let web = Rc::new(Self {
            root,
            browser,
            address,
            status,
            content,
            failure_title,
            failure_detail,
            certificate_button,
            failed: Cell::new(false),
            generation: Cell::new(0),
            tls_failure: RefCell::new(None),
            remote: RefCell::new(None),
            pending: RefCell::new(None),
            binding: RefCell::new(None),
            closed: Cell::new(false),
            endpoint: RefCell::new(endpoint),
        });
        let w = Rc::downgrade(&web);
        web.certificate_button.connect_clicked(move |_| {
            if let Some(w) = w.upgrade() {
                w.review_certificate();
            }
        });
        let w = Rc::downgrade(&web);
        back.connect_clicked(move |_| {
            if let Some(w) = w.upgrade() {
                if w.navigation_ready() {
                    w.browser.go_back();
                } else {
                    w.disconnect();
                }
            }
        });
        let w = Rc::downgrade(&web);
        next.connect_clicked(move |_| {
            if let Some(w) = w.upgrade() {
                if w.navigation_ready() {
                    w.browser.go_forward();
                } else {
                    w.disconnect();
                }
            }
        });
        let w = Rc::downgrade(&web);
        reload.connect_clicked(move |_| {
            if let Some(w) = w.upgrade() {
                if w.navigation_ready() {
                    w.browser.reload();
                } else {
                    w.disconnect();
                }
            }
        });
        let w = Rc::downgrade(&web);
        reconnect.connect_clicked(move |_| {
            if let Some(w) = w.upgrade() {
                w.choose_source();
            }
        });
        let w = Rc::downgrade(&web);
        web.address.connect_activate(move |entry| {
            if let Some(w) = w.upgrade() {
                w.navigate(entry.text().as_str(), None);
            }
        });
        let w = Rc::downgrade(&web);
        external.connect_clicked(move |_| {
            if let Some(w) = w.upgrade() {
                if !w.navigation_ready() {
                    w.disconnect();
                    return;
                }
                if let Some(uri) = w.browser.uri().filter(|u| web_route::parse(u).is_ok()) {
                    gtk::gio::AppInfo::launch_default_for_uri_async(
                        &uri,
                        None::<&gtk::gio::AppLaunchContext>,
                        None::<&gtk::gio::Cancellable>,
                        move |_| {},
                    );
                }
            }
        });
        let w = Rc::downgrade(&web);
        web.browser.connect_uri_notify(move |browser| {
            if let Some(w) = w.upgrade() {
                if let Some(uri) = browser.uri() {
                    w.address.set_text(&uri);
                }
            }
        });
        let w = Rc::downgrade(&web);
        web.browser.connect_load_changed(move |_, event| {
            if let Some(w) = w.upgrade() {
                if event == webkit6::LoadEvent::Started {
                    w.generation.set(w.generation.get().wrapping_add(1));
                    w.failed.set(false);
                    w.tls_failure.borrow_mut().take();
                    w.content.set_visible_child_name("page");
                    w.status.set_text("Loading web service…");
                }
                if event == webkit6::LoadEvent::Finished && !w.failed.get() {
                    if let Some(r) = w.remote.borrow().as_ref() {
                        w.status.set_text(&format!(
                            "{} · remote {}:{} → localhost:{} · ephemeral browser",
                            r.source.label,
                            r.original.host_str().unwrap_or("localhost"),
                            r.original.port_or_known_default().unwrap_or(0),
                            r.forward.local_port
                        ));
                    } else {
                        w.status.set_text("Local web · ephemeral browser");
                    }
                }
            }
        });
        let w = Rc::downgrade(&web);
        web.browser.connect_load_failed(move |_, _, uri, error| {
            if error.matches(webkit6::NetworkError::Cancelled) { return true; }
            if let Some(w) = w.upgrade() {
                if w.tls_failure.borrow().is_none() {
                    let origin = web_route::parse(uri).map(|u| u.origin().ascii_serialization()).unwrap_or_else(|_| "this service".into());
                    w.show_failure("Page could not load", &format!("Could not load {origin}. Check the service address, its server and the SSH connection, then retry."), false);
                }
            }
            true
        });
        let w = Rc::downgrade(&web);
        web.browser.connect_load_failed_with_tls_errors(move |_, uri, certificate, errors| {
            if let Some(w) = w.upgrade() {
                *w.tls_failure.borrow_mut() = Some(TlsFailure { generation: w.generation.get(), uri: uri.into(), certificate: certificate.clone(), errors });
                let remote = w.remote.borrow().as_ref().is_some_and(|r| r.source.valid() && r.forward.valid());
                w.show_failure("Certificate could not be verified", if remote {
                    "The remote localhost service uses a certificate that could not be verified. Review its certificate to continue only in this preview. Nothing has been trusted automatically."
                } else {
                    "The server certificate could not be verified. Use your external browser for this address. Certificate approval in Preview is available only for a service opened through its verified SSH terminal."
                }, remote);
            }
            true
        });
        let w = Rc::downgrade(&web);
        session.connect_download_started(move |_,download|{download.cancel();if let Some(w)=w.upgrade(){w.status.set_text("Downloads are disabled in Preview; use Open externally for an explicit browser download.");}});
        web.browser.connect_permission_request(|_, request| {
            request.deny();
            true
        });
        web.browser.connect_run_file_chooser(|_, request| {
            request.cancel();
            true
        });
        let w = Rc::downgrade(&web);
        web.browser.connect_create(move |_, action| {
            if let Some(w) = w.upgrade() {
                if action.is_user_gesture() {
                    if let Some(uri) = action.request().and_then(|r| r.uri()) {
                        w.navigate(&uri, None);
                    }
                }
            }
            None
        });
        let w = Rc::downgrade(&web);
        web.browser.connect_decide_policy(move |_,decision,kind|{
            if !matches!(kind,webkit6::PolicyDecisionType::NavigationAction|webkit6::PolicyDecisionType::NewWindowAction) {return false;}
            let Some(w)=w.upgrade() else {decision.ignore();return true;};
            if w.endpoint.borrow().is_some() && w.remote.borrow().is_none() {
                decision.ignore();w.status.set_text("Remote preview disconnected. Choose its current SSH terminal to reconnect.");return true;
            }
            let uri=decision.downcast_ref::<webkit6::NavigationPolicyDecision>().and_then(|d|d.navigation_action()).and_then(|a|a.request()).and_then(|r|r.uri());
            let Some(uri)=uri.and_then(|u|web_route::parse(&u).ok()) else {decision.ignore();w.status.set_text("Navigation blocked: only HTTP and HTTPS pages are allowed");return true;};
            if let Some(remote)=w.remote.borrow().as_ref() {
                if !remote.source.valid()||!remote.forward.valid(){decision.ignore();w.browser.stop_loading();w.status.set_text("Original SSH connection changed. Reopen the preview explicitly from its terminal.");return true;}
                let mapped=web_route::mapped_to(&remote.original,remote.forward.local_host,remote.forward.local_port);
                if uri.origin()==mapped.origin() {return false;}
                if uri.origin()==remote.original.origin() {let mapped=web_route::mapped_to(&uri,remote.forward.local_host,remote.forward.local_port);decision.ignore();w.browser.load_uri(mapped.as_str());return true;}
                decision.ignore();w.status.set_text("Remote preview stays on its approved origin/port. Open another URL explicitly in a new Web view.");return true;
            }
            false
        });
        let key = gtk::EventControllerKey::new();
        key.set_propagation_phase(gtk::PropagationPhase::Capture);
        let w = Rc::downgrade(&web);
        key.connect_key_pressed(move |_, key, _, mods| {
            if matches!(key, gdk::Key::l | gdk::Key::L)
                && mods.contains(gdk::ModifierType::CONTROL_MASK)
                && !mods.intersects(
                    gdk::ModifierType::ALT_MASK
                        | gdk::ModifierType::SUPER_MASK
                        | gdk::ModifierType::SHIFT_MASK,
                )
            {
                if let Some(w) = w.upgrade() {
                    w.address.grab_focus();
                    w.address.select_region(0, -1);
                }
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
        web.root.add_controller(key);
        let w = Rc::downgrade(&web);
        glib::timeout_add_local(Duration::from_secs(1), move || {
            let Some(w) = w.upgrade() else {
                return glib::ControlFlow::Break;
            };
            if w.closed.get() {
                return glib::ControlFlow::Break;
            }
            let lost = w
                .remote
                .borrow()
                .as_ref()
                .is_some_and(|r| !r.source.valid() || !r.forward.valid());
            if lost {
                w.disconnect();
            }
            glib::ControlFlow::Continue
        });
        web
    }
    fn show_failure(&self, title: &str, detail: &str, review: bool) {
        self.failed.set(true);
        self.failure_title.set_text(title);
        self.failure_detail.set_text(detail);
        self.certificate_button.set_visible(review);
        self.content.set_visible_child_name("error");
        self.status.set_text(title);
    }
    fn review_certificate(self: &Rc<Self>) {
        use gtk::gio::prelude::TlsCertificateExt;
        use sha2::{Digest, Sha256};
        let Some(failure) = self.tls_failure.borrow().clone() else {
            return;
        };
        let Some(origin) = web_route::parse(&failure.uri).ok() else {
            return;
        };
        let Some(host) = origin.host_str().map(str::to_owned) else {
            return;
        };
        let Some(certificate) = failure
            .certificate
            .certificate()
            .filter(|b| !b.is_empty() && b.len() <= 262144)
        else {
            return;
        };
        let fingerprint = format!("{:x}", Sha256::digest(certificate.as_ref()));
        let label = {
            let remote = self.remote.borrow();
            let Some(remote) = remote
                .as_ref()
                .filter(|r| r.source.valid() && r.forward.valid())
            else {
                return;
            };
            format!(
                "{} · {}",
                remote.source.label,
                remote.original.origin().ascii_serialization()
            )
        };
        let dialog = gtk::Dialog::builder()
            .title("Review remote service certificate")
            .modal(true)
            .default_width(520)
            .build();
        dialog.set_widget_name("web-certificate-dialog");
        dialog.set_transient_for(self.root.root().and_downcast::<gtk::Window>().as_ref());
        dialog.add_button("Cancel", gtk::ResponseType::Cancel);
        dialog.add_button("Trust in this preview", gtk::ResponseType::Accept);
        dialog.set_default_response(gtk::ResponseType::Cancel);
        let issues = format!("{}", certificate_issues(failure.errors));
        for (name, text) in [
            ("web-certificate-origin", label),
            ("web-certificate-issues", issues),
            ("web-certificate-fingerprint", format!("SHA-256: {fingerprint}")),
            ("web-certificate-scope", "Verify this fingerprint with the service owner before continuing. Approval applies only to this certificate for this remote service in this preview. It is discarded when the preview closes; no system trust or saved connection is changed.".into()),
        ] {
            let line = gtk::Label::new(Some(&text));
            line.set_widget_name(name);
            line.set_wrap(true);
            line.set_selectable(true);
            line.set_xalign(0.0);
            line.set_margin_start(12);
            line.set_margin_end(12);
            line.set_margin_top(10);
            dialog.content_area().append(&line);
        }
        // WebKit can clear its current URI after a TLS load failure. Bind the
        // approval to the displayed state, while retrying the captured failed URI.
        let reviewed_uri = self.browser.uri();
        let reviewed_address = self.address.text();
        let weak = Rc::downgrade(self);
        dialog.connect_response(move |dialog, response| {
            dialog.close();
            if response != gtk::ResponseType::Accept { return; }
            let Some(web) = weak.upgrade() else { return; };
            let same_failure = web.tls_failure.borrow().as_ref().is_some_and(|current| current.generation == failure.generation && current.uri == failure.uri);
            let live_route = web.remote.borrow().as_ref().is_some_and(|r| r.source.valid() && r.forward.valid() && crate::profiles::fresh_ssh_profile(&r.source.endpoint).is_some_and(|p| crate::launch_context::binding(&p) == r.source.binding));
            if web.closed.get() || !same_failure || !live_route || web.generation.get() != failure.generation || web.browser.uri() != reviewed_uri || web.address.text() != reviewed_address {
                web.status.set_text("The preview or its SSH connection changed. Review the current service certificate again.");
                return;
            }
            if let Some(session) = web.browser.network_session() {
                session.allow_tls_certificate_for_host(&failure.certificate, &host);
                web.browser.load_uri(&failure.uri);
            }
        });
        dialog.present();
    }
    fn choose_source(self: &Rc<Self>) {
        let Some(endpoint) = self.endpoint.borrow().clone() else {
            self.status.set_text(
                "Open a remote localhost link from its SSH terminal with Ctrl+click first",
            );
            return;
        };
        if crate::web_route::parse(self.address.text().as_str()).is_err() {
            self.status.set_text(
                "Enter the original remote HTTP/HTTPS URL, then choose its current SSH terminal",
            );
            return;
        }
        let sources = TERMINALS.with(|v| {
            v.borrow()
                .as_ref()
                .and_then(Weak::upgrade)
                .map(|views| {
                    views
                        .borrow()
                        .iter()
                        .filter_map(Source::capture)
                        .filter(|s| s.endpoint == endpoint && s.valid())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default()
        });
        if sources.is_empty() {
            self.status.set_text(
                "Open this machine's saved SSH terminal first; no replacement route was inferred",
            );
            return;
        }
        let dialog = gtk::Dialog::builder()
            .title("Choose original SSH terminal")
            .modal(true)
            .build();
        if let Some(window) = self.root.root().and_downcast::<gtk::Window>() {
            dialog.set_transient_for(Some(&window));
        }
        for source in sources {
            let button =
                gtk::Button::with_label(&format!("{} · SSH process {}", source.label, source.pid));
            let weak = Rc::downgrade(self);
            let d = dialog.downgrade();
            button.connect_clicked(move |_| {
                if let Some(d) = d.upgrade() {
                    d.close();
                }
                if let Some(w) = weak.upgrade() {
                    let url = w.address.text();
                    w.navigate(&url, Some(source.clone()));
                }
            });
            dialog.content_area().append(&button);
        }
        dialog.add_button("Cancel", gtk::ResponseType::Cancel);
        dialog.connect_response(|d, _| d.close());
        dialog.present();
    }
    fn navigation_ready(&self) -> bool {
        self.remote.borrow().as_ref().map_or_else(
            || self.endpoint.borrow().is_none(),
            |r| r.source.valid() && r.forward.valid(),
        )
    }
    fn disconnect(&self) {
        if let Some(cancel) = self.pending.borrow_mut().take() {
            cancel.store(true, Ordering::Release);
        }
        self.browser.stop_loading();
        if self.remote.borrow_mut().take().is_some() {
            self.browser.terminate_web_process();
        }
        self.status.set_text("Remote preview disconnected. Reopen from the current SSH terminal to revalidate its route.");
    }
    fn navigate(self: &Rc<Self>, text: &str, source: Option<Source>) {
        if let Some(old) = self.pending.borrow_mut().take() {
            old.store(true, Ordering::Release);
        }
        let uri = match web_route::parse(text) {
            Ok(u) => u,
            Err(e) => {
                self.status.set_text(&e);
                return;
            }
        };
        if let Some(remote) = self.remote.borrow().as_ref() {
            let mapped = web_route::mapped_to(
                &remote.original,
                remote.forward.local_host,
                remote.forward.local_port,
            );
            if source.is_none()
                && (uri.origin() == mapped.origin() || uri.origin() == remote.original.origin())
                && remote.source.valid()
                && remote.forward.valid()
            {
                self.browser.load_uri(
                    web_route::mapped_to(
                        &uri,
                        remote.forward.local_host,
                        remote.forward.local_port,
                    )
                    .as_str(),
                );
                return;
            }
            if source.is_none() {
                self.status.set_text("Open a new Web view for a different origin; this preview belongs to its original SSH host.");
                return;
            }
        }
        if source.is_none() && self.endpoint.borrow().is_some() {
            self.status.set_text("Reconnect this remote preview from its SSH terminal; no route is restored automatically.");
            return;
        }
        match (source, web_route::destination(&uri)) {
            (_, Err(e)) => self.status.set_text(&e),
            (Some(source), Ok(Destination::Loopback { host, port })) => {
                if !source.valid() {
                    self.status.set_text("Original SSH pane or saved endpoint changed; open a current SSH terminal first");
                    return;
                }
                if self
                    .binding
                    .borrow()
                    .as_ref()
                    .is_some_and(|binding| binding != &source.binding)
                {
                    self.status.set_text("Connection details changed. Open a new Web Preview from the current SSH terminal.");
                    return;
                }
                self.disconnect();
                *self.binding.borrow_mut() = Some(source.binding.clone());
                self.status.set_text(
                    "Opening the explicit loopback port through the original verified SSH master…",
                );
                let cancel = Arc::new(AtomicBool::new(false));
                *self.pending.borrow_mut() = Some(cancel.clone());
                let (token, pid) = (source.token.clone(), source.pid);
                let (tx, rx) = std::sync::mpsc::sync_channel(1);
                let isolated_tls = uri.scheme() == "https";
                std::thread::spawn(move || {
                    let result = crate::ssh_transport::LocalForward::open_for_preview(
                        &token,
                        pid,
                        &host,
                        port,
                        isolated_tls,
                    );
                    let _ = tx.send(result);
                });
                let weak = Rc::downgrade(self);
                glib::timeout_add_local(Duration::from_millis(20), move || match rx.try_recv() {
                    Ok(result) => {
                        if let Some(w) = weak.upgrade() {
                            if w.closed.get() || cancel.load(Ordering::Acquire) || !source.valid() {
                                return glib::ControlFlow::Break;
                            }
                            w.pending.borrow_mut().take();
                            match result{Ok(forward)=>{let mapped=web_route::mapped_to(&uri,forward.local_host,forward.local_port);*w.endpoint.borrow_mut()=Some(source.endpoint.clone());*w.remote.borrow_mut()=Some(Remote{source:source.clone(),original:uri.clone(),forward});w.browser.load_uri(mapped.as_str());},Err(_)=>w.status.set_text("SSH preview failed. The original master must be live and permit local forwarding; no replacement connection was opened.")}
                        }
                        glib::ControlFlow::Break
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
                    Err(_) => {
                        if let Some(w) = weak.upgrade() {
                            w.status.set_text("SSH preview worker stopped");
                        }
                        glib::ControlFlow::Break
                    }
                });
            }
            (_, Ok(_)) => {
                self.remote.borrow_mut().take();
                self.endpoint.borrow_mut().take();
                self.browser.load_uri(uri.as_str());
            }
        }
    }
    fn close(&self) {
        if self.closed.replace(true) {
            return;
        }
        if let Some(cancel) = self.pending.borrow_mut().take() {
            cancel.store(true, Ordering::Release);
        }
        self.disconnect();
        self.browser.terminate_web_process();
    }
}
pub fn open(tabs: &adw::TabView, url: Option<&str>, source: Option<Source>) -> Result<(), String> {
    let web = register(None)?;
    let page = crate::tiles::append(tabs, &web.root);
    page.set_title("Web Preview");
    tabs.set_selected_page(&page);
    if let Some(url) = url {
        web.navigate(url, source);
    } else {
        web.address.grab_focus();
    }
    Ok(())
}
/// The main preview action chooses an existing verified terminal, never a new host.
pub fn service_picker(tabs: &adw::TabView) {
    let views = TERMINALS.with(|all| {
        all.borrow()
            .as_ref()
            .and_then(Weak::upgrade)
            .map(|all| {
                all.borrow()
                    .iter()
                    .filter(|v| Source::capture(v).is_some_and(|source| source.valid()))
                    .cloned()
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    });
    let selected = tabs.selected_page();
    let current = views
        .iter()
        .filter(|v| {
            selected
                .as_ref()
                .is_some_and(|page| crate::tiles::contains(&page.child(), &v.root))
        })
        .collect::<Vec<_>>();
    if current.len() == 1 {
        service_dialog(current[0]);
        return;
    }
    let dialog = gtk::Dialog::builder()
        .title("Choose remote terminal for Web Preview")
        .modal(true)
        .default_width(440)
        .build();
    dialog.set_widget_name("web-source-picker");
    dialog.set_transient_for(tabs.root().and_downcast::<gtk::Window>().as_ref());
    let text = gtk::Label::new(Some(if views.is_empty() {
        "Open an SSH terminal from Machines first. Web Preview opens a localhost service on that terminal's remote machine, without RDP."
    } else {
        "Choose the terminal whose remote machine runs the localhost service. This preview will stay bound to that SSH connection."
    }));
    text.set_wrap(true);
    text.set_margin_start(12);
    text.set_margin_end(12);
    text.set_margin_top(12);
    dialog.content_area().append(&text);
    for view in views {
        let title = crate::tiles::page(tabs, &view.root)
            .map(|page| page.title().to_string())
            .unwrap_or_else(|| "Remote terminal".into());
        let button = gtk::Button::with_label(&title);
        let weak = dialog.downgrade();
        button.connect_clicked(move |_| {
            if let Some(dialog) = weak.upgrade() {
                dialog.close();
            }
            if Source::capture(&view).is_some_and(|source| source.valid()) {
                service_dialog(&view);
            }
        });
        dialog.content_area().append(&button);
    }
    dialog.add_button("Cancel", gtk::ResponseType::Cancel);
    dialog.connect_response(|dialog, _| dialog.close());
    dialog.present();
}
/// "Open remote web service…" on an already opened, verified SSH terminal: a
/// port (and optional path) on that machine's own loopback, previewed through
/// this pane's original SSH transport. No new SSH connection, endpoint or
/// listener beyond the usual ephemeral local-loopback forward is created.
pub fn service_dialog(view: &Rc<View>) {
    let Some(source) = Source::capture(view) else {
        view.status.set_text(
            "Remote web services open from a terminal launched through a saved, verified SSH machine",
        );
        return;
    };
    let Some(tabs) = view
        .root
        .ancestor(adw::TabView::static_type())
        .and_downcast::<adw::TabView>()
    else {
        view.status.set_text("Web Preview requires a workspace tab");
        return;
    };
    let dialog = gtk::Dialog::builder()
        .title("Open remote web service")
        .modal(true)
        .default_width(440)
        .build();
    dialog.set_transient_for(view.area.root().and_downcast::<gtk::Window>().as_ref());
    dialog.set_widget_name("remote-service");
    dialog.add_button("Cancel", gtk::ResponseType::Cancel);
    dialog.add_button("Open", gtk::ResponseType::Accept);
    dialog.set_default_response(gtk::ResponseType::Accept);
    let port = gtk::Entry::builder()
        .placeholder_text("Port on that machine, e.g. 5173")
        .input_purpose(gtk::InputPurpose::Digits)
        .activates_default(true)
        .hexpand(true)
        .build();
    port.set_widget_name("remote-service-port");
    let path = gtk::Entry::builder()
        .placeholder_text("Optional path, e.g. /app")
        .activates_default(true)
        .build();
    path.set_widget_name("remote-service-path");
    let https = gtk::CheckButton::with_label("HTTPS");
    https.set_widget_name("remote-service-https");
    let summary = gtk::Label::new(Some(&format!(
        "Opens localhost on {} through this terminal's verified SSH connection (process {}). \
         The service is forwarded only to this computer's loopback while the preview is open; \
         no other machine or network address is used.",
        source.label, source.pid
    )));
    summary.set_widget_name("remote-service-summary");
    summary.set_wrap(true);
    summary.set_xalign(0.0);
    let info = gtk::Label::new(None);
    info.set_widget_name("remote-service-info");
    info.set_wrap(true);
    for w in [
        summary.upcast_ref::<gtk::Widget>(),
        port.upcast_ref(),
        path.upcast_ref(),
        https.upcast_ref(),
        info.upcast_ref(),
    ] {
        w.set_margin_start(12);
        w.set_margin_end(12);
        w.set_margin_top(8);
        dialog.content_area().append(w);
    }
    let focus = port.clone();
    dialog.connect_response(move |d, response| {
        if response != gtk::ResponseType::Accept {
            d.close();
            return;
        }
        let uri = match web_route::service_url(&port.text(), &path.text(), https.is_active()) {
            Ok(uri) => uri,
            Err(e) => {
                info.set_text(&e);
                return;
            }
        };
        if !source.valid() {
            info.set_text("This terminal's SSH connection or saved machine changed; open a current SSH terminal first");
            return;
        }
        match open(&tabs, Some(uri.as_str()), Some(source.clone())) {
            Ok(()) => d.close(),
            Err(e) => info.set_text(&e),
        }
    });
    dialog.present();
    focus.grab_focus();
}
fn register(endpoint: Option<String>) -> Result<Rc<Web>, String> {
    VIEWS.with(|views| {
        let mut views = views.borrow_mut();
        views.retain(|w| !w.closed.get());
        if views.len() >= MAX_VIEWS {
            return Err("Close a Web view before opening another (limit 8)".into());
        }
        let web = Web::new(endpoint);
        views.push(web.clone());
        Ok(web)
    })
}
pub fn restore(endpoint: Option<String>) -> gtk::Widget {
    match register(endpoint) {
        Ok(w) => w.root.clone().upcast(),
        Err(e) => gtk::Label::new(Some(&e)).upcast(),
    }
}
pub fn describe(root: &gtk::Widget) -> Option<Layout> {
    VIEWS.with(|v| {
        v.borrow()
            .iter()
            .find(|w| w.root.upcast_ref::<gtk::Widget>() == root)
            .map(|w| Layout::Web {
                view_id: root.widget_name().into(),
                endpoint: w.endpoint.borrow().clone(),
            })
    })
}
pub fn close_tree(root: &gtk::Widget) {
    VIEWS.with(|views| {
        let mut views = views.borrow_mut();
        views.retain(|w| {
            if crate::tiles::contains(root, &w.root) {
                w.close();
                false
            } else {
                true
            }
        });
    });
}

/// Available only from the existing private, explicit GUI fixture channel.
pub fn fixture(tabs: &adw::TabView, q: &serde_json::Value) -> serde_json::Value {
    use serde_json::json;
    if q["operation"] == "open" {
        let uri = q["url"].as_str().unwrap_or("");
        if !web_route::parse(uri)
            .is_ok_and(|u| matches!(u.host(),Some(url::Host::Ipv4(ip)) if ip.is_loopback()))
        {
            return json!({"error":"Owned loopback fixture URL required"});
        }
        if let Err(e) = open(tabs, Some(uri), None) {
            return json!({"error":e});
        }
    }
    if q["operation"] == "service-picker" {
        service_picker(tabs);
        return json!({"opened":true});
    }
    if q["operation"] == "remote-service" {
        // Drives the real dialog of the terminal whose foreground SSH pane has this PID.
        let pid = q["pid"].as_u64().unwrap_or(0);
        let view = TERMINALS.with(|v| {
            v.borrow()
                .as_ref()
                .and_then(Weak::upgrade)
                .and_then(|views| {
                    views
                        .borrow()
                        .iter()
                        .find(|view| Source::capture(view).is_some_and(|s| u64::from(s.pid) == pid))
                        .cloned()
                })
        });
        let Some(view) = view else {
            return json!({"error":"No verified SSH terminal with that process"});
        };
        if q["existing"] != true {
            service_dialog(&view);
        }
        let mut result = json!({"status": view.status.text().as_str()});
        for top in gtk::Window::list_toplevels() {
            let Some(dialog) = top.downcast_ref::<gtk::Dialog>() else {
                continue;
            };
            if dialog.widget_name() != "remote-service" {
                continue;
            }
            let texts = RefCell::new(serde_json::Map::new());
            crate::chrome::walk(dialog.upcast_ref(), &|w| {
                let name = w.widget_name().to_string();
                if let Some(e) = w.downcast_ref::<gtk::Entry>() {
                    if let Some(value) = q["fields"][&name].as_str() {
                        e.set_text(value);
                    }
                }
                if let Some(c) = w.downcast_ref::<gtk::CheckButton>() {
                    if let Some(value) = q["fields"][&name].as_bool() {
                        c.set_active(value);
                    }
                }
                if let Some(l) = w.downcast_ref::<gtk::Label>() {
                    if name.starts_with("remote-service-") {
                        texts.borrow_mut().insert(name, json!(l.text().as_str()));
                    }
                }
            });
            if q["respond"] == "open" {
                dialog.response(gtk::ResponseType::Accept);
            }
            if q["respond"] == "cancel" {
                dialog.response(gtk::ResponseType::Cancel);
            }
            let texts = texts.into_inner();
            // After a refused "Open" the dialog stays with its message; read it again.
            let info = RefCell::new(String::new());
            crate::chrome::walk(dialog.upcast_ref(), &|w| {
                if w.widget_name() == "remote-service-info" {
                    if let Some(l) = w.downcast_ref::<gtk::Label>() {
                        *info.borrow_mut() = l.text().to_string();
                    }
                }
            });
            let info = info.into_inner();
            result = json!({"dialog": texts, "info": info, "open": dialog.is_visible()});
            if q["respond"] == "open" && dialog.is_visible() {
                dialog.close();
            }
        }
        return result;
    }
    VIEWS.with(|views|{let views=views.borrow();let Some(web)=views.get(q["index"].as_u64().unwrap_or(0) as usize) else{return json!({"count":views.len()});};
        if q["operation"]=="focus" {
            if let Some(page)=crate::tiles::page(tabs,&web.root){tabs.set_selected_page(&page);}
            web.browser.grab_focus();
        }
        if q["operation"]=="address" {
            let text=q["text"].as_str().unwrap_or("");
            if let Err(error)=web_route::parse(text){return json!({"error":error});}
            web.address.set_text(text);
        }
        if q["operation"]=="certificate-review" {web.review_certificate();}
        if q["operation"]=="certificate-response" {
            for top in gtk::Window::list_toplevels() {
                if let Some(dialog) = top.downcast_ref::<gtk::Dialog>().filter(|d| d.widget_name() == "web-certificate-dialog") {
                    dialog.response(if q["accept"] == true {gtk::ResponseType::Accept} else {gtk::ResponseType::Cancel});
                }
            }
        }
        if q["operation"]=="choose-source" {web.choose_source();}
        if q["operation"]=="control" {
            let name=q["name"].as_str().unwrap_or("");
            if !["web-back","web-forward","web-reload","web-reconnect"].contains(&name) {return json!({"error":"Unknown preview control"});}
            crate::chrome::walk(web.root.upcast_ref(),&|w| {if w.widget_name()==name {if let Some(button)=w.downcast_ref::<gtk::Button>() {button.emit_clicked();}}});
        }
        if q["operation"]=="choose-terminal" {
            let suffix=format!("SSH process {}",q["pid"].as_u64().unwrap_or(0));
            let chosen=Cell::new(false);
            for window in gtk::Window::list_toplevels(){
                if let Ok(window)=window.downcast::<gtk::Window>() {
                    if window.title().as_deref()==Some("Choose original SSH terminal") {
                        crate::chrome::walk(window.upcast_ref(),&|w|{
                            if let Some(button)=w.downcast_ref::<gtk::Button>() {
                                if !chosen.get()&&button.label().is_some_and(|label|label.ends_with(&suffix))&&button.is_sensitive(){chosen.set(true);button.emit_clicked();}
                            }
                        });
                    }
                }
            }
            if !chosen.get(){return json!({"error":"Matching SSH chooser button unavailable"});}
        }
        if q["operation"]=="script" {
            if !web.browser.uri().and_then(|u|web_route::parse(&u).ok()).is_some_and(|u|matches!(u.host(),Some(url::Host::Ipv4(ip)) if ip.is_loopback())) {return json!({"error":"Owned loopback page required"});}
            let script=q["script"].as_str().unwrap_or("");if script.len()>8192{return json!({"error":"Fixture script limit"});}
            SCRIPT_RESULT.with(|v|v.borrow_mut().take());
            web.browser.evaluate_javascript(script,None,None,None::<&gtk::gio::Cancellable>,|result|{SCRIPT_RESULT.with(|v|*v.borrow_mut()=Some(result.ok().and_then(|v|v.to_json(0)).and_then(|s|serde_json::from_str(s.as_str()).ok()).unwrap_or(serde_json::Value::Null)));});
        }
        let window=web.root.root().and_downcast::<gtk::Window>();
        let focused=window.as_ref().and_then(gtk::prelude::GtkWindowExt::focus);
        json!({"script_result":SCRIPT_RESULT.with(|v|v.borrow().clone()),"count":views.len(),"failed":web.failed.get(),"error_visible":web.content.visible_child_name().as_deref()==Some("error"),"certificate_review_available":web.certificate_button.is_visible(),"failure_detail":web.failure_detail.text().as_str(),"status":web.status.text().as_str(),"loading":web.browser.is_loading(),"ephemeral":web.browser.network_session().is_some_and(|s|s.is_ephemeral()),"remote":web.remote.borrow().is_some(),"layout":describe(web.root.upcast_ref()),"origin":web.browser.uri().and_then(|u|web_route::parse(&u).ok()).map(|u|u.origin().ascii_serialization()),"address_focused":focused.as_ref().is_some_and(|f|crate::tiles::contains(&web.address,f)),"browser_focused":focused.as_ref().is_some_and(|f|crate::tiles::contains(&web.browser,f)),"browser_origin":window.as_ref().and_then(|w|web.browser.compute_point(w,&gtk::graphene::Point::new(0.,0.))).map(|p|[p.x(),p.y()])})
    })
}
