//! Native IronRDP transport and GTK framebuffer, independent of the PTY daemon.
//! No competitor source is copied; IronRDP is MIT OR Apache-2.0.
use crate::cliprdr::{Backend, Bridge, Event, Format};
use crate::rdp_trust::{Review, Trust};
use adw::prelude::*;
use gtk::{gdk, glib};

/// Local actions belong to the tab; right-click on the framebuffer stays remote input.
pub fn install_tab_menu(
    window: &adw::ApplicationWindow,
    tabs: &adw::TabView,
    views: Rc<RefCell<Vec<Rc<RdpView>>>>,
    terminals: Rc<RefCell<Vec<Rc<crate::view::View>>>>,
) -> gtk::gio::SimpleActionGroup {
    let target: Rc<RefCell<Option<std::rc::Weak<RdpView>>>> = Rc::new(RefCell::new(None));
    let terminal: Rc<RefCell<Option<std::rc::Weak<crate::view::View>>>> =
        Rc::new(RefCell::new(None));
    let page_target: Rc<RefCell<Option<adw::TabPage>>> = Rc::new(RefCell::new(None));
    let menu = gtk::gio::Menu::new();
    let group = gtk::gio::SimpleActionGroup::new();
    let mut actions = Vec::new();
    for (name, label) in [
        ("reconnect", "Reconnect"),
        ("retry-sign-in", "Retry automatic sign-in"),
        ("recover-sign-in", "Recover GNOME login…"),
        ("dynamic", "Dynamic resolution"),
        ("fit", "Fit to window"),
        ("native", "Original size"),
        ("disconnect", "Disconnect"),
    ] {
        let action = gtk::gio::SimpleAction::new(name, None);
        action.set_enabled(false);
        let selected = target.clone();
        action.connect_activate(move |_, _| {
            if let Some(view) = selected.borrow().as_ref().and_then(std::rc::Weak::upgrade) {
                match name {
                    "reconnect" => {
                        if let Err(error) = view.automation_reconnect() {
                            view.status.set_text(&error);
                        }
                    }
                    "disconnect" => view.close(),
                    "retry-sign-in" => view.retry_auto_gdm(),
                    "recover-sign-in" => view.recover_sign_in(),
                    mode => view.set_display(mode, 1.0, 0, 0),
                }
            }
        });
        group.add_action(&action);
        menu.append(Some(label), Some(&format!("connection.{name}")));
        actions.push(action);
    }
    let terminal_menu = gtk::gio::Menu::new();
    let mut terminal_actions = Vec::new();
    for (name, label) in [
        ("copy", "Copy"),
        ("paste", "Paste"),
        ("paste-selection", "Paste selection"),
        ("select-text", "Select text…"),
    ] {
        let action = gtk::gio::SimpleAction::new(name, None);
        action.set_enabled(false);
        let selected = terminal.clone();
        let weak_tabs = tabs.downgrade();
        let selected_page = page_target.clone();
        action.connect_activate(move |_, _| {
            let Some(view) = selected.borrow().as_ref().and_then(std::rc::Weak::upgrade) else {
                return;
            };
            if let (Some(tabs), Some(page)) = (weak_tabs.upgrade(), selected_page.borrow().as_ref())
            {
                tabs.set_selected_page(page);
            }
            view.focus();
            match name {
                "copy" => view.copy_clipboard(false),
                "paste" => view.paste_desktop(),
                "paste-selection" => view.paste_clipboard(true),
                _ => view.open_history(),
            }
        });
        group.add_action(&action);
        terminal_menu.append(Some(label), Some(&format!("connection.{name}")));
        terminal_actions.push((name, action));
    }
    let close = gtk::gio::SimpleAction::new("close-tab", None);
    let weak_tabs = tabs.downgrade();
    let selected_page = page_target.clone();
    close.connect_activate(move |_, _| {
        // Clone before closing: closing the menu may clear its selected page.
        let page = selected_page.borrow().clone();
        if let (Some(tabs), Some(page)) = (weak_tabs.upgrade(), page) {
            tabs.close_page(&page);
        }
    });
    group.add_action(&close);
    let detach = gtk::gio::SimpleAction::new("detach-tab", None);
    let weak_tabs = tabs.downgrade();
    let selected_page = page_target.clone();
    detach.connect_activate(move |_, _| {
        let page = selected_page.borrow().clone();
        if let (Some(tabs), Some(page)) = (weak_tabs.upgrade(), page) {
            if let Some(destination) =
                tabs.emit_by_name::<Option<adw::TabView>>("create-window", &[])
            {
                tabs.transfer_page(&page, &destination, 0);
            }
        }
    });
    group.add_action(&detach);
    let general = gtk::gio::Menu::new();
    general.append(
        Some("Move tab to new window"),
        Some("connection.detach-tab"),
    );
    general.append(Some("Close tab"), Some("connection.close-tab"));
    menu.append_section(None, &general);
    terminal_menu.append_section(None, &general);
    window.insert_action_group("connection", Some(&group));
    // libadwaita captures this model before emitting setup-menu and caches its
    // popover. Keep its identity stable; update its sections for each tab.
    let context_menu = gtk::gio::Menu::new();
    context_menu.append_section(None, &general);
    tabs.set_menu_model(Some(&context_menu));
    tabs.connect_setup_menu(move |tabs, page| {
        let selected = page.and_then(|page| {
            views
                .borrow()
                .iter()
                .find(|view| crate::tiles::contains(&page.child(), &view.root))
                .cloned()
        });
        let selected_terminal = page.and_then(|page| {
            let matches = terminals
                .borrow()
                .iter()
                .filter(|view| crate::tiles::contains(&page.child(), &view.root))
                .cloned()
                .collect::<Vec<_>>();
            // A split tab can contain multiple terminals: use its actual input focus.
            matches
                .iter()
                .find(|view| view.area.has_focus() || view.copy.has_focus())
                .cloned()
                .or_else(|| {
                    if matches.len() == 1 {
                        matches.first().cloned()
                    } else {
                        None
                    }
                })
        });
        for action in &actions {
            action.set_enabled(selected.as_ref().is_some_and(|view| {
                action.name() != "retry-sign-in" || view.auto_gdm_retry_available()
            }));
        }
        for (name, action) in &terminal_actions {
            action.set_enabled(
                selected_terminal
                    .as_ref()
                    .is_some_and(|view| view.clipboard_action_enabled(name)),
            );
        }
        close.set_enabled(page.is_some());
        detach.set_enabled(page.is_some());
        *page_target.borrow_mut() = page.cloned();
        *target.borrow_mut() = selected.as_ref().map(Rc::downgrade);
        *terminal.borrow_mut() = selected_terminal.as_ref().map(Rc::downgrade);
        if page.is_some() {
            context_menu.remove_all();
            context_menu.append_section(
                None,
                if selected_terminal.is_some() {
                    &terminal_menu
                } else if selected.is_some() {
                    &menu
                } else {
                    &general
                },
            );
        }
    });
    group
}

pub fn menu_fixture(
    window: &adw::ApplicationWindow,
    tabs: &adw::TabView,
    actions: &gtk::gio::SimpleActionGroup,
    q: &serde_json::Value,
) -> serde_json::Value {
    let index = q["tab"].as_i64().unwrap_or(0) as i32;
    if index < 0 || index >= tabs.n_pages() {
        return serde_json::json!({"error":"Unknown fixture tab"});
    }
    let page = tabs.nth_page(index);
    // Model captured in the same order as libadwaita's actual do_popup.
    let presented_model = tabs.menu_model();
    tabs.emit_by_name::<()>("setup-menu", &[&Some(page.clone())]);
    fn labels(model: &gtk::gio::MenuModel) -> Vec<String> {
        let mut result = Vec::new();
        for i in 0..model.n_items() {
            if let Some(label) = model
                .item_attribute_value(i, "label", None)
                .and_then(|v| v.get::<String>())
            {
                result.push(label);
            }
            if let Some(section) = model.item_link(i, "section") {
                result.extend(labels(&section));
            }
        }
        result
    }
    let menu = tabs.menu_model().map(|m| labels(&m)).unwrap_or_default();
    let presented_menu = presented_model.as_ref().map(labels).unwrap_or_default();
    let stable_model = presented_model == tabs.menu_model();
    let states = actions
        .list_actions()
        .iter()
        .map(|a| {
            (
                a.to_string(),
                serde_json::json!(actions.is_action_enabled(a)),
            )
        })
        .collect::<serde_json::Map<_, _>>();
    let control = q["control"].as_str().unwrap_or("fit");
    if states.contains_key(control) {
        let _ = gtk::prelude::WidgetExt::activate_action(
            window,
            &format!("connection.{control}"),
            None,
        );
    }
    let value = serde_json::json!({"title":page.title().to_string(),"status_icon":page.indicator_icon().is_some(),"icon":page.icon().is_some(),"menu":menu,"presented_menu":presented_menu,"stable_model":stable_model,"enabled":states});
    tabs.emit_by_name::<()>("setup-menu", &[&None::<adw::TabPage>]);
    value
}

use ironrdp::{
    connector,
    pdu::{
        gcc::KeyboardType,
        input::{
            MousePdu,
            fast_path::{FastPathInputEvent, KeyboardFlags},
            mouse::PointerFlags,
        },
        rdp::{
            capability_sets::MajorPlatformType,
            client_info::{PerformanceFlags, TimezoneInfo},
        },
    },
    session::{ActiveStageBuilder, ActiveStageOutput, image::DecodedImage},
};
use ironrdp_cliprdr::{
    CliprdrClient,
    pdu::{ClipboardFormat, ClipboardFormatId, FormatDataResponse},
};
use ironrdp_tokio::FramedWrite;
use ruwac_clipboard::{ClipPolicy, ClipScope, OfferDecision, RemoteOffer};
use std::{
    cell::{Cell, RefCell},
    collections::HashSet,
    rc::Rc,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};
use x509_cert::der::Decode;
/// ironrdp-displaycontrol 0.8 expects bare capabilities in its client processor,
/// while the server and MS-RDPEDISP encode an eight-byte DISPLAYCONTROL_HEADER.
/// Validate that header, then delegate capabilities and layout encoding upstream.
struct DisplayControlAdapter(
    ironrdp::displaycontrol::client::DisplayControlClient,
    Option<Arc<Mutex<State>>>,
);
ironrdp_core::impl_as_any!(DisplayControlAdapter);
impl ironrdp::dvc::DvcClientProcessor for DisplayControlAdapter {}
impl ironrdp::dvc::DvcProcessor for DisplayControlAdapter {
    fn channel_name(&self) -> &str {
        ironrdp::displaycontrol::CHANNEL_NAME
    }
    fn start(&mut self, id: u32) -> ironrdp::pdu::PduResult<Vec<ironrdp::dvc::DvcMessage>> {
        use ironrdp::dvc::DvcProcessor;
        if let Some(state) = &self.1 {
            state.lock().unwrap().display_channel_opened = true;
        }
        self.0.start(id)
    }
    fn process(
        &mut self,
        id: u32,
        payload: &[u8],
    ) -> ironrdp::pdu::PduResult<Vec<ironrdp::dvc::DvcMessage>> {
        use ironrdp::dvc::DvcProcessor;
        if let Some(state) = &self.1 {
            state.lock().unwrap().display_capability_messages += 1;
        }
        if payload.len() != 20 || u32::from_le_bytes(payload[4..8].try_into().unwrap()) != 20 {
            return Err(ironrdp::pdu::pdu_other_err!(
                "DisplayControl",
                "Invalid capabilities length"
            ));
        }
        let pdu = ironrdp_core::decode::<ironrdp::displaycontrol::pdu::DisplayControlPdu>(payload)
            .map_err(|e| ironrdp::pdu::decode_err!(e))?;
        if !matches!(
            pdu,
            ironrdp::displaycontrol::pdu::DisplayControlPdu::Caps(_)
        ) {
            return Err(ironrdp::pdu::pdu_other_err!(
                "DisplayControl",
                "Expected capabilities"
            ));
        }
        self.0.process(id, &payload[8..])
    }
}
/// Bound SVC and DVC reassembly before the upstream processor can accumulate bytes.
#[derive(Default)]
struct DisplayFragmentGuard {
    svc: crate::cliprdr::FragmentGuard,
    data: Vec<u8>,
    gfx_channel: Option<u32>,
}
impl DisplayFragmentGuard {
    fn check(&mut self, payload: &[u8]) -> Result<(), String> {
        use ironrdp::dvc::pdu::{DrdynvcDataPdu, DrdynvcServerPdu};
        use ironrdp::pdu::rdp::vc::{ChannelControlFlags, ChannelPduHeader};
        let mut cursor = ironrdp_core::ReadCursor::new(payload);
        let header: ChannelPduHeader =
            ironrdp_core::decode_cursor(&mut cursor).map_err(|_| "Invalid display fragment")?;
        if header.length > 16 * 1024 * 1024 {
            return Err("Display SVC reassembly exceeds cap".into());
        }
        self.svc
            .check(payload)
            .map_err(|_| "Invalid display SVC sequence")?;
        if header.flags.contains(ChannelControlFlags::FLAG_FIRST) {
            self.data.clear();
        }
        self.data.extend_from_slice(cursor.remaining());
        if header.flags.contains(ChannelControlFlags::FLAG_LAST) {
            let message = ironrdp_core::decode::<DrdynvcServerPdu>(&self.data)
                .map_err(|_| "Invalid display DVC message")?;
            let oversize = match message {
                DrdynvcServerPdu::Create(p) => {
                    if p.channel_name() == ironrdp_egfx::CHANNEL_NAME {
                        self.gfx_channel = Some(p.channel_id());
                    }
                    false
                }
                DrdynvcServerPdu::Close(p) => {
                    if self.gfx_channel == Some(p.channel_id()) {
                        self.gfx_channel = None;
                    }
                    false
                }
                DrdynvcServerPdu::Data(ref p) => {
                    let limit = if self.gfx_channel == Some(p.channel_id()) {
                        crate::gfx::MAX_WIRE
                    } else {
                        20
                    };
                    match p {
                        DrdynvcDataPdu::DataFirst(p) => {
                            p.length() as usize > limit || p.data().len() > limit
                        }
                        DrdynvcDataPdu::Data(p) => p.data().len() > limit,
                    }
                }
                _ => false,
            };
            self.data.clear();
            if oversize {
                return Err("Display capability DVC reassembly exceeds cap".into());
            }
        }
        Ok(())
    }
}
struct SensitiveUnits(Vec<u16>);
impl Drop for SensitiveUnits {
    fn drop(&mut self) {
        self.0.fill(0);
    }
}
struct SensitiveText(String);
impl Drop for SensitiveText {
    fn drop(&mut self) {
        unsafe {
            self.0.as_bytes_mut().fill(0);
        }
    }
}
enum Command {
    Assisted(SensitiveText, Arc<AtomicU64>, u64, u64, bool),
    Input(Vec<FastPathInputEvent>, Arc<AtomicU64>, u64, u64),
    Release(Vec<FastPathInputEvent>),
    Resize(u16, u16, u32),
    Copy(Vec<ClipboardFormat>),
    Request(ClipboardFormatId),
    Reply(Option<Vec<u8>>),
}
#[derive(Clone)]
struct Profile {
    host: String,
    port: u16,
    user: String,
    reference: String,
    ca: Option<String>,
    reconnect: bool,
    desktop_size: (u16, u16),
    graphics: bool,
    clipboard: bool,
    handover: Option<Arc<crate::handover::Plan>>,
}
impl Profile {
    fn next_attempt(&mut self) -> Self {
        let mut connection = self.clone();
        // A server-issued handover credential is consumed by one connection only.
        // Any ordinary retry starts again with the saved service login.
        connection.handover = self.handover.take();
        connection
    }
}
/// Observe only protocol boundaries, never packet contents or credentials.
struct ProgressTls<S> {
    stream: S,
    state: Arc<Mutex<State>>,
}
impl<S: tokio::io::AsyncRead + Unpin> tokio::io::AsyncRead for ProgressTls<S> {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.stream).poll_read(cx, buf)
    }
}
impl<S: tokio::io::AsyncWrite + Unpin> tokio::io::AsyncWrite for ProgressTls<S> {
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        let result = std::pin::Pin::new(&mut self.stream).poll_write(cx, buf);
        if matches!(result,std::task::Poll::Ready(Ok(n))if n>0) {
            let stage = match buf.first() {
                Some(0x30) => Some("CredSSP exchange"),
                Some(0x03) => Some("RDP session negotiation / licensing / activation"),
                _ => None,
            };
            if let Some(stage) = stage {
                self.state.lock().unwrap().connection_stage = stage.into();
            }
        }
        result
    }
    fn poll_flush(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.stream).poll_flush(cx)
    }
    fn poll_shutdown(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.stream).poll_shutdown(cx)
    }
}
struct Frame {
    generation: u64,
    width: u16,
    height: u16,
    pixels: Vec<u8>,
}
#[derive(Default)]
struct State {
    handover: Option<Arc<crate::handover::Plan>>,
    handovers: u32,
    generation: u64,
    connection_stage: String,
    connection_attempt: u64,
    phase: String,
    frame: Option<Frame>,
    frames: u64,
    connected_since: Option<std::time::Instant>,
    reconnect_attempt: u32,
    reconnect_elapsed: Duration,
    reconnect_background: bool,
    display_cap: Option<u64>,
    display_channel_opened: bool,
    display_capability_messages: u64,
    clipboard_channel_negotiated: bool,
    display_request: Option<(u16, u16)>,
    desktop_size: Option<(u16, u16)>,
    decoded_sizes: std::collections::VecDeque<(u16, u16)>,
    reactivating: bool,
    cadence: std::collections::VecDeque<std::time::Instant>,
    bridge: Bridge,
    status: Option<String>,
    certificate: Option<Review>,
    verified_tls_sha256: Option<String>,
    authentication_error: Option<String>,
    protocol_error: Option<String>,
}
/// The permit covers channel and deferred entries, including payload bytes.
/// Dropping/rejecting a command releases it without replaying rejected input.
#[derive(Default)]
struct QueueBudget {
    count: usize,
    bytes: usize,
}
struct QueuePermit {
    budget: Arc<Mutex<QueueBudget>>,
    bytes: usize,
}
impl Drop for QueuePermit {
    fn drop(&mut self) {
        let mut b = self.budget.lock().unwrap();
        b.count -= 1;
        b.bytes -= self.bytes;
    }
}
struct QueuedCommand {
    command: Command,
    _permit: QueuePermit,
    created: std::time::Instant,
}
impl Command {
    fn bytes(&self) -> usize {
        match self {
            Self::Input(e, ..) | Self::Release(e) => e.len().saturating_mul(32),
            Self::Reply(Some(b)) => b.len(),
            Self::Assisted(t, ..) => t.0.len(),
            Self::Copy(f) => f.len().saturating_mul(1024),
            _ => 64,
        }
    }
    fn stale(&self, generation: u64) -> bool {
        matches!(self, Self::Input(_, epoch, expected, transport) if epoch.load(Ordering::Acquire)!=*expected || *transport!=generation)
    }
}
impl QueueBudget {
    fn reserve(budget: &Arc<Mutex<Self>>, command: Command) -> Result<QueuedCommand, String> {
        let bytes = command.bytes().saturating_add(128);
        let input = matches!(command, Command::Input(..));
        let mut b = budget.lock().unwrap();
        let (count_cap, byte_cap) = if input {
            (4064, 16 * 1024 * 1024)
        } else {
            (4096, 80 * 1024 * 1024)
        };
        if b.count >= count_cap || bytes > byte_cap - b.bytes.min(byte_cap) {
            return Err("RDP queue is full; input was not queued".into());
        }
        b.count += 1;
        b.bytes += bytes;
        drop(b);
        Ok(QueuedCommand {
            command,
            _permit: QueuePermit {
                budget: budget.clone(),
                bytes,
            },
            created: std::time::Instant::now(),
        })
    }
}
struct Connection {
    id: String,
    tx: tokio::sync::mpsc::Sender<QueuedCommand>,
    stop: Arc<AtomicBool>,
    state: Arc<Mutex<State>>,
    /// Commands waiting while the session queue is briefly full (slow link),
    /// kept in order; and since when the backlog has not drained.
    backlog: RefCell<std::collections::VecDeque<QueuedCommand>>,
    budget: Arc<Mutex<QueueBudget>>,
    input_epoch: Arc<AtomicU64>,
}
impl Connection {
    fn start(p: Profile) -> Self {
        Self::start_with_secret(p, None)
    }
    fn start_with_secret(p: Profile, fixture_secret: Option<String>) -> Self {
        let (tx, rx) = tokio::sync::mpsc::channel(128);
        let stop = Arc::new(AtomicBool::new(false));
        let state = Arc::new(Mutex::new(State {
            phase: "connecting".into(),
            ..State::default()
        }));
        let (halt, st) = (stop.clone(), state.clone());
        std::thread::spawn(move || {
            let result = (|| -> Result<(), String> {
                let password = if let Some(secret) = fixture_secret {
                    secret
                } else {
                    crate::credentials::lookup(&p.reference)?
                };
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .map_err(|_| "RDP runtime unavailable".to_string())?;
                rt.block_on(run(p, password, rx, halt, st.clone()))
            })();
            let mut state = st.lock().unwrap();
            state.phase = if result.is_err() {
                "error"
            } else {
                "disconnected"
            }
            .into();
            let failure = result.err();
            // GTK consumes the transient status label. Retain the sanitized
            // terminal failure for authenticated diagnostics after that update.
            if state.authentication_error.is_none()
                && state.certificate.is_none()
                && state.protocol_error.is_none()
            {
                state.protocol_error = failure.clone();
            }
            state.status = Some(
                failure
                    .or_else(|| state.protocol_error.clone())
                    .unwrap_or_else(|| "RDP disconnected".into()),
            );
        });
        Self {
            id: ruwac_protocol::id("rdp-transport"),
            tx,
            stop,
            state,
            backlog: RefCell::default(),
            budget: Arc::default(),
            input_epoch: Arc::default(),
        }
    }
    fn send(&self, events: Vec<FastPathInputEvent>) -> Result<(), String> {
        // Pointer motion is superseded by the next position. While the link
        // is slow, drop it to keep room for clicks, keys and control commands.
        let motion = events.iter().all(
            |e| matches!(e, FastPathInputEvent::MouseEvent(m) if m.flags == PointerFlags::MOVE || m.flags.intersects(PointerFlags::VERTICAL_WHEEL | PointerFlags::HORIZONTAL_WHEEL)),
        );
        if motion && (self.tx.capacity() < 32 || !self.backlog.borrow().is_empty()) {
            return Ok(());
        }
        let generation = self.state.lock().unwrap().generation;
        self.queue(Command::Input(
            events,
            self.input_epoch.clone(),
            self.input_epoch.load(Ordering::Acquire),
            generation,
        ))
    }
    /// Queue a command in order. A full session queue (slow network, busy
    /// decoder) defers it instead of failing; only a closed session fails.
    fn queue(&self, c: Command) -> Result<(), String> {
        if self.flush() {
            self.close();
            return Err("RDP queued input expired; reconnect".into());
        }
        let c = QueueBudget::reserve(&self.budget, c)?;
        let mut backlog = self.backlog.borrow_mut();
        if backlog.is_empty() {
            match self.tx.try_send(c) {
                Ok(()) => return Ok(()),
                Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => {
                    return Err("RDP disconnected; input not sent".into());
                }
                Err(tokio::sync::mpsc::error::TrySendError::Full(c)) => backlog.push_back(c),
            }
        } else {
            backlog.push_back(c);
        }
        Ok(())
    }
    fn invalidate_input(&self) {
        self.input_epoch.fetch_add(1, Ordering::AcqRel);
        self.backlog
            .borrow_mut()
            .retain(|c| !matches!(c.command, Command::Input(..)));
    }
    /// Absolute oldest-command age; partial/trickle progress never renews it.
    fn flush(&self) -> bool {
        let generation = self.state.lock().unwrap().generation;
        let mut backlog = self.backlog.borrow_mut();
        while let Some(c) = backlog.pop_front() {
            if c.command.stale(generation) {
                continue;
            }
            if c.created.elapsed() > Duration::from_secs(30) {
                backlog.push_front(c);
                return true;
            }
            match self.tx.try_send(c) {
                Ok(()) => (),
                Err(tokio::sync::mpsc::error::TrySendError::Full(c)) => {
                    backlog.push_front(c);
                    break;
                }
                Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => backlog.clear(),
            }
        }
        backlog
            .front()
            .is_some_and(|c| c.created.elapsed() > Duration::from_secs(30))
    }
    fn close(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}
impl Drop for Connection {
    fn drop(&mut self) {
        self.close();
    }
}
/// Keep idle NAT/firewall mappings alive and notice a dead path within about
/// a minute and a half instead of the kernel's two-hour default.
/// `None`: automatic GNOME sign-in can run. `Some(None)`: not a GNOME Remote
/// Login endpoint. `Some(Some(reason))`: the endpoint lacks what it needs.
fn auto_sign_in_missing(p: &crate::profiles::Profile) -> Option<Option<&'static str>> {
    let p = &p.preferences;
    if p.rdp_mode != "gnome-remote-login" {
        Some(None)
    } else if !p.gnome_auto_login {
        Some(Some(
            "Automatic GNOME sign-in is off for this machine · Edit → GNOME User Login → Sign in automatically",
        ))
    } else if p.gnome_username.is_empty() {
        Some(Some(
            "Automatic GNOME sign-in needs the Linux username · Edit → GNOME User Login",
        ))
    } else if p.gnome_secret_reference.is_empty() {
        Some(Some(
            "Automatic GNOME sign-in needs the saved Linux password · Edit → GNOME User Login → Set password",
        ))
    } else {
        None
    }
}
fn keepalive(tcp: &tokio::net::TcpStream) {
    use std::os::fd::AsRawFd;
    let fd = tcp.as_raw_fd();
    let set = |level, name, value: libc::c_int| unsafe {
        libc::setsockopt(
            fd,
            level,
            name,
            (&value as *const libc::c_int).cast(),
            std::mem::size_of::<libc::c_int>() as libc::socklen_t,
        )
    };
    set(libc::SOL_SOCKET, libc::SO_KEEPALIVE, 1);
    set(libc::IPPROTO_TCP, libc::TCP_KEEPIDLE, 30);
    set(libc::IPPROTO_TCP, libc::TCP_KEEPINTVL, 10);
    set(libc::IPPROTO_TCP, libc::TCP_KEEPCNT, 6);
}
async fn run(
    mut p: Profile,
    password: String,
    mut input: tokio::sync::mpsc::Receiver<QueuedCommand>,
    halt: Arc<AtomicBool>,
    state: Arc<Mutex<State>>,
) -> Result<(), String> {
    // Setup and backoff share one foreground budget. Afterwards keep trying
    // quietly, so an endpoint returning online needs no new profile/tab.
    let mut failures = 0u32;
    let mut recovery_began = std::time::Instant::now();
    let mut setup_limit = crate::rdp_retry::SETUP_LIMIT;
    let mut handovers = 0u32;
    let mut first = true;
    loop {
        {
            let mut st = state.lock().unwrap();
            if let Some(size) = st.desktop_size {
                p.desktop_size = size;
            }
            st.generation += 1;
            st.connection_attempt += 1;
            st.phase = if first { "connecting" } else { "reconnecting" }.into();
            st.frame = None;
            st.frames = 0;
            st.connected_since = None;
        }
        if halt.load(Ordering::Relaxed) {
            return Ok(());
        }
        if !first {
            {
                let mut st = state.lock().unwrap();
                st.frame = None;
                st.display_cap = None;
                st.display_channel_opened = false;
                st.display_capability_messages = 0;
                st.reactivating = true;
                st.bridge.reset();
                // A retried setup failure must not linger as this
                // connection's final error once it succeeds.
                st.protocol_error = None;
            }
            while input.try_recv().is_ok() {}
            let delay = if failures > 0 {
                let elapsed = recovery_began.elapsed();
                let plan = crate::rdp_retry::plan(elapsed, failures);
                setup_limit = plan.setup;
                let mut st = state.lock().unwrap();
                st.reconnect_attempt = failures;
                st.reconnect_elapsed = elapsed;
                st.reconnect_background = plan.background;
                st.status = Some(if plan.background {
                    "Connection unavailable · retrying saved connection in 15 seconds · Reconnect to try now".into()
                } else {
                    format!("Reconnecting · attempt {failures} · Reconnect to try now")
                });
                plan.delay
            } else {
                setup_limit = crate::rdp_retry::SETUP_LIMIT;
                state.lock().unwrap().status = Some("Reconnecting · GNOME session handover".into());
                Duration::from_millis(500)
            };
            let until = tokio::time::Instant::now() + delay;
            while tokio::time::Instant::now() < until {
                if halt.load(Ordering::Relaxed) {
                    return Ok(());
                }
                tokio::time::sleep_until(
                    until.min(tokio::time::Instant::now() + Duration::from_millis(50)),
                )
                .await;
            }
        }
        first = false;
        let result = run_session(
            p.next_attempt(),
            password.clone(),
            &mut input,
            halt.clone(),
            state.clone(),
            setup_limit,
        )
        .await;
        let stable = state
            .lock()
            .unwrap()
            .connected_since
            .is_some_and(|at| at.elapsed() >= Duration::from_secs(30));
        if stable {
            failures = 0;
            handovers = 0;
            recovery_began = std::time::Instant::now();
        }
        if let Some(plan) = state.lock().unwrap().handover.take() {
            handovers += 1;
            if handovers > 4 {
                return Err("GNOME handover limit reached".into());
            }
            p.handover = Some(plan);
            failures = 0;
            continue;
        }
        // Transport-level failures (including a connection that closes or
        // stalls during setup) are retried; authentication, certificate and
        // server-initiated disconnects are not.
        let retry = state.lock().unwrap().authentication_error.is_none()
            && result.as_ref().err().is_some_and(|e| {
                matches!(
                    e.as_str(),
                    "RDP TCP connection failed"
                        | "RDP stream disconnected"
                        | "RDP response write timeout"
                        | "RDP response write failed"
                        | "RDP reactivation read failed"
                        | "RDP reactivation write failed"
                        | "RDP reactivation timed out"
                        | "RDP security negotiation failed"
                        | "RDP protocol/session setup failed"
                ) || e.starts_with("RDP connection timed out")
            });
        // GNOME aborts a handover the client did not complete within about
        // half a minute; after a second failure, start again from the
        // service login (automatic sign-in then runs on the new greeter).
        if retry && p.handover.is_some() && failures >= 1 {
            p.handover = None;
        }
        if !p.reconnect || !retry {
            return result;
        }
        failures = failures.saturating_add(1);
    }
}
fn encode_resize(
    active: &mut ironrdp::session::ActiveStage,
    w: u16,
    h: u16,
    scale: u32,
) -> Option<ironrdp::session::SessionResult<Vec<u8>>> {
    let dvc = active.get_dvc::<DisplayControlAdapter>()?;
    let id = dvc.channel_id()?;
    let client = &dvc
        .channel_processor_downcast_ref::<DisplayControlAdapter>()?
        .0;
    let messages =
        match client.encode_single_primary_monitor(id, w.into(), h.into(), Some(scale), None) {
            Ok(m) => m,
            Err(_) => return None,
        };
    Some(
        active.process_svc_processor_messages(ironrdp::svc::SvcProcessorMessages::<
            ironrdp::dvc::DrdynvcClient,
        >::new(messages)),
    )
}
fn connector_config(username: String, password: String) -> connector::Config {
    connector::Config {
        support_dyn_vc_gfx_protocol: false,
        enable_rdstls: false,
        redirected_session_id: None,
        credentials: connector::Credentials::UsernamePassword { username, password },
        domain: None,
        enable_tls: false,
        enable_credssp: true,
        keyboard_type: KeyboardType::IbmEnhanced,
        keyboard_subtype: 0,
        keyboard_layout: 0x0409,
        keyboard_functional_keys_count: 12,
        ime_file_name: String::new(),
        dig_product_id: String::new(),
        desktop_size: connector::DesktopSize {
            width: 1280,
            height: 720,
        },
        bitmap: None,
        client_build: 0,
        client_name: "RUWAC".into(),
        client_dir: String::new(),
        platform: MajorPlatformType::UNIX,
        enable_server_pointer: true,
        request_data: Some(ironrdp::pdu::nego::NegoRequestData::cookie("RUWAC".into())),
        autologon: false,
        enable_audio_playback: false,
        compression_type: None,
        pointer_software_rendering: true,
        multitransport_flags: None,
        performance_flags: PerformanceFlags::default(),
        // MS-RDPBCGR requires 100..500; zero also makes deviceScaleFactor
        // invalid and causes the server to ignore the pair.
        desktop_scale_factor: 100,
        hardware_id: None,
        license_cache: None,
        timezone_info: TimezoneInfo::default(),
        alternate_shell: String::new(),
        work_dir: String::new(),
    }
}
fn tls_builder(trust: &Trust) -> Result<native_tls::TlsConnectorBuilder, String> {
    let mut tls = native_tls::TlsConnector::builder();
    match trust {
        Trust::System => {}
        Trust::CaFile(path) => {
            let pem = crate::credentials::bounded_file(std::path::Path::new(path), 1024 * 1024)?;
            tls.add_root_certificate(
                native_tls::Certificate::from_pem(&pem).map_err(|_| "CA certificate invalid")?,
            );
        }
        Trust::Leaf(_) => {
            // This is provisional encrypted TLS, not authorization. An exact
            // endpoint-bound leaf match and validity check precede CredSSP.
            tls.danger_accept_invalid_certs(true)
                .danger_accept_invalid_hostnames(true);
        }
    }
    Ok(tls)
}
async fn inspect_certificate(p: &Profile, previous: &Trust) -> Result<Review, String> {
    let tcp = tokio::net::TcpStream::connect((p.host.as_str(), p.port))
        .await
        .map_err(|_| "Certificate inspection TCP connection failed")?;
    tcp.set_nodelay(true)
        .map_err(|_| "Certificate inspection socket configuration failed")?;
    let addr = tcp
        .local_addr()
        .map_err(|_| "Certificate inspection address unavailable")?;
    // Only X.224 security negotiation and TLS run. Empty credentials, no
    // connect_finalize, no static channels, no authentication or framebuffer.
    let mut connector =
        connector::ClientConnector::new(connector_config(String::new(), String::new()), addr);
    let mut framed = ironrdp_tokio::TokioFramed::new(tcp);
    ironrdp_tokio::connect_begin(&mut framed, &mut connector)
        .await
        .map_err(|_| "Certificate inspection security negotiation failed")?;
    let (stream, remaining) = framed.into_inner();
    if !remaining.is_empty() {
        return Err("Unexpected certificate inspection pre-TLS payload".into());
    }
    let mut builder = native_tls::TlsConnector::builder();
    builder
        .danger_accept_invalid_certs(true)
        .danger_accept_invalid_hostnames(true);
    let tls = tokio_native_tls::TlsConnector::from(
        builder
            .build()
            .map_err(|_| "Certificate inspection TLS unavailable")?,
    );
    let stream = tls
        .connect(&p.host, stream)
        .await
        .map_err(|_| "Certificate inspection TLS failed")?;
    let der = stream
        .get_ref()
        .peer_certificate()
        .map_err(|_| "Certificate inspection peer unavailable")?
        .ok_or("Certificate inspection peer missing")?
        .to_der()
        .map_err(|_| "Certificate inspection DER invalid")?;
    Review::from_der(&der, &p.host, p.port, previous)
}
async fn run_session(
    p: Profile,
    password: String,
    input: &mut tokio::sync::mpsc::Receiver<QueuedCommand>,
    halt: Arc<AtomicBool>,
    state: Arc<Mutex<State>>,
    setup_limit: Duration,
) -> Result<(), String> {
    let gfx: crate::gfx::Shared = Arc::new(Mutex::new(crate::gfx::Pending::default()));
    let session = async {
        let tcp = tokio::net::TcpStream::connect((p.host.as_str(), p.port))
            .await
            .map_err(|_| "RDP TCP connection failed".to_string())?;
        // Interactive RDP and CredSSP records must not wait for TCP packet coalescing.
        tcp.set_nodelay(true)
            .map_err(|_| "RDP socket configuration failed".to_string())?;
        keepalive(&tcp);
        let addr = tcp
            .local_addr()
            .map_err(|_| "RDP socket address unavailable".to_string())?;
        let mut config = connector_config(
            p.user.clone(),
            if p.handover.is_some() {
                String::new()
            } else {
                password
            },
        );
        config.desktop_size = connector::DesktopSize {
            width: p.desktop_size.0,
            height: p.desktop_size.1,
        };
        if let Some(plan) = p.handover.as_ref() {
            config.enable_rdstls = true;
            config.enable_credssp = false;
            config.redirected_session_id = Some(plan.session);
            config.request_data = Some(ironrdp::pdu::nego::NegoRequestData::routing_token(
                plan.routing_token()?,
            ));
            config.credentials = connector::Credentials::UsernamePassword {
                username: plan.username()?,
                password: String::new(),
            };
        }
        if p.graphics {
            config.support_dyn_vc_gfx_protocol = true;
            config.bitmap = Some(connector::BitmapConfig {
                color_depth: 32,
                lossy_compression: false,
                codecs: Default::default(),
            });
        }
        let bridge = state.lock().unwrap().bridge.clone();
        let cap_state = state.clone();
        let mut dynamic =
            ironrdp::dvc::DrdynvcClient::new().with_dynamic_channel(DisplayControlAdapter(
                ironrdp::displaycontrol::client::DisplayControlClient::new(move |caps| {
                    cap_state.lock().unwrap().display_cap = Some(caps.max_monitor_area());
                    Ok(Vec::new())
                }),
                Some(state.clone()),
            ));
        if p.graphics {
            dynamic = dynamic.with_dynamic_channel(crate::gfx::Channel::new(gfx.clone()));
        }
        let mut connector =
            connector::ClientConnector::new(config, addr).with_static_channel(dynamic);
        // Global desktop isolation excludes CLIPRDR itself, not just its data.
        // Live per-view policies still operate over enabled connections.
        if p.clipboard {
            connector =
                connector.with_static_channel(CliprdrClient::new(Box::new(Backend::new(bridge))));
        }
        let mut framed = ironrdp_tokio::TokioFramed::new(tcp);
        state.lock().unwrap().status = Some("RDP security negotiation…".into());
        let upgrade = ironrdp_tokio::connect_begin(&mut framed, &mut connector)
            .await
            .map_err(|_| "RDP security negotiation failed".to_string())?;
        let (stream, leftover) = framed.into_inner();
        if !leftover.is_empty() {
            return Err("Unexpected pre-TLS payload".into());
        }
        let trust = Trust::parse(p.ca.as_deref(), &p.host, p.port)?;
        let tls = tokio_native_tls::TlsConnector::from(
            tls_builder(&trust)?
                .build()
                .map_err(|_| "TLS initialization failed".to_string())?,
        );
        let mut tls = match tls.connect(&p.host, stream).await {
            Ok(tls) => tls,
            Err(_) => {
                if !trust.is_pinned() {
                    if let Ok(review) = inspect_certificate(&p, &trust).await {
                        state.lock().unwrap().certificate = Some(review);
                        return Err("RDP certificate needs approval; review its fingerprint before connecting".into());
                    }
                }
                return Err("TLS certificate/hostname validation failed".into());
            }
        };
        let der = tls
            .get_ref()
            .peer_certificate()
            .map_err(|_| "Peer certificate unavailable".to_string())?
            .ok_or("Peer certificate missing")?
            .to_der()
            .map_err(|_| "Peer certificate invalid".to_string())?;
        if let Err(error) = trust.verify_leaf(&der, &p.host, p.port) {
            if let Ok(review) = Review::from_der(&der, &p.host, p.port, &trust) {
                state.lock().unwrap().certificate = Some(review);
            }
            return Err(error);
        }

        if p.handover.is_some() {
            let fingerprint = Review::from_der(&der, &p.host, p.port, &trust)?.fingerprint();
            if state.lock().unwrap().verified_tls_sha256.as_deref() != Some(fingerprint.as_str()) {
                return Err("GNOME handover changed the authenticated endpoint certificate".into());
            }
        }
        if let Some(plan) = p.handover.as_ref() {
            state.lock().unwrap().connection_stage = "GNOME RDSTLS handover".into();
            crate::handover::authenticate(&mut tls, plan).await?;
        }
        let cert = x509_cert::Certificate::from_der(&der)
            .map_err(|_| "Peer certificate decoding failed".to_string())?;
        state.lock().unwrap().verified_tls_sha256 =
            Some(Review::from_der(&der, &p.host, p.port, &trust)?.fingerprint());
        let key = cert
            .tbs_certificate
            .subject_public_key_info
            .subject_public_key
            .as_bytes()
            .ok_or("Peer key unavailable")?
            .to_vec();
        let upgraded = ironrdp_tokio::mark_as_upgraded(upgrade, &mut connector);
        let mut framed = ironrdp_tokio::TokioFramed::new(ProgressTls {
            stream: tls,
            state: state.clone(),
        });
        let mut network = ironrdp_tokio::reqwest::ReqwestNetworkClient::new();
        state.lock().unwrap().status = Some("RDP CredSSP / session activation…".into());
        let result = ironrdp_tokio::connect_finalize(
            upgraded,
            connector,
            &mut framed,
            &mut network,
            p.host.clone().into(),
            key,
            None,
        )
        .await
        .map_err(|error| {
            let code = match error.kind() {
                connector::ConnectorErrorKind::Credssp(sspi) => {
                    format!("credssp:{:?}", sspi.error_type)
                }
                connector::ConnectorErrorKind::AccessDenied => "access_denied".into(),
                connector::ConnectorErrorKind::Negotiation(_) => "negotiation".into(),
                connector::ConnectorErrorKind::Encode(_) => "encode".into(),
                connector::ConnectorErrorKind::Decode(_) => "decode".into(),
                _ => "session_protocol".into(),
            };
            if matches!(
                error.kind(),
                connector::ConnectorErrorKind::Credssp(_)
                    | connector::ConnectorErrorKind::AccessDenied
            ) {
                state.lock().unwrap().authentication_error = Some(code);
                if p.graphics { "RDP service authentication failed; verify the service username/password, separate from GNOME User Login".to_string() } else { "RDP authentication/session setup failed".to_string() }
            } else {
                state.lock().unwrap().protocol_error = Some(code);
                "RDP protocol/session setup failed".to_string()
            }
        })?;
        if result.desktop_size.width == 0
            || result.desktop_size.height == 0
            || result.desktop_size.width > 4096
            || result.desktop_size.height > 4096
        {
            return Err("Remote desktop exceeds framebuffer limits".into());
        }
        Ok::<_, String>((result, framed))
    };
    let (result, framed) = tokio::select! {r=tokio::time::timeout(setup_limit,session)=>r.map_err(|_|format!("RDP connection timed out during {}",state.lock().unwrap().connection_stage))??,_=async{while !halt.load(Ordering::Relaxed){tokio::time::sleep(Duration::from_millis(50)).await;}}=>return Ok(())};
    let activation_factory = result.activation_factory.clone();
    let mut image = DecodedImage::new(
        ironrdp::graphics::image_processing::PixelFormat::BgrX32,
        result.desktop_size.width,
        result.desktop_size.height,
    );
    let io_channel = result.io_channel_id;
    let clip_channel = result
        .static_channels
        .get_channel_id_by_type::<CliprdrClient>();
    state.lock().unwrap().clipboard_channel_negotiated = clip_channel.is_some();
    let dvc_channel = result
        .static_channels
        .get_channel_id_by_type::<ironrdp::dvc::DrdynvcClient>();
    let mut display_fragments = DisplayFragmentGuard::default();
    let mut fragments = crate::cliprdr::FragmentGuard::default();
    let mut active = ActiveStageBuilder {
        static_channels: result.static_channels,
        user_channel_id: result.user_channel_id,
        io_channel_id: result.io_channel_id,
        message_channel_id: result.message_channel_id,
        share_id: result.share_id,
        compression_type: result.compression_type,
        enable_server_pointer: result.enable_server_pointer,
        pointer_software_rendering: result.pointer_software_rendering,
    }
    .build();
    state.lock().unwrap().status = Some("Connected".into());
    let (mut reader, mut writer) = ironrdp_tokio::split_tokio_framed(framed);
    // Input queued during authentication belongs to the previous transport (or
    // an unready channel). Discard it before processing this connection's PDUs.
    // Fresh clipboard initialization is driven by the new channel callbacks.
    while input.try_recv().is_ok() {}
    loop {
        if state
            .lock()
            .unwrap()
            .bridge
            .overflow
            .load(Ordering::Relaxed)
        {
            return Err("CLIPRDR queue exceeded safety bounds".into());
        }
        if halt.load(Ordering::Relaxed) {
            // Give queued modifier/button releases a bounded flush before closing the socket.
            let _ = tokio::time::timeout(Duration::from_millis(250), async {
                for _ in 0..128 {
                    match input.try_recv() {
                        Ok(QueuedCommand {
                            command: Command::Release(events),
                            ..
                        }) => {
                            if let Ok(outputs) = active.process_fastpath_input(&mut image, &events)
                            {
                                for output in outputs {
                                    if let ActiveStageOutput::ResponseFrame(bytes) = output {
                                        if writer.write_all(&bytes).await.is_err() {
                                            return;
                                        }
                                    }
                                }
                            }
                        }
                        Ok(_) => (),
                        Err(_) => break,
                    }
                }
            })
            .await;
            return Ok(());
        }
        let mut outputs = tokio::select! {
         pdu=reader.read_pdu()=>{let(action,bytes)=pdu.map_err(|_|"RDP stream disconnected".to_string())?;if action==ironrdp::pdu::Action::X224{if let Ok(ctx)=ironrdp::pdu::mcs::decode_send_data_indication(&bytes){if p.graphics && ctx.channel_id==io_channel {
                if let Some(plan)=crate::handover::parse(ctx.user_data)? {
                    {let mut st=state.lock().unwrap();st.handovers+=1;st.handover=Some(Arc::new(plan));}return Ok(());
                }
            }if Some(ctx.channel_id)==clip_channel{fragments.check(ctx.user_data)?;}if Some(ctx.channel_id)==dvc_channel{display_fragments.check(ctx.user_data)?;}}}active.process(&mut image,action,&bytes).map_err(|e|crate::gfx::decoder_location(&e))?},
         command=input.recv()=>{let Some(queued)=command else{return Ok(());};
            if queued.created.elapsed()>Duration::from_secs(30) { return Err("RDP queued command expired".into()); }
            if queued.command.stale(state.lock().unwrap().generation) { continue; }
            let QueuedCommand { command, _permit, .. }=queued;
            match command {
            Command::Resize(w,h,scale)=>{
                let ready=active.get_dvc::<DisplayControlAdapter>().and_then(|d|d.channel_processor_downcast_ref::<DisplayControlAdapter>()).is_some_and(|d|d.0.ready());
                if image.width()==w&&image.height()==h{state.lock().unwrap().status=Some(format!("Connected · remote {w}×{h}"));vec![]}else if ready && state.lock().unwrap().display_cap.is_some_and(|max|w as u64*h as u64<=max) {match encode_resize(&mut active,w,h,scale){Some(Ok(bytes))=>{{let mut st=state.lock().unwrap();st.display_request=Some((w,h));st.status=Some(format!("Display Control request {w}×{h}; awaiting framebuffer"));}vec![ActiveStageOutput::ResponseFrame(bytes)]},_=>{state.lock().unwrap().status=Some("Display Control rejected; local fit remains available".into());vec![]}}}else{state.lock().unwrap().status=Some("Display Control unsupported or not ready · local fit fallback".into());vec![]}
            },
            Command::Assisted(text, epoch, expected, generation, submit)=>{
                let mut completed=true;
                let mut units=SensitiveUnits(text.0.encode_utf16().collect());
                for chunk in units.0.chunks(32) {
                    if halt.load(Ordering::Acquire) || epoch.load(Ordering::Acquire)!=expected || state.lock().unwrap().generation!=generation || state.lock().unwrap().reactivating { completed=false;break; }
                    let events:Vec<_>=chunk.iter().flat_map(|u|[FastPathInputEvent::UnicodeKeyboardEvent(KeyboardFlags::empty(),*u),FastPathInputEvent::UnicodeKeyboardEvent(KeyboardFlags::RELEASE,*u)]).collect();
                    let outputs=active.process_fastpath_input(&mut image,&events).map_err(|_|"Assisted input encoding failed".to_string())?;
                    for output in outputs { if let ActiveStageOutput::ResponseFrame(bytes)=output { tokio::time::timeout(Duration::from_secs(10),writer.write_all(&bytes)).await.map_err(|_|"Assisted input write timeout")?.map_err(|_|"Assisted input write failed")?; } }
                }
                units.0.fill(0);
                if submit && completed && !halt.load(Ordering::Acquire) && epoch.load(Ordering::Acquire)==expected && state.lock().unwrap().generation==generation && !state.lock().unwrap().reactivating {
                    let events=[FastPathInputEvent::KeyboardEvent(KeyboardFlags::empty(),0x1c),FastPathInputEvent::KeyboardEvent(KeyboardFlags::RELEASE,0x1c)];
                    let outputs=active.process_fastpath_input(&mut image,&events).map_err(|_|"GNOME submit encoding failed".to_string())?;
                    for output in outputs {if let ActiveStageOutput::ResponseFrame(bytes)=output {tokio::time::timeout(Duration::from_secs(10),writer.write_all(&bytes)).await.map_err(|_|"GNOME submit write timeout")?.map_err(|_|"GNOME submit write failed")?;}}
                }
                vec![]
            },
            Command::Input(events,..) | Command::Release(events)=>active.process_fastpath_input(&mut image,&events).map_err(|_|"RDP input encoding failed".to_string())?,
            command=>{let clip=active.get_svc_processor_mut::<CliprdrClient>().ok_or("CLIPRDR not negotiated")?;
            let messages=match command{Command::Copy(f)=>clip.initiate_copy(&f),Command::Request(f)=>clip.initiate_paste(f),Command::Reply(data)=>clip.submit_format_data(match data{Some(b)=>FormatDataResponse::new_data(b),None=>FormatDataResponse::new_error()}),_=>unreachable!()}.map_err(|_|"CLIPRDR command failed".to_string())?;
            let bytes=active.process_svc_processor_messages(messages).map_err(|_|"CLIPRDR encoding failed".to_string())?;vec![ActiveStageOutput::ResponseFrame(bytes)]}
         }},
         _=tokio::time::sleep(Duration::from_millis(50))=>continue,
        };
        let (gfx_reset, gfx_changed) = crate::gfx::apply(&gfx, &mut image)?;
        if gfx_reset {
            let mut st = state.lock().unwrap();
            st.generation += 1;
            st.reactivating = true;
            st.frame = None;
        }
        if gfx_changed {
            outputs.push(ActiveStageOutput::GraphicsUpdate(
                ironrdp::pdu::geometry::InclusiveRectangle {
                    left: 0,
                    top: 0,
                    right: image.width() - 1,
                    bottom: image.height() - 1,
                },
            ));
        }
        for output in outputs {
            match output {
                ActiveStageOutput::ResponseFrame(bytes) => {
                    tokio::time::timeout(Duration::from_secs(15), writer.write_all(&bytes))
                        .await
                        .map_err(|_| "RDP response write timeout".to_string())?
                        .map_err(|_| "RDP response write failed".to_string())?
                }
                ActiveStageOutput::GraphicsUpdate(rect) => {
                    let mut st = state.lock().unwrap();
                    st.reactivating = false;
                    st.desktop_size = Some((image.width(), image.height()));
                    let dimensions = (image.width(), image.height());
                    if st.decoded_sizes.back() != Some(&dimensions) {
                        st.decoded_sizes.push_back(dimensions);
                        if st.decoded_sizes.len() > 64 {
                            st.decoded_sizes.pop_front();
                        }
                    }
                    st.phase = "connected".into();
                    st.connected_since
                        .get_or_insert_with(std::time::Instant::now);
                    st.reconnect_background = false;
                    if st.display_request == Some((image.width(), image.height())) {
                        st.display_request = None;
                        st.status = Some(format!(
                            "Connected · remote {}×{}",
                            image.width(),
                            image.height()
                        ));
                    }
                    if rect.right + 1 == image.width() && rect.bottom + 1 == image.height() {
                        st.frames += 1;
                        st.cadence.push_back(std::time::Instant::now());
                        if st.cadence.len() > 128 {
                            st.cadence.pop_front();
                        }
                    }
                    st.frame = Some(Frame {
                        generation: st.generation,
                        width: image.width(),
                        height: image.height(),
                        pixels: image.data().to_vec(),
                    })
                }
                ActiveStageOutput::Terminate(reason) => {
                    // The pinned processor derives this description solely
                    // from numeric MCS/ErrorInfo codes, never wire text.
                    state.lock().unwrap().protocol_error =
                        Some(format!("RDP disconnect: {}", reason.description()));
                    return Ok(());
                }
                ActiveStageOutput::DeactivateAll => {
                    {
                        let mut st = state.lock().unwrap();
                        st.frame = None;
                        st.reactivating = true;
                        st.phase = "display_change".into();
                        st.generation += 1;
                        st.bridge.reset();
                        st.status = Some("Server reactivating display…".into());
                    }
                    while input.try_recv().is_ok() {}
                    let mut sequence = activation_factory.create();
                    let mut buf = ironrdp_core::WriteBuf::new();
                    let reactivation = async {
                        use connector::State;
                        while !sequence.connection_activation_state().is_terminal() {
                            let written = ironrdp_tokio::single_sequence_step_read(
                                &mut reader,
                                &mut sequence,
                                &mut buf,
                            )
                            .await
                            .map_err(|_| "RDP reactivation read failed")?;
                            if written.size().unwrap_or(0) > 0 {
                                writer
                                    .write_all(buf.filled())
                                    .await
                                    .map_err(|_| "RDP reactivation write failed")?;
                            }
                        }
                        Ok::<_, String>(())
                    };
                    tokio::select! {r=tokio::time::timeout(Duration::from_secs(20),reactivation)=>r.map_err(|_|"RDP reactivation timed out")??,_=async{while !halt.load(Ordering::Relaxed){tokio::time::sleep(Duration::from_millis(50)).await;}}=>return Ok(())};
                    if let connector::connection_activation::ConnectionActivationState::Finalized{desktop_size,share_id,enable_server_pointer,pointer_software_rendering}=sequence.connection_activation_state(){
                        if desktop_size.width==0||desktop_size.height==0||desktop_size.width>4096||desktop_size.height>4096{return Err("Reactivated framebuffer exceeds limits".into());}
                        active.set_share_id(share_id);active.set_enable_server_pointer(enable_server_pointer);
                        active.set_fastpath_processor(ironrdp::session::fast_path::ProcessorBuilder{io_channel_id:activation_factory.io_channel_id(),user_channel_id:activation_factory.user_channel_id(),share_id,enable_server_pointer,pointer_software_rendering,bulk_decompressor:None}.build());
                        image=DecodedImage::new(ironrdp::graphics::image_processing::PixelFormat::BgrX32,desktop_size.width,desktop_size.height);
                        state.lock().unwrap().status=Some(format!("Connected · reactivated {}×{}",desktop_size.width,desktop_size.height));
                    } else {return Err("RDP reactivation did not finalize".into());}
                }
                _ => (),
            }
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Scale {
    Fit,
    Native,
    Stretch,
    Dynamic,
}
pub fn transform(
    widget: (f64, f64),
    remote: (u16, u16),
    mode: Scale,
    x: f64,
    y: f64,
) -> Option<(u16, u16)> {
    let (w, h) = widget;
    let (rw, rh) = (remote.0 as f64, remote.1 as f64);
    if w <= 0.0 || h <= 0.0 || rw == 0.0 || rh == 0.0 {
        return None;
    }
    let (sx, sy) = match mode {
        Scale::Fit | Scale::Dynamic => {
            let s = (w / rw).min(h / rh);
            (s, s)
        }
        Scale::Native => (1.0, 1.0),
        Scale::Stretch => (w / rw, h / rh),
    };
    let (ox, oy) = ((w - rw * sx) / 2.0, (h - rh * sy) / 2.0);
    let (px, py) = ((x - ox) / sx, (y - oy) / sy);
    if px < 0.0 || py < 0.0 || px >= rw || py >= rh {
        return None;
    }
    Some((px as u16, py as u16))
}
fn viewport_geometry(
    canvas: (f64, f64),
    viewport: (f64, f64),
    remote: (u16, u16),
    mode: Scale,
    zoom: f64,
    scale: f64,
) -> (f64, f64, f64, f64) {
    let (rw, rh) = (remote.0.max(1) as f64, remote.1.max(1) as f64);
    let (vw, vh) = viewport;
    let z = if mode == Scale::Native {
        zoom / scale.max(0.25)
    } else {
        zoom
    };
    let (sx, sy) = match mode {
        Scale::Native => (z, z),
        Scale::Stretch => (vw / rw * z, vh / rh * z),
        _ => ((vw / rw).min(vh / rh) * z, (vw / rw).min(vh / rh) * z),
    };
    (
        sx,
        sy,
        ((canvas.0 - rw * sx) / 2.0).max(0.0),
        ((canvas.1 - rh * sy) / 2.0).max(0.0),
    )
}
/// Ignore allocation rounding drift without accumulating one-pixel resize requests.
fn stable_resize(raw: (u16, u16), previous: (u16, u16)) -> (u16, u16) {
    let axis = |next: u16, old: u16| {
        if old > 0 && next.abs_diff(old) <= 4 {
            old
        } else {
            next
        }
    };
    (axis(raw.0, previous.0), axis(raw.1, previous.1))
}
#[derive(Default)]
struct InitialViewport {
    sample: Option<((u16, u16), std::time::Instant)>,
}
impl InitialViewport {
    fn observe(&mut self, size: Option<(u16, u16)>, now: std::time::Instant) -> Option<(u16, u16)> {
        let Some(size) = size else {
            self.sample = None;
            return None;
        };
        match self.sample {
            Some((old, since)) if stable_resize(size, old) == old => {
                (now.duration_since(since) >= Duration::from_millis(100)).then_some(old)
            }
            _ => {
                self.sample = Some((size, now));
                None
            }
        }
    }
}
struct PendingConnect {
    profile: Profile,
    began: std::time::Instant,
    viewport: InitialViewport,
}
struct AutoGdm {
    armed: bool,
    sequence: crate::gdm::Sequence,
    last_observation: Option<crate::gdm::Kind>,
    began: std::time::Instant,
    scan_at: std::time::Instant,
    tick_at: std::time::Instant,
    binding: Option<(String, u64, u64)>,
    pending: Option<std::sync::mpsc::Receiver<Result<crate::gdm::Observation, String>>>,
    scan_binding: Option<(String, u64, u64)>,
    image: Option<(u16, u16, u64, Vec<u8>)>,
    fetching: bool,
    submitted_handovers: u32,
    submitted_attempt: u64,
    password_queued: bool,
    /// Shown on the covered canvas while a guard holds the sign-in back.
    waiting: Option<&'static str>,
    /// Shown over the remote screen after a stop, until the user acts.
    notice: Option<String>,
    /// Retry stays offered while the greeter of this handover is current.
    stopped_at: Option<u32>,
    /// A verified completed automatic login may be repeated only when this
    /// same transport receives a later authenticated GNOME service handover.
    completed: Option<(String, u64, u32)>,
}
impl Default for AutoGdm {
    fn default() -> Self {
        Self {
            armed: false,
            sequence: Default::default(),
            last_observation: None,
            began: std::time::Instant::now(),
            scan_at: std::time::Instant::now(),
            tick_at: std::time::Instant::now(),
            binding: None,
            pending: None,
            scan_binding: None,
            image: None,
            fetching: false,
            submitted_handovers: 0,
            submitted_attempt: 0,
            password_queued: false,
            waiting: None,
            notice: None,
            stopped_at: None,
            completed: None,
        }
    }
}
impl AutoGdm {
    fn retry_on_greeter(&self, handovers: u32, verified_live: bool) -> bool {
        !self.armed
            && self.stopped_at.is_some()
            && verified_live
            && self.stopped_at.is_some_and(|at| handovers >= at)
    }
    fn reconnects_to_greeter(&self, transport: &str, generation: u64, handovers: u32) -> bool {
        !self.armed
            && self
                .completed
                .as_ref()
                .is_some_and(|(t, g, h)| t == transport && generation > *g && handovers > *h)
    }
    fn authenticated_desktop_handover(&self, handovers: u32, attempt: u64) -> bool {
        self.sequence.step == crate::gdm::Step::Submitted
            && self.password_queued
            && !self.fetching
            && handovers == self.submitted_handovers.saturating_add(1)
            && attempt == self.submitted_attempt.saturating_add(1)
    }
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum PrimaryStage {
    Preparing,
    Committing,
    Confirming,
    LegacyOffer,
}
#[derive(Clone)]
struct PrimaryClick {
    id: u64,
    x: f64,
    y: f64,
    released: bool,
    started: std::time::Instant,
    transport: String,
    generation: u64,
    epoch: u64,
    geometry: (f64, f64, f64, f64),
    source: gdk::ContentProvider,
    stage: PrimaryStage,
}
#[derive(Clone, PartialEq, Eq)]
struct ClipboardRequest {
    id: u64,
    kind: Format,
    epoch: u64,
    generation: u64,
    created: std::time::Instant,
    transport: String,
    serial: u64,
    transport_generation: u64,
    provider: Option<gdk::ContentProvider>,
}
/// Wire FormatListResponse has no ID. Only one locally stamped offer may
/// await it; newer offers wait until that response has been consumed.
struct LocalOffer {
    formats: Vec<ClipboardFormat>,
    epoch: u64,
    serial: u64,
    source: Option<gdk::ContentProvider>,
    transport: String,
    generation: u64,
    intent: bool,
    created: std::time::Instant,
}
pub struct RdpView {
    pub root: gtk::Box,
    area: gtk::DrawingArea,
    pub status: gtk::Label,
    pub toolbar: gtk::Box,
    disconnect_card: gtk::Box,
    disconnect_message: gtk::Label,
    connection: RefCell<Option<Connection>>,
    surface: RefCell<Option<gtk::cairo::ImageSurface>>,
    size: Cell<(u16, u16)>,
    last_size: Cell<Option<(u16, u16)>>,
    pending_connect: RefCell<Option<PendingConnect>>,
    surface_generation: Cell<u64>,
    rendered_generation: Cell<u64>,
    drawn: Cell<u64>,
    last_pixel: Cell<[u8; 4]>,
    reconnect: Cell<bool>,
    fullscreen: Cell<bool>,
    display_info: gtk::Label,
    pan: Cell<bool>,
    preferences: RefCell<Option<crate::profiles::PreferenceSink>>,
    trust_sink: RefCell<Option<crate::profiles::TrustSink>>,
    certificate: RefCell<Option<Review>>,
    certificate_prompt_pending: Cell<bool>,
    review_certificate: gtk::Button,
    transient_pin: RefCell<Option<String>>,
    preferences_due: Cell<Option<std::time::Instant>>,
    mode: Cell<Scale>,
    scroll: gtk::ScrolledWindow,
    zoom: Cell<f64>,
    desired: Cell<(u16, u16)>,
    resize_at: Cell<Option<std::time::Instant>>,
    requested: Cell<(u16, u16)>,
    pending_super: RefCell<crate::remote_keys::DeferredSuper>,
    clipboard_refresh_pending: Cell<bool>,
    held: RefCell<HashSet<(u8, bool)>>,
    buttons: RefCell<HashSet<u32>>,
    pointer_position: Cell<(u16, u16)>,
    native_middle_presses: Cell<u64>,
    pointer_geometry: Cell<Option<(f64, f64, f64, f64)>>,
    broker: crate::clipboard::Broker,
    id: String,
    policy: Cell<ClipPolicy>,
    serial: Cell<u64>,
    bridge: RefCell<Bridge>,
    advertised: Cell<u64>,
    received_generation: Cell<u64>,
    offers_seen: Cell<u64>,
    remote: Cell<Option<(ClipboardFormatId, Format, u64, u64)>>,
    pending: RefCell<Option<ClipboardRequest>>,
    request_serial: Cell<u64>,
    remote_formats: RefCell<Vec<(ClipboardFormatId, Format)>>,
    lazy_reply: RefCell<Option<crate::remote_content::LazyReply>>,
    replying: Cell<bool>,
    decoding: Cell<bool>,
    offers: RefCell<crate::clip_offers::Offers<LocalOffer>>,
    consumed_keys: RefCell<std::collections::HashSet<u32>>,
    provider: RefCell<Option<gdk::ContentProvider>>,
    entries: [gtk::Entry; 4],
    assist_button: gtk::Button,
    assist_epoch: Arc<AtomicU64>,
    auto_gdm: RefCell<AutoGdm>,
    selected: RefCell<Option<Rc<dyn Fn() -> bool>>>,
    profile_metadata: RefCell<Option<crate::profiles::Profile>>,
    primary: RefCell<Option<crate::primary::Bridge>>,
    primary_attempted: Cell<bool>,
    primary_transport: RefCell<Option<String>>,
    primary_checked: Cell<Option<std::time::Instant>>,
    primary_status: gtk::Label,
    primary_authorize: gtk::Button,
    primary_click: RefCell<Option<PrimaryClick>>,
    primary_error: RefCell<Option<String>>,
    suppress_middle_release: Cell<bool>,
    selection_anchor: Cell<Option<(f64, f64)>>,
}
fn quick_action(toolbar: &gtk::Box, name: &str, icon: &str, label: &str, target: &gtk::Button) {
    let button = gtk::Button::from_icon_name(icon);
    button.set_widget_name(name);
    button.set_tooltip_text(Some(label));
    button.update_property(&[gtk::accessible::Property::Label(label)]);
    button.set_visible(target.property::<bool>("visible"));
    let shown = button.downgrade();
    target.connect_visible_notify(move |target| {
        if let Some(button) = shown.upgrade() {
            button.set_visible(target.property::<bool>("visible"));
        }
    });
    let target = target.clone();
    button.connect_clicked(move |_| target.emit_clicked());
    toolbar.append(&button);
}
impl RdpView {
    pub fn new(broker: crate::clipboard::Broker) -> Rc<Self> {
        let root = gtk::Box::new(gtk::Orientation::Vertical, 4);
        root.set_tooltip_text(None);
        let controls = gtk::Grid::builder()
            .column_spacing(4)
            .row_spacing(4)
            .build();
        let host = gtk::Entry::builder()
            .placeholder_text("Authorized host:port")
            .build();
        let user = gtk::Entry::builder().placeholder_text("Username").build();
        let secret = gtk::Entry::builder()
            .placeholder_text("Secret Service profile reference")
            .build();
        let ca = gtk::Entry::builder()
            .placeholder_text("Optional CA PEM path")
            .build();
        let connect = gtk::Button::with_label("Connect / reconnect");
        let disconnect = gtk::Button::with_label("Disconnect");
        for (i, e) in [&host, &user, &secret, &ca].iter().enumerate() {
            e.set_hexpand(true);
            controls.attach(*e, (i % 2) as i32, (i / 2) as i32, 1, 1);
        }
        controls.attach(&connect, 0, 2, 1, 1);
        controls.attach(&disconnect, 1, 2, 1, 1);
        let popup = gtk::Popover::new();
        controls.set_margin_top(8);
        controls.set_margin_bottom(8);
        controls.set_margin_start(8);
        controls.set_margin_end(8);
        popup.set_child(Some(&controls));
        let menu = gtk::MenuButton::builder()
            .label("RDP controls")
            .popover(&popup)
            .halign(gtk::Align::Start)
            .build();
        let toolbar = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        toolbar.set_widget_name("rdp-toolbar");
        toolbar.add_css_class("toolbar");
        let title = gtk::Label::new(Some("Remote desktop"));
        title.set_xalign(0.0);
        title.set_hexpand(true);
        title.add_css_class("heading");
        title.set_visible(false);
        toolbar.append(&gtk::Image::from_icon_name("computer-symbolic"));
        toolbar.append(&title);
        toolbar.append(&menu);

        quick_action(
            &toolbar,
            "reconnect",
            "view-refresh-symbolic",
            "Connect / reconnect",
            &connect,
        );
        quick_action(
            &toolbar,
            "disconnect",
            "network-offline-symbolic",
            "Disconnect",
            &disconnect,
        );
        let status = gtk::Label::new(Some(
            "Choose a machine from the sidebar or open RDP controls to connect",
        ));
        status.set_xalign(0.0);
        status.set_ellipsize(gtk::pango::EllipsizeMode::End);
        status.add_css_class("dim-label");
        status.set_max_width_chars(80);
        let viewport_overlay = gtk::Overlay::new();
        viewport_overlay.set_vexpand(true);
        root.append(&viewport_overlay);
        let review_certificate = gtk::Button::with_label("Review RDP certificate…");
        review_certificate.set_widget_name("rdp-review-certificate");
        review_certificate.set_halign(gtk::Align::Start);
        review_certificate.set_visible(false);
        toolbar.append(&review_certificate);
        let area = gtk::DrawingArea::builder()
            .hexpand(true)
            .vexpand(true)
            .focusable(true)
            .build();
        // The entire framebuffer is one focusable widget. GTK's automatic
        // scroll-to-focus moves a zoomed desktop when a tab regains focus.
        // Keep the user's pan instead of scrolling the whole canvas into view.
        let viewport = gtk::Viewport::builder()
            .scroll_to_focus(false)
            .child(&area)
            .build();
        let scroll = gtk::ScrolledWindow::builder()
            .hexpand(true)
            .vexpand(true)
            .child(&viewport)
            .build();
        viewport_overlay.set_child(Some(&scroll));
        let disconnect_card = gtk::Box::new(gtk::Orientation::Vertical, 12);
        disconnect_card.set_halign(gtk::Align::Center);
        disconnect_card.set_valign(gtk::Align::Center);
        disconnect_card.set_widget_name("rdp-connection-card");
        disconnect_card.add_css_class("card");
        disconnect_card.set_margin_top(20);
        disconnect_card.set_margin_bottom(20);
        disconnect_card.set_margin_start(20);
        disconnect_card.set_margin_end(20);
        let disconnect_message = gtk::Label::new(Some("Disconnected"));
        disconnect_message.set_wrap(true);
        disconnect_message.set_max_width_chars(52);
        disconnect_card.append(&disconnect_message);
        let reconnect_center = gtk::Button::with_label("Reconnect");
        reconnect_center.set_widget_name("rdp-center-reconnect");
        reconnect_center.add_css_class("suggested-action");
        disconnect_card.append(&reconnect_center);
        disconnect_card.set_visible(false);
        viewport_overlay.add_overlay(&disconnect_card);

        let display_info = gtk::Label::new(None);
        display_info.set_wrap(true);
        display_info.set_visible(false);
        let summary = status.clone();
        let detail = display_info.clone();
        display_info.connect_label_notify(move |_| summary.set_tooltip_text(Some(&detail.text())));
        let assist = gtk::Button::with_label("Assisted GNOME Sign-in");
        assist.set_widget_name("assisted-gnome-sign-in");
        controls.attach(&assist, 0, 4, 2, 1);
        assist.set_visible(false);
        quick_action(
            &toolbar,
            "gnome-sign-in",
            "dialog-password-symbolic",
            "Assisted GNOME Sign-in",
            &assist,
        );
        let primary_status = gtk::Label::new(None);
        primary_status.set_wrap(true);
        primary_status.set_visible(false);
        root.append(&primary_status);
        let primary_authorize = gtk::Button::with_label("Authorize selection connection…");
        primary_authorize.set_widget_name("primary-authorize");
        primary_authorize.set_visible(false);
        let v = Rc::new(Self {
            root: root.clone(),
            area,
            status,
            toolbar: toolbar.clone(),
            disconnect_card,
            disconnect_message,
            connection: RefCell::new(None),
            surface: RefCell::new(None),
            size: Cell::new((0, 0)),
            last_size: Cell::new(None),
            pending_connect: RefCell::new(None),
            surface_generation: Cell::new(0),
            rendered_generation: Cell::new(0),
            drawn: Cell::new(0),
            last_pixel: Cell::new([0; 4]),
            reconnect: Cell::new(true),
            fullscreen: Cell::new(false),
            display_info,
            pan: Cell::new(false),
            preferences: RefCell::new(None),
            trust_sink: RefCell::new(None),
            certificate: RefCell::new(None),
            certificate_prompt_pending: Cell::new(false),
            review_certificate: review_certificate.clone(),
            transient_pin: RefCell::new(None),
            preferences_due: Cell::new(None),
            mode: Cell::new(Scale::Dynamic),
            scroll,
            zoom: Cell::new(1.0),
            desired: Cell::new((0, 0)),
            resize_at: Cell::new(None),
            requested: Cell::new((0, 0)),
            pending_super: RefCell::new(Default::default()),
            clipboard_refresh_pending: Cell::new(false),
            held: RefCell::new(HashSet::new()),
            buttons: RefCell::new(HashSet::new()),
            pointer_position: Cell::new((0, 0)),
            native_middle_presses: Cell::new(0),
            pointer_geometry: Cell::new(None),
            broker,
            id: ruwac_protocol::id("rdp"),
            policy: Cell::new(ClipPolicy::Bidirectional),
            serial: Cell::new(0),
            bridge: RefCell::new(Bridge::default()),
            advertised: Cell::new(0),
            received_generation: Cell::new(0),
            offers_seen: Cell::new(0),
            remote: Cell::new(None),
            pending: RefCell::new(None),
            request_serial: Cell::new(0),
            remote_formats: RefCell::new(Vec::new()),
            lazy_reply: RefCell::new(None),
            replying: Cell::new(false),
            decoding: Cell::new(false),
            offers: RefCell::new(crate::clip_offers::Offers {
                inflight: None,
                waiting: None,
            }),
            consumed_keys: RefCell::new(Default::default()),
            provider: RefCell::new(None),
            entries: [host.clone(), user.clone(), secret.clone(), ca.clone()],
            assist_button: assist.clone(),
            assist_epoch: Arc::new(AtomicU64::new(1)),
            auto_gdm: RefCell::new(AutoGdm::default()),
            selected: RefCell::new(None),
            profile_metadata: RefCell::new(None),
            primary: RefCell::new(None),
            primary_attempted: Cell::new(false),
            primary_transport: RefCell::new(None),
            primary_checked: Cell::new(None),
            primary_status,
            primary_authorize,
            primary_click: RefCell::new(None),
            primary_error: RefCell::new(None),
            suppress_middle_release: Cell::new(false),
            selection_anchor: Cell::new(None),
        });
        let weak = Rc::downgrade(&v);
        let assist_popup = popup.clone();
        assist.connect_clicked(move |_| {
            assist_popup.popdown();
            if let Some(v) = weak.upgrade() {
                v.assist_dialog();
            }
        });
        let weak = Rc::downgrade(&v);
        v.area.connect_has_focus_notify(move |area| {
            if !area.has_focus() {
                if let Some(v) = weak.upgrade() {
                    // Automatic sign-in continues in the background; only a
                    // manual, confirmed sign-in is tied to keyboard focus.
                    if !v.auto_gdm.borrow().armed {
                        v.assist_epoch.fetch_add(1, Ordering::AcqRel);
                    }
                }
            }
        });
        for entry in [&host, &user, &secret, &ca] {
            let weak = Rc::downgrade(&v);
            entry.connect_changed(move |_| {
                if let Some(v) = weak.upgrade() {
                    v.assist_epoch.fetch_add(1, Ordering::AcqRel);
                    v.transient_pin.borrow_mut().take();
                    v.cancel_pending_connect("Connection cancelled: settings changed");
                }
            });
        }
        let weak = Rc::downgrade(&v);
        review_certificate.connect_clicked(move |_| {
            if let Some(v) = weak.upgrade() {
                v.review_peer_certificate();
            }
        });
        let weak = Rc::downgrade(&v);
        connect.connect_clicked(move |_| {
            if let Some(v) = weak.upgrade() {
                if let Err(error) = v.begin_connect(true) {
                    v.status.set_text(&error);
                }
            }
        });
        let weak = Rc::downgrade(&v);
        disconnect.connect_clicked(move |_| {
            if let Some(v) = weak.upgrade() {
                v.close();
            }
        });
        let tools = gtk::FlowBox::builder()
            .selection_mode(gtk::SelectionMode::None)
            .min_children_per_line(1)
            .max_children_per_line(3)
            .column_spacing(4)
            .row_spacing(4)
            .build();
        for (name, mode) in [
            ("Fit", Scale::Fit),
            ("Native pixels", Scale::Native),
            ("Stretch", Scale::Stretch),
            ("Dynamic resolution", Scale::Dynamic),
        ] {
            let b = gtk::Button::with_label(name);
            tools.insert(&b, -1);
            if matches!(mode, Scale::Fit | Scale::Dynamic) {
                quick_action(
                    &toolbar,
                    if mode == Scale::Fit { "fit" } else { "dynamic" },
                    if mode == Scale::Fit {
                        "zoom-fit-best-symbolic"
                    } else {
                        "view-restore-symbolic"
                    },
                    name,
                    &b,
                );
            }
            let w = Rc::downgrade(&v);
            b.connect_clicked(move |_| {
                if let Some(v) = w.upgrade() {
                    v.mode.set(mode);
                    if matches!(mode, Scale::Native | Scale::Dynamic) {
                        v.zoom.set(1.0);
                    }
                    v.request_preferences_save();
                    v.canvas();
                    v.area.queue_draw();
                }
            });
        }
        for (label, multiplier) in [("Zoom −", 0.8), ("Zoom +", 1.25)] {
            let b = gtk::Button::with_label(label);
            tools.insert(&b, -1);
            quick_action(
                &toolbar,
                if multiplier < 1.0 {
                    "zoom-out"
                } else {
                    "zoom-in"
                },
                if multiplier < 1.0 {
                    "zoom-out-symbolic"
                } else {
                    "zoom-in-symbolic"
                },
                label,
                &b,
            );
            let weak = Rc::downgrade(&v);
            b.connect_clicked(move |_| {
                if let Some(v) = weak.upgrade() {
                    // Manual local zoom leaves viewport-matched remote resolution mode.
                    if v.mode.get() == Scale::Dynamic {
                        v.mode.set(Scale::Fit);
                    }
                    v.zoom.set((v.zoom.get() * multiplier).clamp(0.25, 4.0));
                    v.request_preferences_save();
                    v.canvas();
                    v.area.queue_draw();
                }
            });
        }
        let pan = gtk::ToggleButton::with_label("Pan viewport");
        tools.insert(&pan, -1);
        let weak = Rc::downgrade(&v);
        pan.connect_toggled(move |b| {
            if let Some(v) = weak.upgrade() {
                v.pan.set(b.is_active());
            }
        });
        let full = gtk::Button::with_label("Fullscreen");
        tools.insert(&full, -1);
        quick_action(
            &toolbar,
            "fullscreen",
            "view-fullscreen-symbolic",
            "Fullscreen",
            &full,
        );
        let w = Rc::downgrade(&v);
        full.connect_clicked(move |_| {
            if let Some(v) = w.upgrade() {
                if let Some(win) = v.root.root().and_then(|r| r.downcast::<gtk::Window>().ok()) {
                    if win.is_fullscreen() {
                        win.unfullscreen();
                    } else {
                        win.fullscreen();
                    }
                    v.request_preferences_save();
                }
            }
        });
        for (name, keys) in [
            (
                "Ctrl+Alt+Del",
                vec![(0x1d, false), (0x38, false), (0x53, true)],
            ),
            ("PrintScreen", vec![(0x37, true)]),
            ("Alt+Tab", vec![(0x38, false), (0x0f, false)]),
            ("Super", vec![(0x5b, true)]),
            ("AltGr", vec![(0x38, true)]),
        ] {
            let b = gtk::Button::with_label(name);
            tools.insert(&b, -1);
            let w = Rc::downgrade(&v);
            b.connect_clicked(move |_| {
                if let Some(v) = w.upgrade() {
                    let mut events = Vec::new();
                    for (k, e) in &keys {
                        events.push(FastPathInputEvent::KeyboardEvent(
                            if *e {
                                KeyboardFlags::EXTENDED
                            } else {
                                KeyboardFlags::empty()
                            },
                            *k,
                        ));
                    }
                    for (k, e) in keys.iter().rev() {
                        events.push(FastPathInputEvent::KeyboardEvent(
                            KeyboardFlags::RELEASE
                                | if *e {
                                    KeyboardFlags::EXTENDED
                                } else {
                                    KeyboardFlags::empty()
                                },
                            *k,
                        ));
                    }
                    v.send(events);
                }
            });
        }
        let policy = gtk::DropDown::from_strings(&[
            "Clipboard: bidirectional",
            "Local → remote",
            "Remote → local",
            "Manual",
            "Off",
        ]);
        tools.insert(&policy, -1);
        let w = Rc::downgrade(&v);
        policy.connect_selected_notify(move |p| {
            if let Some(v) = w.upgrade() {
                v.policy.set(match p.selected() {
                    0 => ClipPolicy::Bidirectional,
                    1 => ClipPolicy::LocalToRemote,
                    2 => ClipPolicy::RemoteToLocal,
                    3 => ClipPolicy::Manual,
                    _ => ClipPolicy::Disabled,
                });
                v.request_preferences_save();
                v.broker.borrow_mut().local_copy();
                v.advertised.set(0);
                v.advertise(false);
            }
        });
        for (label, receive) in [("Send clipboard", false), ("Receive clipboard", true)] {
            let b = gtk::Button::with_label(label);
            tools.insert(&b, -1);
            let w = Rc::downgrade(&v);
            b.connect_clicked(move |_| {
                if let Some(v) = w.upgrade() {
                    if receive {
                        v.receive(true);
                    } else {
                        v.advertise(true);
                    }
                }
            });
        }
        tools.insert(&v.primary_authorize, -1);
        let weak = Rc::downgrade(&v);
        v.primary_authorize.connect_clicked(move |_| {
            if let Some(v) = weak.upgrade() {
                v.authorize_primary();
            }
        });
        let recover = gtk::Button::with_label("Recover GNOME login…");
        recover.set_widget_name("rdp-recover-sign-in");
        tools.insert(&recover, -1);
        let w = Rc::downgrade(&v);
        recover.connect_clicked(move |_| {
            if let Some(v) = w.upgrade() {
                v.recover_sign_in();
            }
        });
        let w = Rc::downgrade(&v);
        v.area.display().clipboard().connect_changed(move |_| {
            let Some(v) = w.upgrade() else {
                return;
            };
            if v.clipboard_refresh_pending.replace(true) {
                return;
            }
            let w = w.clone();
            // Run after every clipboard observer has advanced FocusBroker's
            // ownership epoch. Signal registration order must not authorize
            // stale remote offers or make fresh local copies disappear.
            glib::idle_add_local_once(move || {
                if let Some(v) = w.upgrade() {
                    v.clipboard_refresh_pending.set(false);
                    if v.broker.borrow().active() == Some(v.id.as_str()) {
                        v.bridge
                            .borrow()
                            .epoch
                            .store(v.broker.borrow().epoch(), Ordering::Release);
                        v.advertise(false);
                    }
                }
            });
        });
        let paste = gtk::Button::with_label("Paste clipboard into remote");
        tools.insert(&paste, -1);
        let w = Rc::downgrade(&v);
        paste.connect_clicked(move |_| {
            if let Some(v) = w.upgrade() {
                v.paste();
            }
        });
        controls.attach(&tools, 0, 3, 2, 1);
        let w = Rc::downgrade(&v);
        v.area.set_draw_func(move |_, cr, width, height| {
            if let Some(v) = w.upgrade() {
                v.drawn.set(v.drawn.get().saturating_add(1));
                cr.set_source_rgb(0.03, 0.03, 0.04);
                let _ = cr.paint();
                let _ = cr.save();
                if let Some(surface) = v.surface.borrow().as_ref() {
                    let (sx, sy, ox, oy) = v.geometry(width as f64, height as f64);
                    cr.translate(ox, oy);
                    cr.scale(sx, sy);
                    let _ = cr.set_source_surface(surface, 0.0, 0.0);
                    if cr.paint().is_ok() {
                        v.rendered_generation.set(v.surface_generation.get());
                    }
                }
                let _ = cr.restore();
                if v.auto_gdm.borrow().armed {
                    // Keep decoding/recognizing the verified framebuffer, but
                    // cover GDM until the authenticated desktop handover.
                    cr.set_source_rgb(0.07, 0.07, 0.08);
                    let _ = cr.paint();
                    cr.set_source_rgb(0.9, 0.9, 0.9);
                    cr.set_font_size(18.0);
                    cr.move_to(24.0, height as f64 / 2.0);
                    let _ = cr.show_text("Signing in to GNOME…");
                    cr.set_font_size(13.0);
                    cr.move_to(24.0, height as f64 / 2.0 + 28.0);
                    let _ = cr
                        .show_text("Remote input is paused. Cancel or press Escape to take over.");
                    if let Some(waiting) = v.auto_gdm.borrow().waiting {
                        cr.set_source_rgb(1.0, 0.82, 0.4);
                        cr.move_to(24.0, height as f64 / 2.0 + 52.0);
                        let _ = cr.show_text(waiting);
                    }
                } else if let Some(notice) = v.auto_gdm.borrow().notice.as_deref() {
                    // Why sign-in did not run stays on the remote screen itself,
                    // not only in the status line, until the user acts.
                    cr.set_font_size(14.0);
                    let text = notice.chars().take(160).collect::<String>();
                    let w = cr
                        .text_extents(&text)
                        .map(|e| e.width())
                        .unwrap_or(400.0)
                        .min(width as f64 - 32.0);
                    cr.set_source_rgba(0.45, 0.25, 0.0, 0.92);
                    cr.rectangle(8.0, 8.0, w + 24.0, 32.0);
                    let _ = cr.fill();
                    cr.set_source_rgb(1.0, 1.0, 1.0);
                    cr.move_to(20.0, 29.0);
                    let _ = cr.show_text(&text);
                }
            }
        });
        let key = gtk::EventControllerKey::new();
        let w = Rc::downgrade(&v);
        key.connect_key_pressed(move |_, k, code, modifiers| {
            if let Some(v) = w.upgrade() {
                if v.auto_gdm.borrow().armed {
                    if k == gdk::Key::Escape {
                        v.auto_gdm_stop("Automatic sign-in cancelled; manual login available");
                        v.auto_gdm.borrow_mut().notice.take();
                    }
                    return glib::Propagation::Stop;
                }
                if v.auto_gdm.borrow_mut().notice.take().is_some() {
                    v.area.queue_draw();
                }
                if k == gdk::Key::v
                    && modifiers.contains(gdk::ModifierType::CONTROL_MASK)
                    && !modifiers.intersects(
                        gdk::ModifierType::ALT_MASK
                            | gdk::ModifierType::SHIFT_MASK
                            | gdk::ModifierType::SUPER_MASK,
                    )
                {
                    v.consumed_keys.borrow_mut().insert(code);
                    v.paste();
                } else {
                    v.key(k, code, false);
                }
            }
            glib::Propagation::Stop
        });
        let w = Rc::downgrade(&v);
        key.connect_key_released(move |_, k, code, _| {
            if let Some(v) = w.upgrade() {
                if v.consumed_keys.borrow_mut().remove(&code) {
                    return;
                }
                if v.auto_gdm.borrow().armed
                    || (k == gdk::Key::Escape && !v.held.borrow().contains(&(0x01, false)))
                {
                    return;
                }
                v.key(k, code, true);
            }
        });
        v.area.add_controller(key);
        let focus = gtk::EventControllerFocus::new();
        let w = Rc::downgrade(&v);
        focus.connect_leave(move |_| {
            if let Some(v) = w.upgrade() {
                v.release_keys();
            }
        });
        v.area.add_controller(focus);
        let click = gtk::GestureClick::new();
        click.set_button(0);
        let w = Rc::downgrade(&v);
        click.connect_pressed(move |g, _, x, y| {
            if let Some(v) = w.upgrade() {
                if v.auto_gdm.borrow().armed {
                    // Input stays paused, but the click gives this screen the
                    // focus that a waiting sign-in needs.
                    v.focus();
                    return;
                }
                v.auto_gdm.borrow_mut().notice.take();
                v.focus();
                v.pointer(x, y, g.current_button(), false);
            }
        });
        let w = Rc::downgrade(&v);
        click.connect_released(move |g, _, x, y| {
            if let Some(v) = w.upgrade() {
                v.pointer(x, y, g.current_button(), true);
            }
        });
        let w = Rc::downgrade(&v);
        click.connect_cancel(move |_, _| {
            if let Some(v) = w.upgrade() {
                v.release_buttons();
            }
        });
        v.area.add_controller(click);
        let motion = gtk::EventControllerMotion::new();
        let w = Rc::downgrade(&v);
        motion.connect_motion(move |_, x, y| {
            if let Some(v) = w.upgrade() {
                v.pointer(x, y, 0, false);
            }
        });
        v.area.add_controller(motion);
        let scroll = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::BOTH_AXES);
        let weak = Rc::downgrade(&v);
        scroll.connect_scroll(move |controller, dx, dy| {
            if let Some(v) = weak.upgrade() {
                if v.auto_gdm.borrow().armed {
                    return glib::Propagation::Stop;
                }
                if controller
                    .current_event_state()
                    .contains(gdk::ModifierType::CONTROL_MASK)
                {
                    v.zoom
                        .set((v.zoom.get() * (-dy * 0.1).exp()).clamp(0.25, 4.0));
                    v.request_preferences_save();
                    v.canvas();
                    v.area.queue_draw();
                    return glib::Propagation::Stop;
                }
                if v.pan.get() {
                    return glib::Propagation::Proceed;
                }
                let (x, y) = v.pointer_position.get();
                let (horizontal, delta) = if dy.abs() >= dx.abs() {
                    (false, -dy)
                } else {
                    (true, dx)
                };
                let units = (delta * 120.0).round().clamp(-256.0, 255.0) as i16;
                if units != 0 {
                    v.send(vec![FastPathInputEvent::MouseEvent(MousePdu {
                        flags: if horizontal {
                            PointerFlags::HORIZONTAL_WHEEL
                        } else {
                            PointerFlags::VERTICAL_WHEEL
                        },
                        number_of_wheel_rotation_units: units,
                        x_position: x,
                        y_position: y,
                    })]);
                }
            }
            glib::Propagation::Stop
        });
        v.area.add_controller(scroll);
        let cancel_auto = gtk::Button::with_label("Cancel automatic sign-in");
        cancel_auto.set_widget_name("cancel-automatic-sign-in");
        cancel_auto.set_visible(false);
        toolbar.append(&cancel_auto);
        let weak = Rc::downgrade(&v);
        cancel_auto.connect_clicked(move |_| {
            if let Some(v) = weak.upgrade() {
                if v.auto_gdm.borrow().armed {
                    v.auto_gdm_stop("Automatic sign-in cancelled; manual login available");
                    v.auto_gdm.borrow_mut().notice.take();
                } else {
                    v.retry_auto_gdm();
                }
                v.focus();
            }
        });
        let w = Rc::downgrade(&v);
        glib::timeout_add_local(Duration::from_millis(16), move || {
            let Some(v) = w.upgrade() else {
                return glib::ControlFlow::Break;
            };
            v.pending_connect_tick();
            v.flush_commands();
            v.update();
            v.auto_gdm_tick();
            v.primary_tick();
            let (armed, retry) = (v.auto_gdm.borrow().armed, v.auto_gdm_retry_available());
            cancel_auto.set_visible(armed || retry);
            cancel_auto.set_label(if armed {
                "Cancel automatic sign-in"
            } else {
                "Retry automatic sign-in"
            });
            v.canvas();
            if v.preferences_due
                .get()
                .is_some_and(|at| at.elapsed() > Duration::from_millis(500))
            {
                v.preferences_due.set(None);
                if let Some(save) = v.preferences.borrow().as_ref() {
                    let full = v
                        .root
                        .root()
                        .and_then(|w| w.downcast::<gtk::Window>().ok())
                        .is_some_and(|w| w.is_fullscreen());
                    save(
                        match v.mode.get() {
                            Scale::Fit => "fit",
                            Scale::Native => "native",
                            Scale::Stretch => "stretch",
                            Scale::Dynamic => "dynamic",
                        },
                        v.zoom.get(),
                        full,
                        match v.policy.get() {
                            ClipPolicy::Bidirectional => 0,
                            ClipPolicy::LocalToRemote => 1,
                            ClipPolicy::RemoteToLocal => 2,
                            ClipPolicy::Manual => 3,
                            ClipPolicy::Disabled => 4,
                        },
                    );
                }
            }
            let reactivating = v
                .connection
                .borrow()
                .as_ref()
                .is_some_and(|c| c.state.lock().unwrap().reactivating);
            let cap = v
                .connection
                .borrow()
                .as_ref()
                .and_then(|c| c.state.lock().unwrap().display_cap);
            let (rw, rh) = v.size.get();
            v.display_info.set_text(&format!(
                "Remote {rw}×{rh} · viewport {}×{} · {:.0}% · {} · {}",
                v.scroll.width(),
                v.scroll.height(),
                v.zoom.get() * 100.0,
                if cap.is_some() {
                    "Display Control negotiated"
                } else {
                    "fixed resolution / local scaling"
                },
                if v.broker.borrow().active() == Some(v.id.as_str()) {
                    "active tab"
                } else {
                    "inactive tab; clipboard offers cached"
                }
            ));
            // Let the recognized GDM form keep its framebuffer coordinates
            // until sign-in finishes or stops. Our own viewport resize must not
            // invalidate an in-flight credential target.
            if v.mode.get() == Scale::Dynamic
                && cap.is_some()
                && v.root.is_mapped()
                && !v.auto_gdm.borrow().armed
            {
                let scale = v
                    .area
                    .native()
                    .and_then(|n| n.surface())
                    .map(|s| s.scale())
                    .unwrap_or(1.0);
                let raw = (
                    (((v.scroll.width() as f64 * scale).clamp(200.0, 4096.0) as u16) / 2) * 2,
                    (v.scroll.height() as f64 * scale).clamp(200.0, 4096.0) as u16,
                );
                let desired = stable_resize(raw, v.desired.get());
                if desired != v.desired.get() {
                    v.desired.set(desired);
                    v.resize_at.set(Some(std::time::Instant::now()));
                }
                if v.size.get() == desired {
                    v.requested.set(desired);
                }
                if !reactivating
                    && v.requested.get() != desired
                    && v.resize_at
                        .get()
                        .is_some_and(|at| at.elapsed() > Duration::from_millis(250))
                {
                    v.command(Command::Resize(
                        desired.0,
                        desired.1,
                        (v.area
                            .native()
                            .and_then(|n| n.surface())
                            .map(|s| s.scale())
                            .unwrap_or(1.0)
                            * 100.0)
                            .round()
                            .clamp(100.0, 500.0) as u32,
                    ));
                    v.requested.set(desired);
                }
            }
            glib::ControlFlow::Continue
        });
        let weak = Rc::downgrade(&v);
        reconnect_center.connect_clicked(move |_| {
            if let Some(v) = weak.upgrade() {
                if let Err(message) = v.automation_reconnect() {
                    v.status.set_text(&message);
                }
            }
        });
        let weak = Rc::downgrade(&v);
        glib::timeout_add_local(Duration::from_millis(100), move || {
            let Some(v) = weak.upgrade() else {
                return glib::ControlFlow::Break;
            };
            let phase = v.metadata()["phase"]
                .as_str()
                .unwrap_or("disconnected")
                .to_owned();
            let configured = !v.entries[0].text().is_empty();
            let show = configured && phase != "connected";
            v.disconnect_card.set_visible(show);
            if show {
                let message = match phase.as_str() {
                    "connecting" => "Connecting…".to_owned(),
                    "reconnecting" => v.status.text().to_string(),
                    "error" => v.status.text().to_string(),
                    _ => "Disconnected".to_owned(),
                };
                v.disconnect_message.set_text(&message);
                v.disconnect_message
                    .set_tooltip_text(Some(&v.status.text()));
            }
            glib::ControlFlow::Continue
        });
        v
    }
    pub fn fixture_connect(&self, port: u16, ca: &std::path::Path) {
        // This generated peer starts at a fixed size; resize cases opt in explicitly.
        self.mode.set(Scale::Fit);
        self.close();
        let c = Connection::start_with_secret(
            Profile {
                host: "127.0.0.1".into(),
                port,
                user: "fixture".into(),
                reference: String::new(),
                reconnect: true,
                desktop_size: (1280, 720),
                graphics: false,
                clipboard: self.broker.borrow().desktop_enabled(),
                handover: None,
                ca: Some(ca.to_string_lossy().into_owned()),
            },
            Some("generated-local-only".into()),
        );
        *self.bridge.borrow_mut() = c.state.lock().unwrap().bridge.clone();
        *self.connection.borrow_mut() = Some(c);
        self.focus();
    }
    pub fn fixture_toolbar(&self, action: Option<&str>) -> serde_json::Value {
        let toolbar = &self.toolbar;
        let mut controls = Vec::new();
        let mut controls_menu = None;
        let mut clicked = false;
        let mut child = toolbar.first_child();
        while let Some(w) = child {
            child = w.next_sibling();
            if let Some(b) = w.downcast_ref::<gtk::Button>() {
                let name = b.widget_name().to_string();
                controls.push(serde_json::json!({"action":name,"icon":b.icon_name().map(|s|s.to_string()),"label":b.tooltip_text().map(|s|s.to_string()),"mapped":b.is_mapped()}));
                if action == Some(name.as_str()) {
                    b.emit_clicked();
                    clicked = true;
                }
            }
            if let Some(menu) = w.downcast_ref::<gtk::MenuButton>() {
                let origin = self
                    .root
                    .root()
                    .and_downcast::<gtk::Window>()
                    .and_then(|window| {
                        menu.compute_point(&window, &gtk::graphene::Point::new(0., 0.))
                    })
                    .map(|p| [p.x(), p.y()]);
                controls_menu = Some(
                    serde_json::json!({"action":"rdp-controls","mapped":menu.is_mapped(),
                    "origin":origin,"size":[menu.width(),menu.height()]}),
                );
            }
        }
        let button = &self.primary_authorize;
        let origin = self
            .root
            .root()
            .and_downcast::<gtk::Window>()
            .and_then(|w| button.compute_point(&w, &gtk::graphene::Point::new(0., 0.)))
            .map(|p| [p.x(), p.y()]);
        let native_point = button.native().and_then(|native| {
            let widget = native.clone().dynamic_cast::<gtk::Widget>().ok()?;
            let point = button.compute_point(&widget, &gtk::graphene::Point::new(0., 0.))?;
            let (x, y) = native.surface_transform();
            Some(serde_json::json!({"kind":widget.type_().name(),"point":[point.x() as f64-x,point.y() as f64-y]}))
        });
        serde_json::json!({"controls":controls,"controls_menu":controls_menu,"clicked":clicked,"state":self.fixture_inspect(),
            "primary_control":{"native":native_point,"label":button.label().map(|s|s.to_string()),"visible":button.is_visible(),
                "mapped":button.is_mapped(),"sensitive":button.is_sensitive(),"origin":origin,
                "size":[button.width(),button.height()]}})
    }
    pub fn fixture_inspect(&self) -> serde_json::Value {
        let metrics = self.connection.borrow().as_ref().map(|c| {
            let s = c.state.lock().unwrap();
            let times: Vec<_> = s
                .cadence
                .iter()
                .zip(s.cadence.iter().skip(1))
                .map(|(a, b)| b.duration_since(*a).as_secs_f64() * 1000.0)
                .collect();
            serde_json::json!({"decoded_updates":s.frames,"decoded_sizes":s.decoded_sizes,"update_intervals_ms":times})
        });
        serde_json::json!({"draws":self.drawn.get(),"mouse_selection":self.primary_status.text().as_str(),"selection_paste_pending":self.primary_click.borrow().is_some(),"selection_sharing_ready":self.primary.borrow().as_ref().is_some_and(|b|b.ready),"connection_pending":self.pending_connect.borrow().is_some(),"transport_id":self.connection.borrow().as_ref().map(|c|c.id.as_str()),"requested_size":self.requested.get(),"display_mode":format!("{:?}",self.mode.get()),"reconnect_visible":self.disconnect_card.is_visible(),"metrics":metrics,"selection_settings":self.profile_metadata.borrow().as_ref().map(|p|serde_json::json!({"enabled":p.preferences.primary_sync,"paste":p.preferences.primary_paste})),"gnome_auto":{"armed":self.auto_gdm.borrow().armed,"step":format!("{:?}",self.auto_gdm.borrow().sequence.step),"input_epoch":self.assist_epoch.load(Ordering::Acquire),"guards":{"canvas_focus":self.area.has_focus(),"window_active":self.root.root().and_downcast::<gtk::Window>().is_some_and(|w|w.is_active()),"selected":self.selected.borrow().as_ref().is_some_and(|f|f()),"profile_current":self.profile_metadata.borrow().as_ref().is_some_and(crate::profiles::assist_profile_current),"rendered_generation":self.rendered_generation.get(),"surface_generation":self.surface_generation.get()},"generation":self.connection.borrow().as_ref().map(|c|c.state.lock().unwrap().generation),"handovers":self.connection.borrow().as_ref().map(|c|c.state.lock().unwrap().handovers)},"last_pixel":self.last_pixel.get(),"status":self.status.text().as_str(),"size":self.size.get(),"zoom":self.zoom.get(),"pan":[self.scroll.hadjustment().value(),self.scroll.vadjustment().value()],"viewport":[self.area.width(),self.area.height()],"window_width":self.root.root().and_then(|r|r.downcast::<gtk::Window>().ok()).map(|w|w.width()),"active":self.broker.borrow().active()==Some(self.id.as_str()),"epoch":self.broker.borrow().epoch(),"pending":self.pending.borrow().is_some(),"received_generation":self.received_generation.get(),"offers":self.offers_seen.get(),"policy":format!("{:?}",self.policy.get())})
    }
    pub fn fixture_policy(&self, index: u32) {
        fn find(w: &gtk::Widget) -> Option<gtk::DropDown> {
            if let Some(d) = w.downcast_ref::<gtk::DropDown>() {
                return Some(d.clone());
            }
            let mut c = w.first_child();
            while let Some(child) = c {
                c = child.next_sibling();
                if let Some(d) = find(&child) {
                    return Some(d);
                }
            }
            None
        }
        if let Some(drop) = find(self.toolbar.upcast_ref()) {
            drop.set_selected(index.min(4));
        }
    }
    pub fn fixture_clipboard(&self, receive: bool) {
        if receive {
            self.receive(true);
        } else {
            self.advertise(true);
        }
    }
    fn geometry(&self, w: f64, h: f64) -> (f64, f64, f64, f64) {
        viewport_geometry(
            (w, h),
            (
                self.scroll.width().max(1) as f64,
                self.scroll.height().max(1) as f64,
            ),
            self.size.get(),
            self.mode.get(),
            self.zoom.get(),
            self.area
                .native()
                .and_then(|n| n.surface())
                .map(|s| s.scale())
                .unwrap_or(1.0),
        )
    }
    fn canvas(&self) {
        let (rw, rh) = self.size.get();
        let (sx, sy, _, _) = self.geometry(0.0, 0.0);
        self.area.set_content_width((rw as f64 * sx).ceil() as i32);
        self.area.set_content_height((rh as f64 * sy).ceil() as i32);
    }
    fn review_peer_certificate(self: &Rc<Self>) {
        let Some(review) = self.certificate.borrow().clone() else {
            return;
        };
        let Some(connection_id) = self.connection.borrow().as_ref().map(|c| c.id.clone()) else {
            return;
        };
        self.certificate_prompt_pending.set(false);
        let dialog_name = format!("rdp-certificate-dialog-{}", self.id);
        if gtk::Window::list_toplevels()
            .iter()
            .any(|w| w.widget_name() == dialog_name)
        {
            return;
        }
        let snapshot: Vec<String> = self.entries.iter().map(|e| e.text().into()).collect();
        let parent = self
            .root
            .root()
            .and_then(|w| w.downcast::<gtk::Window>().ok());
        let dialog = gtk::Dialog::builder()
            .title(if review.changed {
                "RDP certificate changed"
            } else {
                "Verify RDP certificate"
            })
            .modal(true)
            .default_width(580)
            .build();
        dialog.set_widget_name(&format!("rdp-certificate-dialog-{}", self.id));
        dialog.set_transient_for(parent.as_ref());
        dialog.add_button("Cancel", gtk::ResponseType::Cancel);
        let once = dialog.add_button("Trust for this tab", gtk::ResponseType::Other(1));
        let saved = if self.trust_sink.borrow().is_some() {
            Some(dialog.add_button("Save trust for this profile", gtk::ResponseType::Accept))
        } else {
            None
        };
        dialog.set_default_response(gtk::ResponseType::Cancel);
        let box_ = dialog.content_area();
        box_.set_spacing(8);
        box_.set_margin_start(12);
        box_.set_margin_end(12);
        box_.set_margin_top(12);
        box_.set_margin_bottom(12);
        let message = gtk::Label::new(Some(if review.changed {
            "The saved certificate does not match. Verify the new fingerprint with the machine administrator before replacing trust."
        } else {
            "The certificate could not be verified by your trusted CAs. Check its fingerprint through a trusted channel before approving this endpoint."
        }));
        message.set_wrap(true);
        box_.append(&message);
        let details = gtk::Label::new(Some(&format!(
            "Endpoint: {}:{}\nSubject: {}\nIssuer: {}\nValid from: {}\nValid until: {}\nSHA-256: {}{}",
            review.host,
            review.port,
            review.subject,
            review.issuer,
            review.valid_from,
            review.valid_until,
            review.fingerprint(),
            review
                .previous
                .as_ref()
                .map(|p| format!("\nPrevious SHA-256: {p}"))
                .unwrap_or_default()
        )));
        details.set_widget_name("rdp-certificate-details");
        details.set_wrap(true);
        details.set_selectable(true);
        details.set_xalign(0.0);
        box_.append(&details);
        let confirmation = gtk::Entry::new();
        confirmation.set_widget_name("rdp-certificate-confirmation");
        if review.changed {
            confirmation
                .set_placeholder_text(Some("Type TRUST after verifying the changed fingerprint"));
            box_.append(&confirmation);
        }
        let error = gtk::Label::new(None);
        error.set_wrap(true);
        box_.append(&error);
        if !review.valid_now {
            error.set_text("This certificate is expired or not yet valid. Renew it on the server before connecting.");
        }
        once.set_sensitive(review.valid_now && !review.changed);
        if let Some(saved) = saved.as_ref() {
            saved.set_sensitive(review.valid_now && !review.changed);
        }
        if review.changed {
            let (once, saved, valid) = (once.clone(), saved.clone(), review.valid_now);
            confirmation.connect_changed(move |entry| {
                let allowed = valid && entry.text() == "TRUST";
                once.set_sensitive(allowed);
                if let Some(saved) = saved.as_ref() {
                    saved.set_sensitive(allowed);
                }
            });
        }
        let weak = Rc::downgrade(self);
        dialog.connect_response(move |dialog,response| {
            if !matches!(response, gtk::ResponseType::Accept | gtk::ResponseType::Other(1)) { dialog.close();return; }
            let Some(v)=weak.upgrade() else {dialog.close();return;};
            if !review.valid_now || (review.changed && confirmation.text()!="TRUST") { return; }
            let unchanged=v.root.root().is_some() && v.entries.iter().zip(&snapshot).all(|(e,value)|e.text().as_str()==value)
                && v.connection.borrow().as_ref().is_some_and(|c|c.id==connection_id);
            if !unchanged {error.set_text("Connection settings changed while reviewing; cancel and review the current endpoint again.");return;}
            let pin=review.token();
            if response==gtk::ResponseType::Accept {
                let result=v.trust_sink.borrow().as_ref().ok_or_else(||"This tab has no saved profile".to_string())
                    .and_then(|save|save(&snapshot[0],&snapshot[3],&pin));
                if let Err(reason)=result {error.set_text(&reason);return;}
                v.entries[3].set_text(&pin);
                if let Some(p)=v.profile_metadata.borrow_mut().as_mut(){p.preferences.trust=pin;}
            } else { *v.transient_pin.borrow_mut()=Some(pin); }
            dialog.close();
            if let Err(reason)=v.begin_connect(false){v.status.set_text(&reason);}
        });
        dialog.present();
    }
    pub fn fixture_trust(self: &Rc<Self>, q: &serde_json::Value) -> serde_json::Value {
        // Test-only dialog driver cannot approve real-host or owner credentials.
        let address = self.entries[0].text();
        if !crate::profiles::endpoint(&address, 3389).is_ok_and(|(host, _)| {
            host.parse::<std::net::IpAddr>()
                .is_ok_and(|ip| ip.is_loopback())
        }) {
            return serde_json::json!({"error":"Certificate fixture requires a loopback endpoint"});
        }
        if q["operation"] == "review" {
            self.review_certificate.emit_clicked();
        }
        let mut open = false;
        for window in gtk::Window::list_toplevels() {
            if window.widget_name().as_str() != format!("rdp-certificate-dialog-{}", self.id) {
                continue;
            }
            if let Ok(dialog) = window.downcast::<gtk::Dialog>() {
                open = true;
                if let Some(value) = q["confirmation"].as_str() {
                    fn update(w: &gtk::Widget, value: &str) {
                        if w.widget_name() == "rdp-certificate-confirmation" {
                            if let Some(e) = w.downcast_ref::<gtk::Entry>() {
                                e.set_text(value);
                            }
                        }
                        let mut c = w.first_child();
                        while let Some(w) = c {
                            c = w.next_sibling();
                            update(&w, value);
                        }
                    }
                    update(dialog.upcast_ref(), value);
                }
                match q["response"].as_str() {
                    Some("cancel") => dialog.response(gtk::ResponseType::Cancel),
                    Some("once") => dialog.response(gtk::ResponseType::Other(1)),
                    Some("save") => dialog.response(gtk::ResponseType::Accept),
                    _ => (),
                }
            }
        }
        serde_json::json!({"dialog_open":open,"pending":self.certificate.borrow().is_some(),"review_available":self.review_certificate.is_visible(),"saved_pin":self.entries[3].text().starts_with("pin-sha256:"),"tab_pin":self.transient_pin.borrow().is_some(),"certificate":self.certificate.borrow().as_ref().map(|r|serde_json::json!({"sha256":r.fingerprint(),"token":r.token(),"changed":r.changed,"valid_now":r.valid_now}))})
    }
    pub fn set_trust_sink(&self, sink: crate::profiles::TrustSink) {
        *self.trust_sink.borrow_mut() = Some(sink);
    }
    pub fn set_preferences_sink(&self, sink: crate::profiles::PreferenceSink) {
        *self.preferences.borrow_mut() = Some(sink);
    }
    fn request_preferences_save(&self) {
        if self.preferences.borrow().is_some() {
            self.preferences_due.set(Some(std::time::Instant::now()));
        }
    }
    pub fn configuration(&self) -> Option<crate::profiles::Profile> {
        if self.entries[0].text().is_empty() {
            return None;
        }
        let mut p = self
            .profile_metadata
            .borrow()
            .clone()
            .unwrap_or(crate::profiles::Profile {
                id: String::new(),
                machine_id: String::new(),
                preferences: crate::profiles::Preferences::default(),
                inherit: false,
                inherit_credential: false,
                inherit_connection: false,
                name: "RDP".into(),
                group: String::new(),
                protocol: "RDP".into(),
                server: String::new(),
                username: String::new(),
                secret_reference: String::new(),
                favorite: false,
                last_used: 0,
                tags: Vec::new(),
            });
        p.server = self.entries[0].text().into();
        p.username = self.entries[1].text().into();
        p.secret_reference = self.entries[2].text().into();
        p.preferences.trust = self.entries[3].text().into();
        p.preferences.display = match self.mode.get() {
            Scale::Fit => "fit",
            Scale::Native => "native",
            Scale::Stretch => "stretch",
            Scale::Dynamic => "dynamic",
        }
        .into();
        p.preferences.zoom = self.zoom.get();
        p.preferences.fullscreen = self.fullscreen.get();
        p.preferences.reconnect = self.reconnect.get();
        p.preferences.clipboard = self.policy.get() as u32;
        Some(p)
    }
    pub fn configure(&self, p: &crate::profiles::Profile) {
        self.primary_checked.set(None);
        self.primary.borrow_mut().take();
        self.primary_attempted.set(false);
        self.cancel_pending_connect("Connection cancelled: profile changed");
        self.assist_epoch.fetch_add(1, Ordering::AcqRel);
        self.assist_button
            .set_visible(p.preferences.rdp_mode == "gnome-remote-login");
        self.root.set_tooltip_text(None);
        *self.profile_metadata.borrow_mut() = Some(p.clone());
        self.set_profile(
            &p.server,
            &p.username,
            &p.secret_reference,
            &p.preferences.trust,
        );
        self.mode.set(match p.preferences.display.as_str() {
            "native" => Scale::Native,
            "stretch" => Scale::Stretch,
            "dynamic" => Scale::Dynamic,
            _ => Scale::Fit,
        });
        self.zoom.set(if self.mode.get() == Scale::Dynamic {
            1.0
        } else {
            p.preferences.zoom
        });
        self.reconnect.set(p.preferences.reconnect);
        self.fullscreen.set(p.preferences.fullscreen);
        self.fixture_policy(p.preferences.clipboard);
        self.canvas();
    }
    fn begin_connect(&self, activate: bool) -> Result<(), String> {
        self.refresh_saved_profile_for_reconnect()?;
        let address = self.entries[0].text();
        let (host, port) = crate::profiles::endpoint(&address, 3389)?;
        let user = self.entries[1].text();
        let reference = self.entries[2].text();
        let ca = self
            .transient_pin
            .borrow()
            .clone()
            .unwrap_or_else(|| self.entries[3].text().into());
        if user.is_empty() || reference.is_empty() {
            self.status
                .set_text("Host, user and secret reference are required");
            return Err("Configured user and credential reference required".into());
        }
        let profile = Profile {
            host,
            port,
            user: user.into(),
            reference: reference.into(),
            reconnect: self.reconnect.get(),
            desktop_size: self.last_size.get().unwrap_or((1280, 720)),
            clipboard: self.broker.borrow().desktop_enabled(),
            handover: None,
            // Advertise the implemented lossless EGFX pipeline on standard RDP
            // too; modern Windows uses it for dynamic desktop updates.
            graphics: true,
            ca: if ca.is_empty() { None } else { Some(ca.into()) },
        };
        self.close();
        self.certificate.borrow_mut().take();
        self.certificate_prompt_pending.set(false);
        self.review_certificate.set_visible(false);
        if self.mode.get() == Scale::Dynamic {
            // New tabs and fullscreen transitions have no trustworthy canvas
            // allocation in their creation callback. Do not authenticate at a
            // temporary fallback size and move remote windows a second time.
            *self.pending_connect.borrow_mut() = Some(PendingConnect {
                profile,
                began: std::time::Instant::now(),
                viewport: InitialViewport::default(),
            });
            if activate {
                self.focus();
            }
            self.status.set_text("Waiting for desktop layout…");
        } else {
            self.start_connection(profile, activate);
        }
        Ok(())
    }
    fn refresh_saved_profile_for_reconnect(&self) -> Result<(), String> {
        let Some(previous) = self.profile_metadata.borrow().clone() else {
            return Ok(());
        };
        // Preserve manually edited connection fields. A reconnect of a saved,
        // unchanged endpoint picks up its current selection and sign-in options.
        if self.entries[0].text() != previous.server
            || self.entries[1].text() != previous.username
            || self.entries[2].text() != previous.secret_reference
            || self.entries[3].text() != previous.preferences.trust
        {
            return Ok(());
        }
        let Some(fresh) = crate::profiles::saved_reconnect_profile(&previous)? else {
            return Ok(());
        };
        // saved_reconnect_profile has verified the exact host/user/credential/
        // trust identity. Reapplying identical entry text emits changed and
        // clears a tab-only certificate approval, even though its endpoint has
        // not changed. Keep that approval across this options-only refresh.
        let pin = self.transient_pin.borrow().clone();
        self.configure(&fresh);
        *self.transient_pin.borrow_mut() = pin;
        Ok(())
    }
    fn cancel_pending_connect(&self, reason: &str) {
        if self.pending_connect.borrow_mut().take().is_some() {
            self.status.set_text(reason);
        }
    }
    fn connection_viewport(&self) -> Option<(u16, u16)> {
        let scale = self
            .area
            .native()
            .and_then(|n| n.surface())
            .map(|s| s.scale())
            .unwrap_or(1.0);
        let dimensions = |w: i32, h: i32| {
            (w > 0 && h > 0).then(|| {
                (
                    ((w as f64 * scale).clamp(200.0, 4096.0) as u16) / 2 * 2,
                    (h as f64 * scale).clamp(200.0, 4096.0) as u16,
                )
            })
        };
        if self.root.is_mapped() {
            return dimensions(self.scroll.width(), self.scroll.height());
        }
        // A hidden reconnect keeps its last actual framebuffer, including a
        // split workspace's smaller viewport. A new background tab uses the
        // allocated TabView without selecting it or taking clipboard ownership.
        self.last_size.get().or_else(|| {
            self.root
                .ancestor(adw::TabView::static_type())
                .filter(|tabs| tabs.is_mapped())
                .and_then(|tabs| dimensions(tabs.width(), tabs.height()))
        })
    }
    fn pending_connect_tick(&self) {
        if self.pending_connect.borrow().is_none() {
            return;
        }
        if self.mode.get() != Scale::Dynamic {
            self.cancel_pending_connect("Connection cancelled: display mode changed");
            return;
        }
        let now = std::time::Instant::now();
        let size = self.connection_viewport();
        let mut pending = self.pending_connect.borrow_mut();
        let Some(request) = pending.as_mut() else {
            return;
        };
        if now.duration_since(request.began) > Duration::from_secs(15) {
            pending.take();
            self.status
                .set_text("Desktop layout is unavailable; show this window and reconnect");
            return;
        }
        let Some(size) = request.viewport.observe(size, now) else {
            return;
        };
        let mut request = pending.take().unwrap();
        drop(pending);
        request.profile.desktop_size = size;
        self.start_connection(request.profile, false);
    }
    fn start_connection(&self, profile: Profile, activate: bool) {
        let connection = Connection::start(profile);
        *self.bridge.borrow_mut() = connection.state.lock().unwrap().bridge.clone();
        *self.connection.borrow_mut() = Some(connection);
        let mut automatic = AutoGdm::default();
        if let Some(p) = self.profile_metadata.borrow().as_ref() {
            match auto_sign_in_missing(p) {
                None => automatic.armed = true,
                Some(missing) => automatic.notice = missing.map(str::to_owned),
            }
        }
        *self.auto_gdm.borrow_mut() = automatic;
        if activate {
            self.focus();
        }
        self.status
            .set_text("Connecting with certificate validation…");
    }
    pub fn automation_reconnect(&self) -> Result<(), String> {
        self.begin_connect(false)
    }
    pub fn event_state(&self) -> (String, bool) {
        if self.pending_connect.borrow().is_some() {
            return ("connecting".into(), false);
        }
        self.connection
            .borrow()
            .as_ref()
            .map(|c| {
                let s = c.state.lock().unwrap();
                (s.phase.clone(), s.frames > 0)
            })
            .unwrap_or(("disconnected".into(), false))
    }
    /// Cheap production tab summary: no fixture metrics, credential/profile
    /// freshness checks, cadence vectors or framebuffer snapshots.
    pub fn tab_summary(&self) -> (String, String) {
        let (phase, _) = self.event_state();
        let (w, h) = self.size.get();
        let text = format!(
            "{}\nState: {}\nEndpoint: {}\nDesktop size: {} × {}\nLatency: not measured\nClipboard: {:?}",
            self.tab_title(),
            self.status.text(),
            self.entries[0].text(),
            w,
            h,
            self.policy.get()
        );
        (phase, text)
    }
    pub fn tab_title(&self) -> String {
        self.profile_metadata
            .borrow()
            .as_ref()
            .map(|p| p.name.clone())
            .unwrap_or_else(|| {
                let host = self.entries[0].text();
                if host.is_empty() {
                    "Remote desktop".into()
                } else {
                    host.to_string()
                }
            })
    }
    pub fn metadata(&self) -> serde_json::Value {
        let connection = self.connection.borrow();
        let automatic_sign_in = {
            let a = self.auto_gdm.borrow();
            serde_json::json!({"armed":a.armed,"stage":format!("{:?}",a.sequence.step),"last_screen":a.last_observation.map(|kind|format!("{kind:?}")),"retry_available":self.auto_gdm_retry_available()})
        };
        let phase = connection
            .as_ref()
            .map(|c| c.state.lock().unwrap().phase.clone())
            .unwrap_or_else(|| {
                if self.pending_connect.borrow().is_some() {
                    "connecting".into()
                } else {
                    "disconnected".into()
                }
            });
        serde_json::json!({"id":self.root.widget_name().as_str(),"automatic_sign_in":automatic_sign_in,"phase":phase,"display_mode":format!("{:?}",self.mode.get()),"viewport":[self.scroll.width(),self.scroll.height()],"zoom":self.zoom.get(),"input_paused_for_login":self.auto_gdm.borrow().armed,"reactivating":connection.as_ref().is_some_and(|c|c.state.lock().unwrap().reactivating),"mouse_selection":self.primary_status.text().as_str(),"connection_stage":connection.as_ref().map(|c|c.state.lock().unwrap().connection_stage.clone()),"recovery":connection.as_ref().map(|c|{let st=c.state.lock().unwrap();serde_json::json!({"attempt":st.reconnect_attempt,"elapsed_ms":st.reconnect_elapsed.as_millis() as u64,"background":st.reconnect_background})}),"size":self.size.get(),"transport_id":connection.as_ref().map(|c|c.id.as_str()),"decoded_updates":connection.as_ref().map(|c|c.state.lock().unwrap().frames).unwrap_or(0),"clipboard_owner":self.broker.borrow().active()==Some(self.id.as_str()),"display_control":connection.as_ref().map(|c|{let s=c.state.lock().unwrap();serde_json::json!({"channel_opened":s.display_channel_opened,"capability_messages":s.display_capability_messages,"negotiated":s.display_cap.is_some()})}),"clipboard_channel_negotiated":connection.as_ref().is_some_and(|c|c.state.lock().unwrap().clipboard_channel_negotiated),"verified_tls_sha256":connection.as_ref().and_then(|c|c.state.lock().unwrap().verified_tls_sha256.clone()),"protocol_error":connection.as_ref().and_then(|c|c.state.lock().unwrap().protocol_error.clone()),"authentication_error":connection.as_ref().and_then(|c|c.state.lock().unwrap().authentication_error.clone()),"certificate_review":self.certificate.borrow().as_ref().map(|r|serde_json::json!({"sha256":r.fingerprint(),"changed":r.changed,"valid_now":r.valid_now})),"profile":self.profile_metadata.borrow().as_ref().map(|p|p.id.clone())})
    }
    pub fn connect_profile(&self) {
        fn find(w: &gtk::Widget) -> Option<gtk::Button> {
            if let Some(b) = w.downcast_ref::<gtk::Button>() {
                if b.label().as_deref() == Some("Connect / reconnect") {
                    return Some(b.clone());
                }
            }
            let mut c = w.first_child();
            while let Some(w) = c {
                c = w.next_sibling();
                if let Some(b) = find(&w) {
                    return Some(b);
                }
            }
            None
        }
        if let Some(b) = find(self.toolbar.upcast_ref()) {
            b.emit_clicked();
        }
        if self.fullscreen.get() {
            if let Some(w) = self
                .root
                .root()
                .and_then(|w| w.downcast::<gtk::Window>().ok())
            {
                w.fullscreen();
            }
        }
    }
    pub fn fixture_window(&self, w: i32, h: i32) {
        if let Some(window) = self
            .root
            .root()
            .and_then(|w| w.downcast::<gtk::Window>().ok())
        {
            window.set_default_size(w, h);
        }
    }
    pub fn set_display(&self, mode: &str, zoom: f64, width: u16, height: u16) {
        self.mode.set(match mode {
            "native" => Scale::Native,
            "stretch" => Scale::Stretch,
            "dynamic" => Scale::Dynamic,
            _ => Scale::Fit,
        });
        self.zoom.set(zoom.clamp(0.25, 4.0));
        self.request_preferences_save();
        self.canvas();
        if width > 0 && height > 0 {
            self.command(Command::Resize(
                (width.clamp(200, 4096) / 2) * 2,
                height.clamp(200, 4096),
                100,
            ));
        }
    }
    pub fn fixture_pointer(&self, q: &serde_json::Value) -> serde_json::Value {
        let position_before = self.pointer_position.get();
        // Scope the owned fixture's disabled-route probe to this callback;
        // no asynchronous profile watcher observes synthetic metadata.
        let saved_primary_paste = if q["disable_primary_paste"] == true {
            self.profile_metadata
                .borrow_mut()
                .as_mut()
                .map(|p| std::mem::replace(&mut p.preferences.primary_paste, false))
        } else {
            None
        };
        if q["cancel"] == true {
            let controllers = self.area.observe_controllers();
            for i in 0..controllers.n_items() {
                if let Some(click) = controllers.item(i).and_downcast::<gtk::GestureClick>() {
                    click.emit_by_name::<()>("cancel", &[&None::<gdk::EventSequence>]);
                }
            }
        } else if let (Some(x), Some(y)) = (q["x"].as_f64(), q["y"].as_f64()) {
            // The owned fixture supplies canvas coordinates, as GTK does.
            self.pointer(
                x,
                y,
                q["button"].as_u64().unwrap_or(0) as u32,
                q["release"] == true,
            );
        }
        if let (Some(x), Some(y)) = (q["pan_x"].as_f64(), q["pan_y"].as_f64()) {
            self.scroll.hadjustment().set_value(x);
            self.scroll.vadjustment().set_value(y);
        }
        if let Some(saved) = saved_primary_paste {
            if let Some(p) = self.profile_metadata.borrow_mut().as_mut() {
                p.preferences.primary_paste = saved;
            }
        }
        serde_json::json!({"held_buttons":self.buttons.borrow().len(),"middle_presses":self.native_middle_presses.get(),"position_before":position_before,"position":self.pointer_position.get(),"state":self.fixture_inspect()})
    }
    pub fn set_profile(&self, host: &str, user: &str, reference: &str, ca: &str) {
        self.cancel_pending_connect("Connection cancelled: settings changed");
        if self.entries[0].text() != host {
            self.last_size.set(None);
        }
        for (e, value) in self.entries.iter().zip([host, user, reference, ca]) {
            e.set_text(value);
        }
    }
    pub fn pause_keyboard(&self) {
        self.release_keys();
    }
    pub fn inactive(&self) {
        self.cancel_primary_click();
        if !self.auto_gdm.borrow().armed {
            self.assist_epoch.fetch_add(1, Ordering::AcqRel);
        }
        self.bridge.borrow().epoch.store(0, Ordering::Release);
        self.release_keys();
    }
    fn command(&self, c: Command) {
        if let Some(conn) = self.connection.borrow().as_ref() {
            if let Err(e) = conn.queue(c) {
                self.status.set_text(&e);
            }
        }
    }
    /// Deliver deferred commands; a connection that accepts nothing for 30
    /// seconds is closed so Reconnect can start a fresh one.
    fn flush_commands(&self) {
        if self
            .pending
            .borrow()
            .as_ref()
            .is_some_and(|r| r.created.elapsed() > Duration::from_secs(10))
        {
            // DataResponse also has no request ID: reset the transport rather
            // than allowing its late reply to satisfy a later request.
            self.lazy_reply.borrow_mut().take();
            if let Some(c) = self.connection.borrow().as_ref() {
                c.close();
            }
        }
        self.flush_offer();
        if self
            .offers
            .borrow()
            .inflight
            .as_ref()
            .is_some_and(|o| o.created.elapsed() > Duration::from_secs(10))
        {
            self.offers.borrow_mut().waiting.take();
            if let Some(conn) = self.connection.borrow().as_ref() {
                conn.close();
            }
            self.status
                .set_text("Remote clipboard response timed out; reconnect to reset the channel");
        }
        if let Some(conn) = self.connection.borrow().as_ref() {
            if conn.flush() {
                self.status
                    .set_text("RDP connection stopped responding for 30 seconds; reconnect");
                conn.close();
            }
        }
    }
    fn primary_tick(&self) {
        let enabled = self
            .profile_metadata
            .borrow()
            .as_ref()
            .is_some_and(|p| p.preferences.primary_sync);
        self.primary_status
            .set_visible(enabled || self.primary_error.borrow().is_some());
        if !enabled {
            self.primary_authorize.set_visible(false);
            self.primary.borrow_mut().take();
            return;
        }
        let transport = self.connection.borrow().as_ref().map(|c| c.id.clone());
        if *self.primary_transport.borrow() != transport {
            self.primary.borrow_mut().take();
            self.primary_click.borrow_mut().take();
            self.primary_error.borrow_mut().take();
            self.primary_attempted.set(false);
            *self.primary_transport.borrow_mut() = transport;
        }
        let login = self
            .profile_metadata
            .borrow()
            .as_ref()
            .is_some_and(|p| p.preferences.rdp_mode == "gnome-remote-login");
        let connected = self.connection.borrow().as_ref().is_some_and(|c| {
            let s = c.state.lock().unwrap();
            matches!(s.phase.as_str(), "connected" | "display_change")
                && s.frames > 0
                && (!login || s.handovers >= 2)
        });
        if !connected {
            self.primary_authorize.set_visible(false);
            self.primary_click.borrow_mut().take();
            if self.primary.borrow_mut().take().is_some() {
                self.primary_attempted.set(false);
            }
            self.primary_status
                .set_text("Mouse selection sharing waits for the remote desktop");
            return;
        }
        if self
            .primary_checked
            .get()
            .is_none_or(|at| at.elapsed() > Duration::from_secs(1))
        {
            self.primary_checked.set(Some(std::time::Instant::now()));
            let current = self
                .profile_metadata
                .borrow()
                .as_ref()
                .is_some_and(crate::profiles::primary_profile_current);
            if !current {
                self.primary_authorize.set_visible(false);
                self.primary_click.borrow_mut().take();
                self.primary.borrow_mut().take();
                self.primary_attempted.set(true);
                self.primary_status.set_text(
                    "Mouse selection settings changed; reconnect this desktop to apply them",
                );
                return;
            }
        }
        let active = self.area.has_focus()
            && self
                .root
                .root()
                .and_then(|r| r.downcast::<gtk::Window>().ok())
                .is_some_and(|w| w.is_active());
        if self.primary.borrow().is_none() && !self.primary_attempted.get() && active {
            self.primary_attempted.set(true);
            if let Some(p) = self.profile_metadata.borrow().as_ref() {
                match crate::primary::Bridge::start(p) {
                    Ok(b) => *self.primary.borrow_mut() = Some(b),
                    Err(e) => self.primary_status.set_text(&e),
                }
            }
        }
        if let Some(bridge) = self.primary.borrow_mut().as_mut() {
            bridge.tick(
                active && self.allowed(false, true),
                active && self.allowed(true, false),
            );
            self.primary_authorize.set_visible(
                bridge.failed || (bridge.native() && bridge.ready && !bridge.authorized()),
            );
            self.primary_authorize.set_label(if bridge.failed {
                "Retry selection connection"
            } else {
                "Authorize selection connection…"
            });
            self.primary_authorize
                .set_sensitive(!bridge.authorizing() && !self.auto_gdm.borrow().armed);
            if active && self.allowed(true, false) && self.primary_click.borrow().is_none() {
                if let Err(error) = bridge.publish_primary(&self.area.display().primary_clipboard())
                {
                    self.primary_status.set_text(&error);
                }
            }

            if let Some(error) = self.primary_error.borrow().as_ref() {
                self.primary_status.set_text(error);
            } else {
                self.primary_status.set_text(if bridge.failed {"Selection sharing stopped. Check the SSH account, trusted host and configured desktop, then retry the selection connection"} else if bridge.native() && bridge.authorizing() {"Approve or deny this connection on the remote desktop; no paste is waiting"} else if bridge.native() && !bridge.authorized() {"Native selection delivery needs account permission and explicit connection authorization"} else if bridge.native() && bridge.authorized() && !bridge.observes() && self.allowed(true, false) {"Selection sending ready; receiving remote selections needs this desktop’s Xwayland bridge"} else if bridge.ready {"Mouse selection sharing ready · Ctrl+C clipboard remains separate"} else {"Connecting mouse selection sharing over SSH…"});
            }
        } else {
            self.primary_authorize.set_visible(false);
        }
        self.finish_primary_click();
    }
    fn authorize_primary(&self) {
        if self.auto_gdm.borrow().armed
            || !self
                .selected
                .borrow()
                .as_ref()
                .is_some_and(|selected| selected())
            || !self
                .root
                .root()
                .and_downcast::<gtk::Window>()
                .is_some_and(|w| w.is_active())
            || !self
                .profile_metadata
                .borrow()
                .as_ref()
                .is_some_and(crate::profiles::primary_profile_current)
        {
            self.primary_status.set_text(
                "Authorization requires the current connected desktop and its send policy",
            );
            return;
        }
        self.cancel_primary_click();
        // Opening the controls intentionally relinquishes desktop ownership.
        // This explicit action restores the selected view before evaluating its
        // send policy; it never reuses an earlier paste or approval transaction.
        if let Some(popover) = self
            .primary_authorize
            .ancestor(gtk::Popover::static_type())
            .and_downcast::<gtk::Popover>()
        {
            popover.popdown();
        }
        self.focus();
        if !self.area.has_focus() || !self.allowed(false, true) {
            self.primary_status.set_text(
                "Authorization requires the current connected desktop and its send policy",
            );
            return;
        }
        self.primary_tick();
        if self
            .primary
            .borrow()
            .as_ref()
            .is_some_and(|bridge| bridge.failed)
        {
            self.primary.borrow_mut().take();
            self.primary_attempted.set(false);
            self.primary_error.borrow_mut().take();
            self.primary_tick();
            return;
        }
        let active = self.area.has_focus()
            && self
                .root
                .root()
                .and_downcast::<gtk::Window>()
                .is_some_and(|w| w.is_active());
        if !active {
            return;
        }
        let result = self
            .primary
            .borrow_mut()
            .as_mut()
            .ok_or("Selection connection is unavailable".to_string())
            .and_then(|b| b.authorize());
        match result {
            Ok(()) => {
                self.primary_error.borrow_mut().take();
            }
            Err(error) => {
                *self.primary_error.borrow_mut() = Some(error);
            }
        }
    }
    fn cancel_primary_click(&self) {
        if let Some(pending) = self.primary_click.borrow_mut().take() {
            if let Some(bridge) = self.primary.borrow_mut().as_mut() {
                bridge.cancel_primary(pending.id);
            }
        }
    }
    fn finish_primary_click(&self) {
        let Some(pending) = self.primary_click.borrow().clone() else {
            return;
        };
        let active = self.area.has_focus()
            && self
                .root
                .root()
                .and_then(|r| r.downcast::<gtk::Window>().ok())
                .is_some_and(|w| w.is_active());
        let mut valid = active
            && self.allowed(false, true)
            && self.broker.borrow().epoch() == pending.epoch
            && self.connection.borrow().as_ref().is_some_and(|c| {
                let state = c.state.lock().unwrap();
                c.id == pending.transport
                    && state.generation == pending.generation
                    && !state.reactivating
                    && matches!(state.phase.as_str(), "connected" | "display_change")
            })
            && self.geometry(self.area.width() as f64, self.area.height() as f64)
                == pending.geometry
            && self.area.display().primary_clipboard().content().as_ref() == Some(&pending.source);
        valid &= self.profile_metadata.borrow().as_ref().is_some_and(|p| {
            p.preferences.primary_paste && crate::profiles::primary_profile_current(p)
        });
        let answer = self
            .primary
            .borrow_mut()
            .as_mut()
            .and_then(|b| match pending.stage {
                PrimaryStage::Preparing => b.take_prepare_result(pending.id),
                PrimaryStage::Committing | PrimaryStage::LegacyOffer => {
                    b.take_offer_result(pending.id)
                }
                PrimaryStage::Confirming => b.take_confirm_result(pending.id),
            });
        if matches!(
            pending.stage,
            PrimaryStage::Preparing | PrimaryStage::Committing
        ) && valid
            && answer == Some(true)
            && pending.started.elapsed() <= Duration::from_secs(4)
        {
            let advanced =
                self.primary
                    .borrow_mut()
                    .as_mut()
                    .is_some_and(|b| match pending.stage {
                        PrimaryStage::Preparing => b.commit_primary(pending.id).is_ok(),
                        PrimaryStage::Committing => b.confirm_primary(pending.id).is_ok(),
                        _ => false,
                    });
            if advanced {
                if let Some(current) = self.primary_click.borrow_mut().as_mut() {
                    current.stage = if pending.stage == PrimaryStage::Preparing {
                        PrimaryStage::Committing
                    } else {
                        PrimaryStage::Confirming
                    };
                }
                return;
            }
            valid = false;
        }
        let failed = self.primary.borrow().as_ref().is_none_or(|b| b.failed);
        if !valid
            || failed
            || pending.started.elapsed() > Duration::from_secs(4)
            || answer.is_some()
        {
            self.cancel_primary_click();
            if valid
                && !failed
                && matches!(
                    pending.stage,
                    PrimaryStage::Confirming | PrimaryStage::LegacyOffer
                )
                && pending.started.elapsed() <= Duration::from_secs(4)
                && answer == Some(true)
            {
                self.suppress_middle_release.set(false);
                self.pointer_native(pending.x, pending.y, 2, false);
                if pending.released {
                    self.pointer_native(pending.x, pending.y, 2, true);
                }
                self.primary_error.borrow_mut().take();
            } else {
                *self.primary_error.borrow_mut()=Some(if !valid {"Selection paste cancelled: source, focus, endpoint or display changed"} else if pending.stage == PrimaryStage::Confirming {"Selection paste cancelled: remote selection changed or confirmation expired. Select the text again"} else {"Selection paste was not confirmed. Focus the remote RUWAC window, enable its mouse-selection reception, and check that both GUIs are updated"}.into());
            }
        }
    }
    fn allowed(&self, receive: bool, manual: bool) -> bool {
        self.broker.borrow().desktop_enabled()
            && self.broker.borrow().active() == Some(self.id.as_str())
            && match self.policy.get() {
                ClipPolicy::Disabled => false,
                ClipPolicy::Manual => manual,
                ClipPolicy::LocalToRemote => !receive,
                ClipPolicy::RemoteToLocal => receive,
                ClipPolicy::Bidirectional => true,
            }
    }
    fn advertise(&self, manual: bool) {
        self.advertise_intent(manual, false);
    }
    fn advertise_intent(&self, manual: bool, intent: bool) {
        if !self.allowed(false, manual) {
            return;
        }
        let clip = self.area.display().clipboard();
        if self
            .provider
            .borrow()
            .as_ref()
            .is_some_and(|p| clip.content().as_ref() == Some(p))
        {
            if manual {
                self.status.set_text("Clipboard still belongs to this remote session; copy new local content before sending");
            }
            return;
        }
        let f = clip.formats();
        if std::env::var_os("RUWAC_RDP_CLIP_TRACE").as_deref() == Some(std::ffi::OsStr::new("1")) {
            eprintln!(
                "{}",
                serde_json::json!({"clip_trace":"advertise","manual":manual,"self_provider":self.provider.borrow().as_ref().is_some_and(|p|clip.content().as_ref()==Some(p)),"local":clip.is_local(),"png":f.contain_mime_type("image/png"),"epoch":self.broker.borrow().epoch()})
            );
        }
        let formats = crate::cliprdr::formats(
            f.contain_mime_type("text/plain;charset=utf-8") || f.contain_mime_type("text/plain"),
            f.contain_mime_type("image/png"),
        );
        if formats.is_empty() {
            return;
        }
        self.enqueue_offer(formats, intent);
    }
    fn offer_valid(&self, offer: &LocalOffer) -> bool {
        offer.serial == self.serial.get()
            && offer.epoch == self.broker.borrow().epoch()
            && offer.source == self.area.display().clipboard().content()
            && offer.created.elapsed() < Duration::from_secs(10)
            && self.allowed(false, true)
            && self.connection.borrow().as_ref().is_some_and(|c| {
                let state = c.state.lock().unwrap();
                c.id == offer.transport
                    && state.generation == offer.generation
                    && !state.reactivating
                    && !c.stop.load(Ordering::Acquire)
                    && matches!(state.phase.as_str(), "connected" | "display_change")
            })
    }
    fn enqueue_offer(&self, formats: Vec<ClipboardFormat>, intent: bool) {
        let Some((transport, generation)) = self
            .connection
            .borrow()
            .as_ref()
            .map(|c| (c.id.clone(), c.state.lock().unwrap().generation))
        else {
            return;
        };
        self.offers.borrow_mut().submit(LocalOffer {
            formats,
            epoch: self.broker.borrow().epoch(),
            serial: self.serial.get(),
            source: self.area.display().clipboard().content(),
            transport,
            generation,
            intent,
            created: std::time::Instant::now(),
        });
        self.flush_offer();
    }
    fn flush_offer(&self) {
        if self.offers.borrow().inflight.is_some() {
            return;
        }
        let Some(mut offer) = self.offers.borrow_mut().ready() else {
            return;
        };
        // Initialization carries no input intent and must also consume its ACK.
        if !offer.formats.is_empty() && !self.offer_valid(&offer) {
            return;
        }
        let Some(conn) = self
            .connection
            .borrow()
            .as_ref()
            .map(|c| c.queue(Command::Copy(std::mem::take(&mut offer.formats))))
        else {
            return;
        };
        match conn {
            Ok(()) => {
                self.advertised.set(offer.epoch);
                self.offers.borrow_mut().sent(offer);
            }
            Err(e) => self.status.set_text(&e),
        }
    }
    fn paste(&self) {
        if self.auto_gdm.borrow().armed {
            return;
        }
        if self.allowed(false, true) {
            self.release_keys();
            self.advertise_intent(true, true);
        } else {
            self.status
                .set_text("Clipboard policy prevents local paste");
        }
    }
    // Serving another MIME from an already-owned offer is not a new clipboard
    // promotion. Consumers in other windows may read it after focus changes.
    fn owns_remote_provider(&self, provider: &gdk::ContentProvider) -> bool {
        self.broker.borrow().desktop_enabled()
            && matches!(
                self.policy.get(),
                ClipPolicy::Manual | ClipPolicy::RemoteToLocal | ClipPolicy::Bidirectional
            )
            && self.provider.borrow().as_ref() == Some(provider)
            && self.area.display().clipboard().content().as_ref() == Some(provider)
    }
    fn clipboard_request(
        &self,
        kind: Format,
        epoch: u64,
        generation: u64,
    ) -> Option<ClipboardRequest> {
        let binding = self.connection.borrow();
        let c = binding.as_ref()?;
        let state = c.state.lock().unwrap();
        if c.stop.load(Ordering::Acquire)
            || state.reactivating
            || !matches!(state.phase.as_str(), "connected" | "display_change")
        {
            return None;
        }
        let id = self.request_serial.get().checked_add(1)?;
        self.request_serial.set(id);
        Some(ClipboardRequest {
            id,
            kind,
            epoch,
            generation,
            created: std::time::Instant::now(),
            transport: c.id.clone(),
            serial: self.serial.get(),
            transport_generation: state.generation,
            provider: None,
        })
    }
    fn receive(&self, manual: bool) {
        if !self.allowed(true, manual) || self.pending.borrow().is_some() {
            return;
        }
        let Some((id, kind, generation, offered_epoch)) = self.remote.get() else {
            return;
        };
        // A repeated Receive after automatic delivery must not re-read the same
        // remote generation and reclaim a newer local selection asynchronously.
        if self.received_generation.get() == generation {
            return;
        }
        let epoch = self.broker.borrow().epoch();
        if !manual && offered_epoch != epoch {
            return;
        }
        let Some(request) = self.clipboard_request(kind, epoch, generation) else {
            return;
        };
        *self.pending.borrow_mut() = Some(request);
        self.command(Command::Request(id));
    }
    fn clip_event(self: &Rc<Self>, e: Event) {
        if std::env::var_os("RUWAC_RDP_CLIP_TRACE").as_deref() == Some(std::ffi::OsStr::new("1")) {
            let event = match &e {
                Event::Reset => "reset",
                Event::Initialize => "initialize",
                Event::Ready => "ready",
                Event::Offer { .. } => "offer",
                Event::Request(_) => "request",
                Event::Data(_) => "data",
                Event::Accepted(_) => "accepted",
            };
            let detail = match &e {
                Event::Offer {
                    generation,
                    epoch,
                    selected,
                    ..
                } => {
                    serde_json::json!({"generation":generation,"epoch":epoch,"kind":selected.map(|(_,kind)|format!("{kind:?}"))})
                }
                Event::Request(id) => serde_json::json!({"format_id":id.0}),
                Event::Accepted(ok) => serde_json::json!({"accepted":ok}),
                _ => serde_json::json!({}),
            };
            eprintln!(
                "{}",
                serde_json::json!({"clip_trace":event,"epoch":self.broker.borrow().epoch(),"advertised":self.advertised.get(),"replying":self.replying.get(),"pending":self.pending.borrow().is_some(),"detail":detail})
            );
        }

        match e {
            Event::Reset => {
                self.offers.borrow_mut().acknowledge();
                self.offers.borrow_mut().waiting.take();
                self.serial.set(self.serial.get().saturating_add(1));
                let active = self.broker.borrow().active() == Some(self.id.as_str());
                self.broker.borrow_mut().disconnect(&self.id);
                self.pending.borrow_mut().take();
                self.lazy_reply.borrow_mut().take();
                self.remote_formats.borrow_mut().clear();
                self.remote.set(None);
                self.received_generation.set(0);
                self.advertised.set(0);
                self.requested.set((0, 0));
                self.replying.set(false);
                self.decoding.set(false);
                self.release_keys();
                self.surface.borrow_mut().take();
                self.size.set((0, 0));
                if active {
                    self.focus();
                }
                self.area.queue_draw();
            }
            Event::Initialize => self.enqueue_offer(Vec::new(), false),
            Event::Ready => self.advertise(false),
            Event::Accepted(ok) => {
                let offer = self.offers.borrow_mut().acknowledge();
                if offer
                    .as_ref()
                    .is_some_and(|o| o.intent && self.offer_valid(o))
                    && ok
                {
                    self.send(vec![
                        FastPathInputEvent::KeyboardEvent(KeyboardFlags::empty(), 0x1d),
                        FastPathInputEvent::KeyboardEvent(KeyboardFlags::empty(), 0x2f),
                        FastPathInputEvent::KeyboardEvent(KeyboardFlags::RELEASE, 0x2f),
                        FastPathInputEvent::KeyboardEvent(KeyboardFlags::RELEASE, 0x1d),
                    ]);
                }
                if !ok {
                    self.status
                        .set_text("Remote clipboard rejected format list");
                }
            }
            Event::Offer {
                generation,
                epoch,
                selected,
                formats,
            } => {
                *self.remote_formats.borrow_mut() = formats;
                self.offers_seen
                    .set(self.offers_seen.get().saturating_add(1));
                self.remote
                    .set(selected.map(|(id, k)| (id, k, generation, epoch)));
                if let Some((_, kind)) = selected {
                    let decision = self.broker.borrow_mut().offer(
                        RemoteOffer {
                            view: self.id.clone(),
                            mime: kind.mime().into(),
                            generation,
                            owner_focus_epoch: epoch,
                            size: 0,
                            scope: ClipScope::Clipboard,
                        },
                        self.policy.get(),
                    );
                    if decision == OfferDecision::Eligible {
                        self.receive(false);
                    }
                }
            }
            Event::Request(id) => {
                let kind = if id == Format::Text.id() {
                    Some(Format::Text)
                } else if id == Format::Png.id() || id == crate::cliprdr::MIME_PNG_ID {
                    Some(Format::Png)
                } else if id == Format::Dib.id() {
                    Some(Format::Dib)
                } else if id == Format::DibV5.id() {
                    Some(Format::DibV5)
                } else {
                    None
                };
                let epoch = self.broker.borrow().epoch();
                if kind.is_none()
                    || !self.allowed(false, true)
                    || epoch != self.advertised.get()
                    || self.replying.replace(true)
                {
                    self.command(Command::Reply(None));
                    return;
                }
                let kind = kind.unwrap();
                let serial = self.serial.get();
                let w = Rc::downgrade(self);
                let clip = self.area.display().clipboard();
                let broker = self.broker.clone();
                crate::clipboard::read_bytes(&clip, broker, epoch, kind.mime(), move |result| {
                    let Some(v) = w.upgrade() else {
                        return;
                    };
                    if v.serial.get() != serial {
                        return;
                    }
                    let w = Rc::downgrade(&v);
                    let (tx, rx) = std::sync::mpsc::sync_channel(1);
                    std::thread::spawn(move || {
                        let _ = tx.send(result.and_then(|b| crate::cliprdr::encode(kind, &b)));
                    });
                    glib::timeout_add_local(Duration::from_millis(10), move || {
                        match rx.try_recv() {
                            Ok(r) => {
                                if let Some(v) = w.upgrade() {
                                    if v.serial.get() != serial {
                                        return glib::ControlFlow::Break;
                                    }
                                    v.replying.set(false);
                                    let valid = v.allowed(false, true)
                                        && v.broker.borrow().epoch() == epoch
                                        && v.advertised.get() == epoch;
                                    if std::env::var_os("RUWAC_RDP_CLIP_TRACE").as_deref()
                                        == Some(std::ffi::OsStr::new("1"))
                                    {
                                        eprintln!(
                                            "{}",
                                            serde_json::json!({"clip_trace":"reply","valid":valid,"success":r.is_ok(),"bytes":r.as_ref().ok().map(Vec::len),"reason":r.as_ref().err(),"epoch":v.broker.borrow().epoch(),"expected_epoch":epoch})
                                        );
                                    }
                                    v.command(Command::Reply(if valid { r.ok() } else { None }));
                                }
                                glib::ControlFlow::Break
                            }
                            Err(std::sync::mpsc::TryRecvError::Empty) => {
                                glib::ControlFlow::Continue
                            }
                            Err(_) => glib::ControlFlow::Break,
                        }
                    });
                });
            }
            Event::Data(data) => {
                let Some(request) = self.pending.borrow().clone() else {
                    return;
                };
                let (kind, epoch, generation) = (request.kind, request.epoch, request.generation);
                if self.decoding.replace(true) {
                    return;
                }
                let serial = request.serial;
                let (tx, rx) = std::sync::mpsc::sync_channel(1);
                std::thread::spawn(move || {
                    let _ = tx.send(data.and_then(|b| crate::cliprdr::decode(kind, &b)));
                });
                let w = Rc::downgrade(self);
                glib::timeout_add_local(Duration::from_millis(10), move || match rx.try_recv() {
                    Ok(r) => {
                        if let Some(v) = w.upgrade() {
                            let current = v.pending.borrow().as_ref() == Some(&request);
                            let transport_current = v.serial.get() == request.serial
                                && v.connection.borrow().as_ref().is_some_and(|c| {
                                    let state = c.state.lock().unwrap();
                                    c.id == request.transport
                                        && state.generation == request.transport_generation
                                        && !c.stop.load(Ordering::Acquire)
                                        && !state.reactivating
                                });
                            if current {
                                v.pending.borrow_mut().take();
                                v.decoding.set(false);
                            }
                            let delivery_allowed = if let Some(provider) = request.provider.as_ref()
                            {
                                v.owns_remote_provider(provider)
                                    && v.received_generation.get() == generation
                            } else {
                                v.allowed(true, true) && v.broker.borrow().epoch() == epoch
                            };
                            if current
                                && transport_current
                                && delivery_allowed
                                && v.remote.get().is_some_and(|o| o.2 == generation)
                            {
                                if let Some(reply) = v.lazy_reply.borrow_mut().take() {
                                    let _ = reply.send(r.map(glib::Bytes::from_owned));
                                    return glib::ControlFlow::Break;
                                }
                                match r {
                                    Ok(b) => {
                                        let mime = if kind == Format::Text {
                                            "text/plain;charset=utf-8"
                                        } else {
                                            kind.mime()
                                        };
                                        let mimes = v
                                            .remote_formats
                                            .borrow()
                                            .iter()
                                            .map(|(_, k)| {
                                                if *k == Format::Text {
                                                    "text/plain;charset=utf-8"
                                                } else {
                                                    "image/png"
                                                }
                                            })
                                            .collect::<Vec<_>>();
                                        let clip = v.area.display().clipboard();
                                        let current = v.provider.borrow().as_ref()
                                            .filter(|p| clip.content().as_ref() == Some(p))
                                            .and_then(|p| p.clone().downcast::<crate::remote_content::RemoteContent>().ok())
                                            .filter(|p| p.mime() == Some(mime) && p.supports_formats(&mimes));
                                        let bytes = glib::Bytes::from_owned(b);
                                        let reused = current.is_some();
                                        let provider: gdk::ContentProvider =
                                            if let Some(p) = current {
                                                p.replace_bytes(bytes.clone());
                                                v.broker.borrow_mut().local_copy();
                                                v.bridge.borrow().epoch.store(
                                                    v.broker.borrow().epoch(),
                                                    Ordering::Release,
                                                );
                                                p.upcast()
                                            } else {
                                                crate::remote_content::RemoteContent::new(
                                                    mime,
                                                    bytes.clone(),
                                                )
                                                .upcast()
                                            };
                                        if let Ok(remote) = provider
                                            .clone()
                                            .downcast::<crate::remote_content::RemoteContent>(
                                        ) {
                                            let mimes = v
                                                .remote_formats
                                                .borrow()
                                                .iter()
                                                .map(|(_, k)| {
                                                    if *k == Format::Text {
                                                        "text/plain;charset=utf-8"
                                                    } else {
                                                        "image/png"
                                                    }
                                                })
                                                .collect::<Vec<_>>();
                                            let weak = Rc::downgrade(&v);
                                            let content = remote.downgrade();
                                            remote.offer(&mimes,mime,bytes.clone(),Rc::new(move |requested,reply| {
                                                let Some(v)=weak.upgrade() else {return;};
                                                let Some((id,kind))=v.remote_formats.borrow().iter().find(|(_,k)| k.mime()==requested || (*k==Format::Text && requested=="text/plain;charset=utf-8")).copied() else {return;};
                                                let Some(provider)=content.upgrade().map(|p|p.upcast::<gdk::ContentProvider>()) else{return;};
                                                if v.serial.get()!=serial || v.received_generation.get()!=generation || !v.owns_remote_provider(&provider)
                                                    || v.pending.borrow().is_some() {return;}
                                                let Some(mut request)=v.clipboard_request(kind,v.broker.borrow().epoch(),generation) else{return;};
                                                request.provider=Some(provider);
                                                *v.lazy_reply.borrow_mut()=Some(reply);
                                                *v.pending.borrow_mut()=Some(request);
                                                v.command(Command::Request(id));
                                            }));
                                        }
                                        let previous = v.provider.replace(Some(provider.clone()));
                                        if reused
                                            || v.area
                                                .display()
                                                .clipboard()
                                                .set_content(Some(&provider))
                                                .is_ok()
                                        {
                                            v.received_generation.set(generation);
                                            v.status.set_text(
                                                "Remote clipboard received through CLIPRDR",
                                            );
                                        } else {
                                            v.provider.replace(previous);
                                            v.status.set_text(
                                                "Desktop refused the remote clipboard offer",
                                            );
                                        }
                                    }
                                    Err(e) => v.status.set_text(&e),
                                }
                            }
                            if current {
                                v.lazy_reply.borrow_mut().take();
                            }
                            if v.remote.get().is_some_and(|o| o.2 != generation) {
                                // An offer received during this read replaced its
                                // generation. Request only that newest eligible
                                // offer; receive rechecks focus/ownership/policy.
                                v.receive(false);
                            }
                        }
                        glib::ControlFlow::Break
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
                    Err(_) => glib::ControlFlow::Break,
                });
            }
        }
    }
    fn send(&self, e: Vec<FastPathInputEvent>) {
        if self.auto_gdm.borrow().armed {
            return;
        }
        self.assist_epoch.fetch_add(1, Ordering::AcqRel);
        if let Some(c) = self.connection.borrow().as_ref() {
            if let Err(e) = c.send(e) {
                self.status.set_text(&e);
            }
        }
    }
    fn key(&self, key: gdk::Key, code: u32, release: bool) {
        if !release {
            let held = self.held.borrow();
            let selecting = (held.contains(&(0x2a, false)) || held.contains(&(0x36, false)))
                && matches!(
                    key,
                    gdk::Key::Left
                        | gdk::Key::Right
                        | gdk::Key::Up
                        | gdk::Key::Down
                        | gdk::Key::Home
                        | gdk::Key::End
                        | gdk::Key::Page_Up
                        | gdk::Key::Page_Down
                );
            let all = (held.contains(&(0x1d, false)) || held.contains(&(0x1d, true)))
                && matches!(key, gdk::Key::a | gdk::Key::A);
            if selecting || all {
                crate::terminal_primary::forget();
            }
        }
        let meta = match key {
            gdk::Key::Super_L => Some(0x5b),
            gdk::Key::Super_R => Some(0x5c),
            _ => None,
        };
        if meta.is_some() {
            self.assist_epoch.fetch_add(1, Ordering::AcqRel);
        }
        let focused = self.area.has_focus()
            && self
                .root
                .root()
                .and_downcast::<gtk::Window>()
                .is_some_and(|w| w.is_active());
        let held = meta.is_some_and(|scan| self.held.borrow().contains(&(scan, true)));
        let plan = self
            .pending_super
            .borrow_mut()
            .key(meta, release, focused, held);
        if !plan.events.is_empty() {
            let mut events = Vec::new();
            for (scan, up) in plan.events {
                if !up {
                    self.held.borrow_mut().insert((scan, true));
                } else {
                    self.held.borrow_mut().remove(&(scan, true));
                }
                events.push(FastPathInputEvent::KeyboardEvent(
                    KeyboardFlags::EXTENDED
                        | if up {
                            KeyboardFlags::RELEASE
                        } else {
                            KeyboardFlags::empty()
                        },
                    scan,
                ));
            }
            self.send(events);
        }
        if plan.handled {
            return;
        }

        // GDK/XKB hardware codes are evdev+8, mostly set-1 for base keys.
        let special = match key {
            gdk::Key::Up => Some((0x48, true)),
            gdk::Key::Down => Some((0x50, true)),
            gdk::Key::Left => Some((0x4b, true)),
            gdk::Key::Right => Some((0x4d, true)),
            gdk::Key::Home => Some((0x47, true)),
            gdk::Key::End => Some((0x4f, true)),
            gdk::Key::Page_Up => Some((0x49, true)),
            gdk::Key::Page_Down => Some((0x51, true)),
            gdk::Key::Delete => Some((0x53, true)),
            gdk::Key::Insert => Some((0x52, true)),
            gdk::Key::Control_R => Some((0x1d, true)),
            gdk::Key::Alt_R | gdk::Key::ISO_Level3_Shift => Some((0x38, true)),
            gdk::Key::Super_L => Some((0x5b, true)),
            gdk::Key::Super_R => Some((0x5c, true)),
            gdk::Key::Print => Some((0x37, true)),
            _ => None,
        };
        if let Some((scan, extended)) = special.or_else(|| {
            if (9..=96).contains(&code) {
                Some(((code - 8) as u8, false))
            } else {
                None
            }
        }) {
            if release {
                if !self.held.borrow_mut().remove(&(scan, extended)) {
                    return;
                }
            } else {
                self.held.borrow_mut().insert((scan, extended));
            }
            self.send(vec![FastPathInputEvent::KeyboardEvent(
                (if release {
                    KeyboardFlags::RELEASE
                } else {
                    KeyboardFlags::empty()
                }) | if extended {
                    KeyboardFlags::EXTENDED
                } else {
                    KeyboardFlags::empty()
                },
                scan,
            )]);
        } else if let Some(ch) = key.to_unicode() {
            for unit in ch.to_string().encode_utf16() {
                self.send(vec![FastPathInputEvent::UnicodeKeyboardEvent(
                    if release {
                        KeyboardFlags::RELEASE
                    } else {
                        KeyboardFlags::empty()
                    },
                    unit,
                )]);
            }
        }
    }

    fn pointer(&self, x: f64, y: f64, button: u32, release: bool) {
        if button == 1 {
            self.selection_anchor
                .set(if release { None } else { Some((x, y)) });
        }
        if button == 0
            && self.buttons.borrow().contains(&1)
            && self
                .selection_anchor
                .get()
                .is_some_and(|(a, b)| (x - a).abs() + (y - b).abs() > 4.0)
        {
            crate::terminal_primary::forget();
        }
        if button == 2 && release && self.suppress_middle_release.replace(false) {
            if let Some(pending) = self.primary_click.borrow_mut().as_mut() {
                pending.released = true;
            }
            return;
        }
        if button == 2
            && !release
            && !self.auto_gdm.borrow().armed
            && self
                .profile_metadata
                .borrow()
                .as_ref()
                .is_some_and(|p| p.preferences.primary_sync)
        {
            let clipboard = self.area.display().primary_clipboard();
            if let Some(text) = crate::terminal_primary::selected(&clipboard) {
                if self
                    .primary
                    .borrow()
                    .as_ref()
                    .is_some_and(|bridge| bridge.selection_from_this_desktop(&clipboard))
                {
                    self.primary_click.borrow_mut().take();
                    self.primary_error.borrow_mut().take();
                    self.suppress_middle_release.set(false);
                    self.pointer_native(x, y, button, release);
                    return;
                }
                self.suppress_middle_release.set(true);
                if !self
                    .profile_metadata
                    .borrow()
                    .as_ref()
                    .is_some_and(|p| p.preferences.primary_paste)
                {
                    *self.primary_error.borrow_mut() = Some("Pasting local mouse selections is disabled for this connection. Enable selection paste in connection settings".into());
                    self.primary_status
                        .set_text(self.primary_error.borrow().as_deref().unwrap());
                    return;
                }
                if self.primary_click.borrow().is_some() {
                    return;
                }
                let geometry = self.geometry(self.area.width() as f64, self.area.height() as f64);
                let (px, py) = ((x - geometry.2) / geometry.0, (y - geometry.3) / geometry.1);
                let (rw, rh) = self.size.get();
                if !px.is_finite()
                    || !py.is_finite()
                    || px < 0.0
                    || py < 0.0
                    || px >= rw as f64
                    || py >= rh as f64
                {
                    return;
                }
                let active = self.area.has_focus()
                    && self
                        .root
                        .root()
                        .and_then(|r| r.downcast::<gtk::Window>().ok())
                        .is_some_and(|w| w.is_active());
                let route = self
                    .profile_metadata
                    .borrow()
                    .as_ref()
                    .is_some_and(crate::profiles::primary_profile_current);
                let transport = self
                    .connection
                    .borrow()
                    .as_ref()
                    .map(|c| (c.id.clone(), c.state.lock().unwrap().generation));
                if !active || !self.allowed(false, true) || !route || transport.is_none() {
                    *self.primary_error.borrow_mut()=Some("Selection paste requires the focused connected desktop and its send policy".into());
                    return;
                }
                self.pointer_native(x, y, 0, false);
                let offer = self
                    .primary
                    .borrow_mut()
                    .as_mut()
                    .ok_or_else(|| "Mouse selection sharing is not connected".into())
                    .and_then(|bridge| {
                        bridge.tick(
                            active && self.allowed(false, true),
                            active && self.allowed(true, false),
                        );
                        bridge.offer_primary(text)
                    });
                match offer {
                    Ok(id) => {
                        *self.primary_click.borrow_mut() = Some(PrimaryClick {
                            id,
                            x,
                            y,
                            released: false,
                            started: std::time::Instant::now(),
                            transport: transport.as_ref().unwrap().0.clone(),
                            generation: transport.as_ref().unwrap().1,
                            epoch: self.broker.borrow().epoch(),
                            geometry,
                            source: clipboard.content().unwrap(),
                            stage: if self.primary.borrow().as_ref().is_some_and(|b| b.native()) {
                                PrimaryStage::Preparing
                            } else {
                                PrimaryStage::LegacyOffer
                            },
                        });
                        self.primary_error.borrow_mut().take();
                    }
                    Err(error) => *self.primary_error.borrow_mut() = Some(error),
                }
                return;
            }
        }
        self.pointer_native(x, y, button, release);
    }
    fn pointer_native(&self, x: f64, y: f64, button: u32, release: bool) {
        if self.auto_gdm.borrow().armed {
            return;
        }
        let geometry = self.geometry(self.area.width() as f64, self.area.height() as f64);
        // A changed local zoom/allocation maps the same physical pointer to a
        // different remote position. End the old drag before sending that move.
        if self
            .pointer_geometry
            .replace(Some(geometry))
            .is_some_and(|old| old != geometry)
        {
            self.release_buttons();
        }
        let (sx, sy, ox, oy) = geometry;
        let (px, py) = ((x - ox) / sx, (y - oy) / sy);
        let (rw, rh) = self.size.get();
        let inside = px.is_finite()
            && py.is_finite()
            && px >= 0.0
            && py >= 0.0
            && px < rw as f64
            && py < rh as f64;
        // A release outside the image (letterboxing, toolbar or window edge)
        // must still end a drag. Keep its last valid remote position: converting
        // out-of-bounds coordinates can fling a held window to a desktop corner.
        let (x, y) = if inside {
            (px as u16, py as u16)
        } else if release && self.buttons.borrow().contains(&button) {
            self.pointer_position.get()
        } else {
            return;
        };
        if release && !self.buttons.borrow().contains(&button) {
            return;
        }
        self.pointer_position.set((x, y));
        if button != 0 {
            if release {
                self.buttons.borrow_mut().remove(&button);
            } else {
                self.buttons.borrow_mut().insert(button);
            }
        }
        let mut flags = match button {
            0 => PointerFlags::MOVE,
            1 => PointerFlags::LEFT_BUTTON,
            2 => PointerFlags::MIDDLE_BUTTON_OR_WHEEL,
            3 => PointerFlags::RIGHT_BUTTON,
            _ => return,
        };
        if button != 0 && !release {
            flags |= PointerFlags::DOWN;
        }
        if button == 2 && !release {
            self.native_middle_presses
                .set(self.native_middle_presses.get().saturating_add(1));
        }
        self.send(vec![FastPathInputEvent::MouseEvent(MousePdu {
            flags,
            number_of_wheel_rotation_units: 0,
            x_position: x,
            y_position: y,
        })]);
    }
    pub fn owns_focus(&self, widget: &gtk::Widget) -> bool {
        crate::tiles::contains(&self.area, widget)
    }
    pub fn activate_clipboard(&self) {
        let foreground = self
            .area
            .root()
            .and_then(|r| r.downcast::<gtk::Window>().ok())
            .is_some_and(|w| w.is_active());
        if !foreground {
            self.inactive();
            if self.broker.borrow().active() == Some(self.id.as_str()) {
                self.broker.borrow_mut().focus(None);
            }
            return;
        }

        let changed = self.broker.borrow().active() != Some(self.id.as_str());
        if changed {
            self.broker.borrow_mut().focus(Some(&self.id));
        }
        self.bridge
            .borrow()
            .epoch
            .store(self.broker.borrow().epoch(), Ordering::Release);
        if changed {
            // A Wayland selection can reach GDK before the window-active
            // notification. Re-advertise the current local selection on focus;
            // advertise still refuses this view's received remote provider.
            self.advertise(false);
        }
    }
    pub fn fixture_assist(self: &Rc<Self>, q: &serde_json::Value) -> serde_json::Value {
        if !self.profile_metadata.borrow().as_ref().is_some_and(|p| {
            crate::profiles::endpoint(&p.server, 3389).is_ok_and(|(h, _)| h == "127.0.0.1")
        }) {
            return serde_json::json!({"error":"Loopback profile required"});
        }
        if q["operation"] == "cancel-automatic" {
            let mut child = self.root.first_child();
            while let Some(w) = child {
                child = w.next_sibling();
                if w.widget_name() == "cancel-automatic-sign-in" {
                    if let Some(b) = w.downcast_ref::<gtk::Button>() {
                        b.emit_clicked();
                    }
                }
            }
        }
        if q["operation"] == "open" {
            if let Some(p) = self
                .assist_button
                .ancestor(gtk::Popover::static_type())
                .and_downcast::<gtk::Popover>()
            {
                p.popup();
            }
            self.assist_button.emit_clicked();
        }
        if q["operation"] == "stale-frame" {
            let previous = self.rendered_generation.replace(0);
            self.assist_dialog();
            self.rendered_generation.set(previous);
        }
        let mut shown = false;
        for window in gtk::Window::list_toplevels() {
            if let Ok(d) = window.downcast::<gtk::Dialog>() {
                if d.title().as_deref() == Some("Confirm GNOME login input") {
                    shown = true;
                    let mut child = d.content_area().first_child();
                    while let Some(c) = child {
                        child = c.next_sibling();
                        if c.widget_name() == "gnome-screen-confirmation" {
                            if let Some(c) = c.downcast_ref::<gtk::CheckButton>() {
                                c.set_active(q["confirmed"] == true);
                            }
                        }
                    }
                    if let Some(field) = q["field"].as_str() {
                        d.response(match field {
                            "username" => gtk::ResponseType::Other(1),
                            "password" => gtk::ResponseType::Other(2),
                            _ => gtk::ResponseType::Cancel,
                        });
                    }
                }
            }
        }
        serde_json::json!({"dialog_shown":shown,"status":self.status.text().as_str(),"action_visible":self.assist_button.property::<bool>("visible"),"action_grid_row":self.assist_button.parent().and_downcast::<gtk::Grid>().map(|g|g.query_child(&self.assist_button).1)})
    }
    pub fn bind_selection(&self, _selected: Rc<dyn Fn() -> bool>) {
        let root = self.root.downgrade();
        *self.selected.borrow_mut() = Some(Rc::new(move || {
            root.upgrade().is_some_and(|root| {
                root.ancestor(adw::TabView::static_type())
                    .and_downcast::<adw::TabView>()
                    .is_some_and(|tabs| {
                        tabs.selected_page()
                            .is_some_and(|page| crate::tiles::contains(&page.child(), &root))
                    })
            })
        }));
    }
    fn assist_ready(
        &self,
        p: &crate::profiles::Profile,
        transport: &str,
        generation: u64,
        focus: bool,
    ) -> bool {
        p.preferences.rdp_mode == "gnome-remote-login"
            && self.selected.borrow().as_ref().is_some_and(|f| f())
            && self.root.is_mapped()
            && self
                .root
                .root()
                .and_downcast::<gtk::Window>()
                .is_some_and(|w| w.is_active())
            && (!focus
                || (self.area.has_focus()
                    && self.broker.borrow().active() == Some(self.id.as_str())))
            && self.held.borrow().is_empty()
            && self.buttons.borrow().is_empty()
            && self.entries[0].text() == p.server
            && self.entries[1].text() == p.username
            && self.entries[2].text() == p.secret_reference
            && self.entries[3].text() == p.preferences.trust
            && crate::profiles::assist_profile_current(p)
            && self.connection.borrow().as_ref().is_some_and(|c| {
                let s = c.state.lock().unwrap();
                c.id == transport
                    && !c.stop.load(Ordering::Acquire)
                    && s.generation == generation
                    && self.rendered_generation.get() == generation
                    && self.size.get() != (0, 0)
                    && s.frames > 0
                    && !s.reactivating
                    && s.verified_tls_sha256.is_some()
                    && s.authentication_error.is_none()
            })
    }
    /// Automatic sign-in needs the verified connection and profile, not local
    /// focus: credentials go only over this tab's own pinned connection, and
    /// user input to it is paused while armed. So a background tab (several
    /// machines reconnecting at once) signs in without being selected.
    fn auto_ready(&self, p: &crate::profiles::Profile, transport: &str, generation: u64) -> bool {
        p.preferences.rdp_mode == "gnome-remote-login"
            && self.held.borrow().is_empty()
            && self.buttons.borrow().is_empty()
            && self.entries[0].text() == p.server
            && self.entries[1].text() == p.username
            && self.entries[2].text() == p.secret_reference
            && self.entries[3].text() == p.preferences.trust
            && crate::profiles::assist_profile_current(p)
            && self.connection.borrow().as_ref().is_some_and(|c| {
                let s = c.state.lock().unwrap();
                c.id == transport
                    && !c.stop.load(Ordering::Acquire)
                    && s.generation == generation
                    && self.surface_generation.get() == generation
                    && self.size.get() != (0, 0)
                    && s.frames > 0
                    && !s.reactivating
                    && s.verified_tls_sha256.is_some()
                    && s.authentication_error.is_none()
            })
    }
    /// After a stop, the same greeter (no later handover) may be retried.
    fn auto_gdm_retry_available(&self) -> bool {
        let a = self.auto_gdm.borrow();
        self.connection.borrow().as_ref().is_some_and(|c| {
            let st = c.state.lock().unwrap();
            a.retry_on_greeter(
                st.handovers,
                st.phase == "connected"
                    && st.frames > 0
                    && !st.reactivating
                    && st.verified_tls_sha256.is_some()
                    && st.authentication_error.is_none(),
            )
        }) && self
            .profile_metadata
            .borrow()
            .as_ref()
            .is_some_and(|p| auto_sign_in_missing(p).is_none())
    }
    fn recover_sign_in(self: &Rc<Self>) {
        let dialog = gtk::Dialog::builder()
            .title("Recover GNOME login")
            .modal(true)
            .build();
        if let Some(window) = self.root.root().and_downcast::<gtk::Window>() {
            dialog.set_transient_for(Some(&window));
        }
        let message = gtk::Label::new(Some(
            "If GNOME returned to its login screen, retry the saved sign-in or reconnect this connection. Manual login cancels automation and releases the keyboard. A rejected password must be corrected in Machines → GNOME User Login. Existing remote applications are preserved.",
        ));
        message.set_wrap(true);
        message.set_max_width_chars(64);
        message.set_margin_top(16);
        message.set_margin_bottom(16);
        message.set_margin_start(16);
        message.set_margin_end(16);
        dialog.content_area().append(&message);
        let retry = dialog.add_button("Retry saved sign-in", gtk::ResponseType::Other(1));
        retry.set_sensitive(self.auto_gdm_retry_available());
        dialog.add_button("Reconnect", gtk::ResponseType::Other(2));
        dialog.add_button("Manual login", gtk::ResponseType::Other(3));
        dialog.add_button("Cancel", gtk::ResponseType::Cancel);
        let weak = Rc::downgrade(self);
        dialog.connect_response(move |d, r| {
            d.close();
            if let Some(v) = weak.upgrade() {
                match r {
                    gtk::ResponseType::Other(1) => v.retry_auto_gdm(),
                    gtk::ResponseType::Other(2) => {
                        if let Err(e) = v.automation_reconnect() {
                            v.status.set_text(&e);
                        }
                    }
                    gtk::ResponseType::Other(3) => {
                        v.auto_gdm_stop("Manual GNOME login selected");
                        v.auto_gdm.borrow_mut().notice.take();
                        v.focus();
                    }
                    _ => (),
                }
            }
        });
        dialog.present();
    }
    fn retry_auto_gdm(&self) {
        if !self.auto_gdm_retry_available() {
            return;
        }
        self.assist_epoch.fetch_add(1, Ordering::AcqRel);
        let mut a = self.auto_gdm.borrow_mut();
        let image = a.image.take();
        *a = AutoGdm::default();
        a.armed = true;
        a.image = image;
        drop(a);
        self.status.set_text("Automatic sign-in · retrying");
        self.area.queue_draw();
    }
    /// Which guard holds automatic sign-in back, in words for the cover.
    fn assist_wait_reason(&self, p: &crate::profiles::Profile) -> &'static str {
        if !self.held.borrow().is_empty() || !self.buttons.borrow().is_empty() {
            "Waiting: release held keys and mouse buttons"
        } else if !crate::profiles::assist_profile_current(p)
            || self.entries[0].text() != p.server
            || self.entries[1].text() != p.username
            || self.entries[2].text() != p.secret_reference
            || self.entries[3].text() != p.preferences.trust
        {
            "Waiting: this machine's saved settings changed · reconnect"
        } else {
            "Waiting for the remote screen"
        }
    }
    fn auto_gdm_stop(&self, reason: &str) {
        let handovers = self
            .connection
            .borrow()
            .as_ref()
            .map(|c| c.state.lock().unwrap().handovers);
        let mut a = self.auto_gdm.borrow_mut();
        let was_armed = a.armed;
        a.sequence.stop();
        a.armed = false;
        a.completed = None;
        a.pending = None;
        a.fetching = false;
        a.waiting = None;
        if was_armed {
            a.notice = Some(reason.to_owned());
            a.stopped_at = handovers;
        }
        if a.stopped_at.is_none() {
            a.image = None;
        }
        self.assist_epoch.fetch_add(1, Ordering::AcqRel);
        self.status.set_text(reason);
        self.area.queue_draw();
    }
    fn auto_gdm_tick(self: &Rc<Self>) {
        use crate::gdm::{Action, Step};
        if !self.auto_gdm.borrow().armed && self.auto_gdm.borrow().completed.is_none() {
            return;
        }
        {
            let mut a = self.auto_gdm.borrow_mut();
            if a.tick_at.elapsed() < Duration::from_millis(100) {
                return;
            }
            a.tick_at = std::time::Instant::now();
        }
        let Some(p) = self.profile_metadata.borrow().clone() else {
            self.auto_gdm_stop("Automatic sign-in stopped: profile unavailable");
            return;
        };
        let Some((transport, generation, handovers, phase, frames, attempt)) =
            self.connection.borrow().as_ref().map(|c| {
                let st = c.state.lock().unwrap();
                (
                    c.id.clone(),
                    st.generation,
                    st.handovers,
                    st.phase.clone(),
                    st.frames,
                    st.connection_attempt,
                )
            })
        else {
            return;
        };
        if !self.auto_gdm.borrow().armed {
            let restart = phase == "connected"
                && frames > 0
                && self
                    .auto_gdm
                    .borrow()
                    .reconnects_to_greeter(&transport, generation, handovers)
                && auto_sign_in_missing(&p).is_none()
                && crate::profiles::assist_profile_current(&p);
            if !restart {
                return;
            }
            let mut a = self.auto_gdm.borrow_mut();
            *a = AutoGdm::default();
            a.armed = true;
            self.status
                .set_text("Automatic sign-in · reconnected GNOME login");
        }
        if phase == "error" || phase == "disconnected" {
            let service_auth_failed = handovers == 0
                && self
                    .connection
                    .borrow()
                    .as_ref()
                    .is_some_and(|c| c.state.lock().unwrap().authentication_error.is_some());
            let reason = if service_auth_failed {
                "RDP service authentication failed; verify RDP Service Login, separate from GNOME User Login".to_owned()
            } else if self.certificate.borrow().is_some() {
                "Review this server's certificate before automatic sign-in; use Review certificate"
                    .to_owned()
            } else {
                self.status.text().to_string()
            };
            self.auto_gdm_stop(&reason);
            return;
        }
        // Transport recovery is not time spent recognizing a login form. A
        // slow/offline server must not consume the sign-in budget before its
        // greeter has even produced a frame.
        if phase != "connected" || frames == 0 || handovers == 0 {
            self.auto_gdm.borrow_mut().began = std::time::Instant::now();
            return;
        }
        if self.auto_gdm.borrow().began.elapsed() > Duration::from_secs(90) {
            self.auto_gdm_stop(
                "Automatic sign-in stopped: supported GDM form or desktop not verified",
            );
            return;
        }
        // A second authenticated server handover after password submission must
        // also produce a new decoded/rendered desktop before reporting completion.
        if self.auto_gdm.borrow().sequence.step == Step::Submitted {
            let target_ok = self
                .auto_gdm
                .borrow()
                .binding
                .as_ref()
                .is_some_and(|(t, _, e)| {
                    t == &transport && *e == self.assist_epoch.load(Ordering::Acquire)
                })
                && crate::profiles::assist_profile_current(&p);
            if !target_ok {
                self.auto_gdm_stop(
                    "Automatic sign-in stopped: submitted credential target changed",
                );
                return;
            }
            let complete = {
                let a = self.auto_gdm.borrow();
                a.authenticated_desktop_handover(handovers, attempt)
                    && frames > 0
                    && phase == "connected"
                    && self.surface_generation.get() == generation
                    && self.size.get() != (0, 0)
            };
            if complete {
                let mut a = self.auto_gdm.borrow_mut();
                a.sequence.authenticated_handover();
                a.armed = false;
                a.completed = Some((transport.clone(), generation, handovers));
                a.image = None;
                a.pending = None;
                self.status.set_text("GNOME sign-in completed");
                self.area.queue_draw();
            } else if attempt > self.auto_gdm.borrow().submitted_attempt
                && !self
                    .auto_gdm
                    .borrow()
                    .authenticated_desktop_handover(handovers, attempt)
            {
                self.auto_gdm_stop("Connection changed after password submission; use Retry automatic sign-in on the new login screen");
            }
            return;
        }
        // The verified GNOME Remote Login path first redirects the service
        // connection to its greeter. Desktop Sharing must never trigger login
        // input merely because its desktop resembles a sign-in form.
        if handovers == 0 || self.auto_gdm.borrow().began.elapsed() < Duration::from_millis(1500) {
            return;
        }
        let epoch = self.assist_epoch.load(Ordering::Acquire);
        if !self.auto_ready(&p, &transport, generation) {
            if self.auto_gdm.borrow().binding.is_some() {
                self.auto_gdm_stop(
                    "Automatic sign-in stopped: selected tab, profile or focus changed",
                );
                return;
            }
            let reason = self.assist_wait_reason(&p);
            let mut a = self.auto_gdm.borrow_mut();
            if a.waiting != Some(reason) {
                a.waiting = Some(reason);
                drop(a);
                self.area.queue_draw();
            }
            return;
        }
        if self.auto_gdm.borrow_mut().waiting.take().is_some() {
            self.area.queue_draw();
        }
        let changed = self
            .auto_gdm
            .borrow()
            .binding
            .as_ref()
            .and_then(|(t, g, e)| {
                if t != &transport {
                    Some("Automatic sign-in stopped: connection changed")
                } else if *e != epoch {
                    Some("Automatic sign-in stopped: user input or focus changed")
                } else if *g != generation {
                    Some("Automatic sign-in stopped: remote display changed")
                } else {
                    None
                }
            });
        if let Some(reason) = changed {
            // A reconnect after a brief network loss shows a fresh greeter.
            // Before the password was submitted, start over on that screen.
            let restart = {
                let a = self.auto_gdm.borrow();
                a.binding
                    .as_ref()
                    .is_some_and(|(t, g, e)| t == &transport && *e == epoch && *g != generation)
                    && !a.fetching
                    && !a.password_queued
            };
            if restart {
                let mut a = self.auto_gdm.borrow_mut();
                a.sequence = Default::default();
                a.last_observation = None;
                a.binding = None;
                a.pending = None;
                a.scan_binding = None;
                a.image = None;
                a.began = std::time::Instant::now();
                return;
            }
            self.auto_gdm_stop(reason);
            return;
        }
        if self.auto_gdm.borrow().fetching {
            return;
        }
        let result = {
            let mut a = self.auto_gdm.borrow_mut();
            match a.pending.as_ref().map(|r| r.try_recv()) {
                Some(Ok(v)) => {
                    a.pending = None;
                    Some(v)
                }
                Some(Err(std::sync::mpsc::TryRecvError::Disconnected)) => {
                    a.pending = None;
                    Some(Err("Recognition worker stopped".into()))
                }
                _ => None,
            }
        };
        if let Some(result) = result {
            // Loading-screen observations may become obsolete during the initial
            // service-to-GDM handover. Discard them before the first input;
            // after input starts, the immutable binding above cancels changes.
            if self
                .auto_gdm
                .borrow()
                .scan_binding
                .as_ref()
                .is_none_or(|(t, g, e)| t != &transport || *g != generation || *e != epoch)
            {
                return;
            }
            let observation = match result {
                Ok(v) => v,
                Err(_) => {
                    self.auto_gdm_stop("Automatic sign-in stopped: local recognition unavailable");
                    return;
                }
            };
            let action = {
                let mut a = self.auto_gdm.borrow_mut();
                a.last_observation = Some(observation.kind);
                a.sequence.observe(&observation)
            };
            if matches!(
                action,
                Action::Click(..) | Action::Username | Action::Password
            ) {
                self.auto_gdm.borrow_mut().binding = Some((transport.clone(), generation, epoch));
            }
            match action {
                Action::Stop => {
                    self.auto_gdm_stop("Automatic sign-in stopped: ambiguous or unsupported field");
                    return;
                }
                Action::Click(x, y) => {
                    let events = vec![
                        FastPathInputEvent::MouseEvent(MousePdu {
                            flags: PointerFlags::MOVE,
                            x_position: x,
                            y_position: y,
                            number_of_wheel_rotation_units: 0,
                        }),
                        FastPathInputEvent::MouseEvent(MousePdu {
                            flags: PointerFlags::LEFT_BUTTON | PointerFlags::DOWN,
                            x_position: x,
                            y_position: y,
                            number_of_wheel_rotation_units: 0,
                        }),
                        FastPathInputEvent::MouseEvent(MousePdu {
                            flags: PointerFlags::LEFT_BUTTON,
                            x_position: x,
                            y_position: y,
                            number_of_wheel_rotation_units: 0,
                        }),
                    ];
                    self.send(events);
                    self.status
                        .set_text("Automatic sign-in · selecting saved Linux account");
                }
                Action::Username => {
                    self.command(Command::Assisted(
                        SensitiveText(p.preferences.gnome_username.clone()),
                        self.assist_epoch.clone(),
                        epoch,
                        generation,
                        true,
                    ));
                    self.status
                        .set_text("Automatic sign-in · saved Linux username sent");
                }
                Action::Password => {
                    self.auto_gdm.borrow_mut().fetching = true;
                    let (tx, rx) = std::sync::mpsc::sync_channel(1);
                    let reference = p.preferences.gnome_secret_reference.clone();
                    std::thread::spawn(move || {
                        let _ = tx.send(crate::credentials::lookup(&reference).map(SensitiveText));
                    });
                    let weak = Rc::downgrade(self);
                    let deadline = std::time::Instant::now() + Duration::from_secs(10);
                    glib::timeout_add_local(Duration::from_millis(25), move || {
                        let Some(v) = weak.upgrade() else {
                            return glib::ControlFlow::Break;
                        };
                        if !v.auto_gdm.borrow().armed
                            || v.assist_epoch.load(Ordering::Acquire) != epoch
                            || !v.auto_ready(&p, &transport, generation)
                        {
                            v.auto_gdm_stop(
                                "Automatic sign-in stopped: credential target or focus changed",
                            );
                            return glib::ControlFlow::Break;
                        }
                        match rx.try_recv() {
                            Ok(Ok(text)) => {
                                if text.0.is_empty()
                                    || text.0.len() > 4096
                                    || text.0.chars().any(char::is_control)
                                {
                                    v.auto_gdm_stop(
                                        "Automatic sign-in stopped: unsupported credential text",
                                    );
                                } else {
                                    let mut a = v.auto_gdm.borrow_mut();
                                    a.fetching = false;
                                    a.password_queued = true;
                                    // Bind completion to the connection/frame
                                    // that authorized this lookup, not a retry
                                    // that may have started while it finished.
                                    a.submitted_handovers = handovers;
                                    a.submitted_attempt = attempt;
                                    drop(a);
                                    v.command(Command::Assisted(
                                        text,
                                        v.assist_epoch.clone(),
                                        epoch,
                                        generation,
                                        true,
                                    ));
                                    v.status.set_text(
                                        "Automatic sign-in · awaiting authenticated user desktop",
                                    );
                                }
                                glib::ControlFlow::Break
                            }
                            Ok(Err(_)) => {
                                v.auto_gdm_stop(
                                    "Automatic sign-in stopped: saved Linux credential unavailable",
                                );
                                glib::ControlFlow::Break
                            }
                            _ if std::time::Instant::now() < deadline => {
                                glib::ControlFlow::Continue
                            }
                            _ => {
                                v.auto_gdm_stop(
                                    "Automatic sign-in stopped: credential lookup deadline",
                                );
                                glib::ControlFlow::Break
                            }
                        }
                    });
                    return;
                }
                Action::Wait => {}
            }
        }
        let mut a = self.auto_gdm.borrow_mut();
        if a.pending.is_none() && a.scan_at.elapsed() > Duration::from_millis(700) {
            if let Some((w, h, g, pixels)) = a.image.as_ref().cloned() {
                if g != generation {
                    drop(a);
                    self.auto_gdm_stop("Automatic sign-in stopped: stale rendered frame");
                    return;
                }
                a.scan_binding = Some((transport, generation, epoch));
                let trusted = !matches!(
                    a.sequence.step,
                    crate::gdm::Step::UserList | crate::gdm::Step::Stopped
                );
                a.pending = Some(crate::gdm::recognize(w, h, pixels, trusted));
                a.scan_at = std::time::Instant::now();
            }
        }
    }
    fn assist_dialog(self: &Rc<Self>) {
        let Some(p) = self.profile_metadata.borrow().clone() else {
            self.status
                .set_text("Assisted sign-in requires a saved GNOME Remote Login profile");
            return;
        };
        let Some((transport, generation)) = self
            .connection
            .borrow()
            .as_ref()
            .map(|c| (c.id.clone(), c.state.lock().unwrap().generation))
        else {
            return;
        };
        if !self.assist_ready(&p, &transport, generation, false) {
            self.status.set_text("Assisted sign-in requires the selected, authenticated GNOME tab with a decoded desktop and unchanged saved endpoint");
            return;
        }
        let Some(parent) = self.root.root().and_downcast::<gtk::Window>() else {
            return;
        };
        let d = gtk::Dialog::builder()
            .title("Confirm GNOME login input")
            .transient_for(&parent)
            .modal(true)
            .build();
        let label = gtk::Label::new(Some(&format!(
            "Selected connection: {}\nEndpoint: {}\nAddress: {}\nLinux user: {}\nConfirm that this selected tab shows GDM and the intended field has the caret. Send username first, then advance to the password field yourself and confirm again. RUWAC sends neither Tab nor Enter and cannot detect GDM reliably.",
            p.name, p.id, p.server, p.preferences.gnome_username
        )));
        label.set_wrap(true);
        label.set_max_width_chars(64);
        d.content_area().append(&label);
        let confirm =
            gtk::CheckButton::with_label("I verified this endpoint, GDM screen and focused field");
        confirm.set_widget_name("gnome-screen-confirmation");
        d.content_area().append(&confirm);
        d.add_button("Cancel", gtk::ResponseType::Cancel);
        let user = d.add_button("Send Linux username", gtk::ResponseType::Other(1));
        let password = d.add_button("Send Linux password", gtk::ResponseType::Other(2));
        user.set_sensitive(false);
        password.set_sensitive(false);
        confirm.connect_toggled(move |c| {
            user.set_sensitive(c.is_active());
            password.set_sensitive(c.is_active());
        });
        let weak = Rc::downgrade(self);
        d.connect_response(move|d,r|{
            d.close(); let Some(v)=weak.upgrade()else{return;};
            if !confirm.is_active() || !matches!(r,gtk::ResponseType::Other(1)|gtk::ResponseType::Other(2)){return;}
            let (v,p,transport)=(v.clone(),p.clone(),transport.clone());
            glib::timeout_add_local_once(Duration::from_millis(150),move||{
            v.focus();
            if !v.assist_ready(&p,&transport,generation,true){v.status.set_text("Sign-in cancelled: selected endpoint or transport changed");return;}
            let epoch=v.assist_epoch.fetch_add(1,Ordering::AcqRel)+1;
            let (tx,rx)=std::sync::mpsc::sync_channel(1);let p2=p.clone();
            std::thread::spawn(move||{let result=if r==gtk::ResponseType::Other(1){if p2.preferences.gnome_username.is_empty(){Err("Linux username is missing".into())}else{Ok(p2.preferences.gnome_username)}}else{crate::credentials::lookup(&p2.preferences.gnome_secret_reference)};let _=tx.send(result.map(SensitiveText));});
            let (weak,p,transport)=(Rc::downgrade(&v),p.clone(),transport.clone());let deadline=std::time::Instant::now()+Duration::from_secs(10);
            glib::timeout_add_local(Duration::from_millis(25),move||{
                let Some(v)=weak.upgrade()else{return glib::ControlFlow::Break;};
                if v.assist_epoch.load(Ordering::Acquire)!=epoch || !v.assist_ready(&p,&transport,generation,true){v.status.set_text("Sign-in cancelled: focus, profile, tab or connection changed");return glib::ControlFlow::Break;}
                match rx.try_recv(){Ok(Ok(text))=>{if text.0.is_empty()||text.0.len()>4096||text.0.chars().any(char::is_control){v.status.set_text("Sign-in cancelled: unsupported credential text");}else if let Some(c)=v.connection.borrow().as_ref(){if c.queue(Command::Assisted(text,v.assist_epoch.clone(),epoch,generation,false)).is_err(){v.status.set_text("Sign-in input not queued");}else{v.status.set_text("Confirmed field input queued; submit at GDM yourself. Login success is not verified.");}}glib::ControlFlow::Break},Ok(Err(e))=>{v.status.set_text(&e);glib::ControlFlow::Break},Err(std::sync::mpsc::TryRecvError::Empty) if std::time::Instant::now()<deadline=>glib::ControlFlow::Continue,_=>{v.status.set_text("Sign-in credential lookup unavailable");glib::ControlFlow::Break}}
            });
            });
        });
        d.present();
    }
    pub fn focus(&self) {
        self.area.grab_focus();
        self.activate_clipboard();
    }
    fn release_keys(&self) {
        if let Some(c) = self.connection.borrow().as_ref() {
            c.invalidate_input();
        }
        self.pending_super.borrow_mut().clear();
        self.release_buttons();
        let keys: Vec<_> = self
            .held
            .borrow_mut()
            .drain()
            .map(|(k, e)| {
                FastPathInputEvent::KeyboardEvent(
                    KeyboardFlags::RELEASE
                        | if e {
                            KeyboardFlags::EXTENDED
                        } else {
                            KeyboardFlags::empty()
                        },
                    k,
                )
            })
            .collect();
        if !keys.is_empty() {
            self.command(Command::Release(keys));
        }
    }
    fn release_buttons(&self) {
        let (x, y) = self.pointer_position.get();
        let events: Vec<_> = self
            .buttons
            .borrow_mut()
            .drain()
            .filter_map(|b| {
                let flags = match b {
                    1 => PointerFlags::LEFT_BUTTON,
                    2 => PointerFlags::MIDDLE_BUTTON_OR_WHEEL,
                    3 => PointerFlags::RIGHT_BUTTON,
                    _ => return None,
                };
                Some(FastPathInputEvent::MouseEvent(MousePdu {
                    flags,
                    number_of_wheel_rotation_units: 0,
                    x_position: x,
                    y_position: y,
                }))
            })
            .collect();
        if !events.is_empty() {
            self.command(Command::Release(events));
        }
    }
    pub fn close(&self) {
        self.primary.borrow_mut().take();
        self.primary_attempted.set(false);
        self.primary_status.set_visible(false);
        self.primary_checked.set(None);
        self.pending_connect.borrow_mut().take();
        *self.auto_gdm.borrow_mut() = AutoGdm::default();
        self.rendered_generation.set(0);
        self.surface_generation.set(0);
        self.assist_epoch.fetch_add(1, Ordering::AcqRel);
        self.serial.set(self.serial.get().saturating_add(1));
        self.release_keys();
        if let Some(c) = self.connection.borrow_mut().take() {
            c.close();
        }
        self.bridge.borrow().epoch.store(0, Ordering::Release);
        self.pending.borrow_mut().take();
        self.lazy_reply.borrow_mut().take();
        self.remote_formats.borrow_mut().clear();
        self.remote.set(None);
        self.received_generation.set(0);
        self.advertised.set(0);
        self.replying.set(false);
        self.decoding.set(false);
        self.offers.borrow_mut().acknowledge();
        self.offers.borrow_mut().waiting.take();
        self.surface.borrow_mut().take();
        self.size.set((0, 0));
        self.area.queue_draw();
        self.broker.borrow_mut().disconnect(&self.id);
        self.status.set_text("RDP disconnected");
    }
    pub fn fixture_frame(&self) {
        let mut pixels = vec![0; 320 * 180 * 4];
        for (y, row) in pixels.chunks_mut(320 * 4).enumerate() {
            for (x, p) in row.chunks_mut(4).enumerate() {
                p.copy_from_slice(&[(x % 255) as u8, (y % 255) as u8, 80, 255]);
            }
        }
        self.size.set((320, 180));
        *self.surface.borrow_mut() = gtk::cairo::ImageSurface::create_for_data(
            pixels,
            gtk::cairo::Format::Rgb24,
            320,
            180,
            320 * 4,
        )
        .ok();
        self.status
            .set_text("Synthetic framebuffer fixture · no remote connection");
        self.area.queue_draw();
    }
    fn update(self: &Rc<Self>) {
        let epoch = if self.broker.borrow().active() == Some(self.id.as_str()) {
            self.broker.borrow().epoch()
        } else {
            0
        };
        self.bridge.borrow().epoch.store(epoch, Ordering::Release);
        if self
            .pending
            .borrow()
            .as_ref()
            .is_some_and(|p| p.created.elapsed() > Duration::from_secs(30))
        {
            self.close();
            self.status.set_text(
                "CLIPRDR response timed out; disconnected to prevent stale response reuse",
            );
            return;
        }
        let events = self.bridge.borrow().drain();
        for e in events {
            self.clip_event(e);
        }

        let frame = {
            let c = self.connection.borrow();
            let Some(c) = c.as_ref() else {
                return;
            };
            let mut s = c.state.lock().unwrap();
            if let Some(review) = s.certificate.take() {
                *self.certificate.borrow_mut() = Some(review);
                self.certificate_prompt_pending.set(true);
                self.review_certificate.set_visible(true);
            }
            if let Some(status) = s.status.take() {
                self.status.set_text(&status);
            }
            s.frame.take()
        };
        // Ask on the selected foreground connection, even when its toolbar is
        // hidden. Background failures wait until their tab is selected.
        if self.certificate_prompt_pending.get()
            && self
                .selected
                .borrow()
                .as_ref()
                .is_none_or(|selected| selected())
            && self.root.is_mapped()
            && self
                .root
                .root()
                .and_downcast::<gtk::Window>()
                .is_some_and(|w| w.is_active())
        {
            self.review_peer_certificate();
        }
        if let Some(f) = frame {
            // GNOME hands the authenticated user to a fresh desktop with its
            // own initial resolution. Reapply the viewport request there;
            // an already matching resize acknowledgement must not loop.
            if self.mode.get() == Scale::Dynamic
                && f.generation != self.surface_generation.get()
                && self.requested.get() != (0, 0)
                && self.requested.get() != (f.width, f.height)
            {
                self.requested.set((0, 0));
                self.resize_at.set(Some(std::time::Instant::now()));
            }
            {
                let mut a = self.auto_gdm.borrow_mut();
                // A stopped greeter keeps its last frame so Retry can scan it.
                if (a.armed && !a.fetching && a.sequence.step != crate::gdm::Step::Submitted)
                    || (!a.armed && a.stopped_at.is_some())
                {
                    a.image = Some((f.width, f.height, f.generation, f.pixels.clone()));
                }
            }
            if self.size.get() != (f.width, f.height) {
                self.release_keys();
                self.surface.borrow_mut().take();
            }
            self.size.set((f.width, f.height));
            self.last_size.set(Some((f.width, f.height)));
            if let Some(p) = f.pixels.get(f.pixels.len().saturating_sub(4)..) {
                if p.len() == 4 {
                    self.last_pixel.set(p.try_into().unwrap());
                }
            }
            match gtk::cairo::ImageSurface::create_for_data(
                f.pixels,
                gtk::cairo::Format::Rgb24,
                f.width as i32,
                f.height as i32,
                f.width as i32 * 4,
            ) {
                Ok(surface) => {
                    self.surface_generation.set(f.generation);
                    *self.surface.borrow_mut() = Some(surface);
                    self.area.queue_draw();
                }
                Err(_) => self.status.set_text("Framebuffer format rejected"),
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn explicit_retry_survives_a_later_handover_but_needs_a_verified_live_greeter() {
        let mut a = AutoGdm::default();
        assert!(!a.retry_on_greeter(1, true));
        a.stopped_at = Some(1);
        assert!(a.retry_on_greeter(1, true));
        assert!(a.retry_on_greeter(2, true));
        assert!(!a.retry_on_greeter(2, false));
        assert!(!a.retry_on_greeter(0, true));
        a.armed = true;
        assert!(!a.retry_on_greeter(2, true));
    }
    #[test]
    fn completed_login_rearms_only_for_a_later_verified_service_handover() {
        let mut a = AutoGdm::default();
        assert!(!a.reconnects_to_greeter("transport", 8, 3));
        a.completed = Some(("transport".into(), 7, 2));
        assert!(!a.reconnects_to_greeter("other", 8, 3));
        assert!(!a.reconnects_to_greeter("transport", 7, 3));
        assert!(!a.reconnects_to_greeter("transport", 8, 2));
        assert!(a.reconnects_to_greeter("transport", 8, 3));
        a.armed = true;
        assert!(!a.reconnects_to_greeter("transport", 8, 3));
        a.armed = false;
        a.completed = None; // An explicit stop/cancel clears this permission.
        assert!(!a.reconnects_to_greeter("transport", 8, 3));
    }

    #[test]
    fn blackholed_setup_obeys_the_remaining_recovery_budget_and_cancel() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            for cancel in [false, true] {
                let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
                let port = listener.local_addr().unwrap().port();
                let peer = tokio::spawn(async move {
                    let (_socket, _) = listener.accept().await.unwrap();
                    std::future::pending::<()>().await;
                });
                let profile = Profile {
                    host: "127.0.0.1".into(),
                    port,
                    user: "generated".into(),
                    reference: String::new(),
                    ca: None,
                    reconnect: true,
                    desktop_size: (800, 600),
                    graphics: false,
                    clipboard: false,
                    handover: None,
                };
                let (_tx, mut rx) = tokio::sync::mpsc::channel(1);
                let halt = Arc::new(AtomicBool::new(false));
                let stop = halt.clone();
                let canceller = tokio::spawn(async move {
                    if cancel {
                        tokio::time::sleep(Duration::from_millis(20)).await;
                        stop.store(true, Ordering::Relaxed);
                    }
                });
                let at = std::time::Instant::now();
                let result = run_session(
                    profile,
                    "generated".into(),
                    &mut rx,
                    halt,
                    Arc::new(Mutex::new(State::default())),
                    if cancel {
                        Duration::from_secs(30)
                    } else {
                        Duration::from_millis(80)
                    },
                )
                .await;
                peer.abort();
                let _ = peer.await;
                let _ = canceller.await;
                assert!(at.elapsed() < Duration::from_secs(3));
                if cancel {
                    assert!(result.is_ok());
                } else {
                    assert!(result.unwrap_err().starts_with("RDP connection timed out"));
                }
            }
        });
    }

    #[test]
    fn initial_gdm_handover_cannot_finish_pending_password_lookup() {
        let mut a = AutoGdm::default();
        a.sequence.step = crate::gdm::Step::Submitted;
        a.fetching = true;
        a.binding = Some(("generated-transport".into(), 4, 1));
        a.submitted_attempt = 4;
        // The real service-to-GDM handover is already counted before lookup.
        assert!(!a.authenticated_desktop_handover(1, 4));
        a.fetching = false;
        assert!(!a.authenticated_desktop_handover(1, 4));
        a.submitted_handovers = 1;
        a.password_queued = true;
        assert!(!a.authenticated_desktop_handover(1, 4));
        assert!(a.authenticated_desktop_handover(2, 5));
        // A transport retry through service->greeter is not the desktop
        // handover caused by this password submission.
        assert!(!a.authenticated_desktop_handover(2, 6));
        assert!(!a.authenticated_desktop_handover(3, 6));
        a.sequence.stop();
        assert!(!a.authenticated_desktop_handover(2, 5));
    }
    #[test]
    fn handover_is_consumed_once_and_service_identity_survives_retry() {
        let mut packet = vec![0, 4, 0, 0];
        packet.extend_from_slice(&42u32.to_le_bytes());
        packet.extend_from_slice(&(0x2u32 | 0x4 | 0x10 | 0x4000 | 0x8000).to_le_bytes());
        for value in [
            b"12345".to_vec(),
            vec![b'u', 0, 0, 0],
            vec![0xa5; 128],
            vec![b'g', 0, 0, 0],
        ] {
            packet.extend_from_slice(&(value.len() as u32).to_le_bytes());
            packet.extend(value);
        }
        let length = packet.len() as u16;
        packet[2..4].copy_from_slice(&length.to_le_bytes());
        let mut data = ((packet.len() + 8) as u16).to_le_bytes().to_vec();
        data.extend_from_slice(&[0x1a, 0, 0, 0, 0, 0]);
        data.extend(packet);
        let plan = Arc::new(crate::handover::parse(&data).unwrap().unwrap());
        let mut profile = Profile {
            host: "fixture.invalid".into(),
            port: 3389,
            user: "service-user".into(),
            reference: "service-reference".into(),
            ca: None,
            reconnect: true,
            desktop_size: (1280, 720),
            graphics: true,
            clipboard: true,
            handover: Some(plan.clone()),
        };
        let redirected = profile.next_attempt();
        assert!(Arc::ptr_eq(redirected.handover.as_ref().unwrap(), &plan));
        let retry = profile.next_attempt();
        assert!(retry.handover.is_none());
        assert_eq!(retry.user, "service-user");
        assert_eq!(retry.reference, "service-reference");
        assert_eq!((retry.host.as_str(), retry.port), ("fixture.invalid", 3389));
        assert!(retry.graphics && retry.reconnect);
    }
    #[test]
    fn real_loopback_tls_validation() {
        use std::{
            fs,
            io::{Read, Write},
            os::unix::fs::{DirBuilderExt, PermissionsExt},
        };
        let base = std::env::temp_dir().join(ruwac_protocol::id("rdp_tls"));
        fs::DirBuilder::new().mode(0o700).create(&base).unwrap();
        let cert = base.join("cert.pem");
        let key = base.join("key.pem");
        let status = std::process::Command::new("openssl")
            .args([
                "req",
                "-x509",
                "-newkey",
                "rsa:2048",
                "-nodes",
                "-days",
                "1",
                "-subj",
                "/CN=RUWAC-local-fixture",
                "-addext",
                "subjectAltName=IP:127.0.0.1",
                "-keyout",
            ])
            .arg(&key)
            .arg("-out")
            .arg(&cert)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap();
        assert!(status.success());
        fs::set_permissions(&key, fs::Permissions::from_mode(0o600)).unwrap();
        let identity =
            native_tls::Identity::from_pkcs8(&fs::read(&cert).unwrap(), &fs::read(&key).unwrap())
                .unwrap();
        for trusted in [false, true] {
            let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
            let port = listener.local_addr().unwrap().port();
            let identity = identity.clone();
            let server = std::thread::spawn(move || {
                let (mut tcp, _) = listener.accept().unwrap();
                tcp.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
                tcp.set_write_timeout(Some(Duration::from_secs(3))).unwrap();
                let mut header = [0; 4];
                tcp.read_exact(&mut header).unwrap();
                let len = u16::from_be_bytes([header[2], header[3]]) as usize;
                assert!((4..4096).contains(&len));
                let mut data = vec![0; len - 4];
                tcp.read_exact(&mut data).unwrap();
                tcp.write_all(&[3, 0, 0, 19, 14, 0xd0, 0, 0, 0, 0, 0, 2, 0, 8, 0, 2, 0, 0, 0])
                    .unwrap();
                let acceptor = native_tls::TlsAcceptor::new(identity).unwrap();
                let _ = acceptor.accept(tcp);
            });
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            let (_tx, rx) = tokio::sync::mpsc::channel(1);
            let result = rt.block_on(run(
                Profile {
                    host: "127.0.0.1".into(),
                    port,
                    user: "synthetic-fixture".into(),
                    reference: String::new(),
                    // A certificate failure is never retried, even with
                    // reconnect on; a setup that closes after TLS is a
                    // retried transport failure, so it is classified here
                    // with reconnect off.
                    reconnect: !trusted,
                    desktop_size: (1280, 720),
                    graphics: false,
                    clipboard: true,
                    handover: None,
                    ca: if trusted {
                        Some(cert.to_string_lossy().into_owned())
                    } else {
                        None
                    },
                },
                "synthetic-fixture-password".into(),
                rx,
                Arc::new(AtomicBool::new(false)),
                Arc::new(Mutex::new(State::default())),
            ));
            assert_eq!(
                result.unwrap_err(),
                if trusted {
                    "RDP protocol/session setup failed"
                } else {
                    "TLS certificate/hostname validation failed"
                }
            );
            server.join().unwrap();
        }
        fs::remove_file(cert).unwrap();
        fs::remove_file(key).unwrap();
        fs::remove_dir(base).unwrap();
    }
    #[test]
    fn display_reassembly_capped_before_allocation() {
        use ironrdp::dvc::pdu::{DataFirstPdu, DataPdu, DrdynvcDataPdu, DrdynvcServerPdu};
        fn svc(bytes: &[u8]) -> Vec<u8> {
            [
                (bytes.len() as u32).to_le_bytes().as_slice(),
                3u32.to_le_bytes().as_slice(),
                bytes,
            ]
            .concat()
        }
        let mut guard = DisplayFragmentGuard::default();
        let caps =
            ironrdp::displaycontrol::pdu::DisplayControlCapabilities::new(1, 3840, 2400).unwrap();
        let encoded =
            ironrdp_core::encode_vec(&ironrdp::displaycontrol::pdu::DisplayControlPdu::Caps(caps))
                .unwrap();
        let data = ironrdp_core::encode_vec(&DrdynvcServerPdu::Data(DrdynvcDataPdu::Data(
            DataPdu::new(1, encoded),
        )))
        .unwrap();
        assert!(guard.check(&svc(&data)).is_ok());
        let bad = ironrdp_core::encode_vec(&DrdynvcServerPdu::Data(DrdynvcDataPdu::DataFirst(
            DataFirstPdu::new(1, u32::MAX, vec![1]),
        )))
        .unwrap();
        assert!(guard.check(&svc(&bad)).is_err());
        assert!(
            DisplayFragmentGuard::default()
                .check(
                    &[
                        65537u32.to_le_bytes().as_slice(),
                        3u32.to_le_bytes().as_slice()
                    ]
                    .concat()
                )
                .is_err()
        );
    }
    #[test]
    fn display_control_framed_caps_delegate_and_bound() {
        use ironrdp::dvc::DvcProcessor;
        let maximum = Arc::new(Mutex::new(0u64));
        let capture = maximum.clone();
        let mut client = DisplayControlAdapter(
            ironrdp::displaycontrol::client::DisplayControlClient::new(move |caps| {
                *capture.lock().unwrap() = caps.max_monitor_area();
                Ok(vec![])
            }),
            None,
        );
        let caps =
            ironrdp::displaycontrol::pdu::DisplayControlCapabilities::new(1, 3840, 2400).unwrap();
        let wire =
            ironrdp_core::encode_vec(&ironrdp::displaycontrol::pdu::DisplayControlPdu::Caps(caps))
                .unwrap();
        assert_eq!(wire.len(), 20);
        client.process(1, &wire).unwrap();
        assert!(client.0.ready());
        assert_eq!(*maximum.lock().unwrap(), 3840 * 2400);
        assert!(client.process(1, &wire[..12]).is_err());
        let mut bad = wire;
        bad[4] = 255;
        assert!(client.process(1, &bad).is_err());
    }
    #[test]
    fn viewport_zoom_fractional_scale_pan_and_display_layout() {
        let native = viewport_geometry(
            (2048.0, 1152.0),
            (800.0, 600.0),
            (2560, 1440),
            Scale::Native,
            1.0,
            1.25,
        );
        assert_eq!((native.0, native.1), (0.8, 0.8));
        // GTK delivers canvas coordinates including adjustment offsets, not viewport coordinates.
        let pointer = (400.0 + 800.0) / native.0;
        assert_eq!(pointer, 1500.0);
        let fit = viewport_geometry(
            (800.0, 600.0),
            (800.0, 600.0),
            (2560, 1440),
            Scale::Fit,
            1.0,
            1.25,
        );
        assert_eq!(fit.0, 0.3125);
        assert_eq!(fit.3, 75.0);
        let stretch = viewport_geometry(
            (800.0, 600.0),
            (800.0, 600.0),
            (2560, 1440),
            Scale::Stretch,
            1.25,
            1.0,
        );
        assert_ne!(stretch.0, stretch.1);
        use ironrdp::displaycontrol::pdu::DisplayControlMonitorLayout;
        assert!(
            DisplayControlMonitorLayout::new_single_primary_monitor(2560, 1440, Some(125), None)
                .is_ok()
        );
        assert!(
            DisplayControlMonitorLayout::new_single_primary_monitor(3000, 1800, Some(150), None)
                .is_ok()
        );
        assert_eq!(
            DisplayControlMonitorLayout::new_single_primary_monitor(201, 200, None, None)
                .unwrap()
                .monitors()[0]
                .dimensions(),
            (200, 200)
        );
        assert!(
            DisplayControlMonitorLayout::new_single_primary_monitor(0, 200, None, None).is_err()
        );
    }
    #[test]
    fn transforms() {
        assert_eq!(
            transform((800.0, 600.0), (1280, 720), Scale::Fit, 400.0, 300.0),
            Some((640, 360))
        );
        assert_eq!(
            transform((800.0, 600.0), (1280, 720), Scale::Fit, 400.0, 10.0),
            None
        );
        assert_eq!(
            transform((800.0, 600.0), (1280, 720), Scale::Stretch, 400.0, 300.0),
            Some((640, 360))
        );
        assert_eq!(
            transform((1280.0, 720.0), (1280, 720), Scale::Native, 100.0, 200.0),
            Some((100, 200))
        );
    }
}
#[cfg(test)]
#[path = "rdp_fixture.rs"]
mod authenticated_fixture;

#[cfg(test)]
mod resize_stability_tests {
    use super::*;
    #[test]
    fn initial_connection_waits_for_stable_real_allocation() {
        let start = std::time::Instant::now();
        let mut gate = InitialViewport::default();
        assert_eq!(gate.observe(None, start), None);
        assert_eq!(gate.observe(Some((1400, 868)), start), None);
        assert_eq!(
            gate.observe(Some((1400, 868)), start + Duration::from_millis(99)),
            None
        );
        assert_eq!(
            gate.observe(Some((1400, 868)), start + Duration::from_millis(100)),
            Some((1400, 868))
        );
    }
    #[test]
    fn initial_connection_restarts_wait_when_layout_changes() {
        let start = std::time::Instant::now();
        let mut gate = InitialViewport::default();
        gate.observe(Some((1280, 720)), start);
        assert_eq!(
            gate.observe(Some((1400, 868)), start + Duration::from_millis(90)),
            None
        );
        assert_eq!(
            gate.observe(Some((1400, 868)), start + Duration::from_millis(150)),
            None
        );
        assert_eq!(
            gate.observe(Some((1400, 868)), start + Duration::from_millis(190)),
            Some((1400, 868))
        );
    }
    #[test]
    fn initial_connection_loses_unmapped_sample_without_jitter_accumulation() {
        let start = std::time::Instant::now();
        let mut gate = InitialViewport::default();
        gate.observe(Some((1400, 868)), start);
        assert_eq!(
            gate.observe(Some((1402, 869)), start + Duration::from_millis(100)),
            Some((1400, 868))
        );
        gate.observe(None, start + Duration::from_millis(110));
        assert_eq!(
            gate.observe(Some((1400, 868)), start + Duration::from_millis(120)),
            None
        );
    }
    #[test]
    fn allocation_jitter_does_not_accumulate() {
        let mut previous = (1000, 700);
        for i in 0..200 {
            let raw = if i % 2 == 0 { (998, 699) } else { (1002, 701) };
            previous = stable_resize(raw, previous);
            assert_eq!(previous, (1000, 700));
        }
        assert_eq!(stable_resize((1200, 800), previous), (1200, 800));
        assert_eq!(stable_resize((200, 200), (0, 0)), (200, 200));
    }
}

#[cfg(test)]
#[path = "rdp_protocol_tests.rs"]
mod protocol_conformance;

#[cfg(test)]
mod queue_safety_tests {
    use super::*;
    #[test]
    fn hard_count_bound_rejects_before_insert_and_reserves_control_capacity() {
        let budget = Arc::default();
        let epoch = Arc::new(AtomicU64::new(1));
        let mut commands = Vec::new();
        for _ in 0..4064 {
            commands.push(
                QueueBudget::reserve(&budget, Command::Input(vec![], epoch.clone(), 1, 0)).unwrap(),
            );
        }
        assert!(
            QueueBudget::reserve(&budget, Command::Input(vec![], epoch.clone(), 1, 0)).is_err()
        );
        assert_eq!(budget.lock().unwrap().count, 4064);
        commands.push(QueueBudget::reserve(&budget, Command::Reply(None)).unwrap());
        assert_eq!(budget.lock().unwrap().count, 4065);
        drop(commands);
        assert_eq!(budget.lock().unwrap().count, 0);
        assert_eq!(budget.lock().unwrap().bytes, 0);
    }
    #[test]
    fn bytes_bound_rejects_without_changing_accounting() {
        let budget = Arc::new(Mutex::new(QueueBudget {
            count: 1,
            bytes: 80 * 1024 * 1024 - 100,
        }));
        assert!(QueueBudget::reserve(&budget, Command::Reply(None)).is_err());
        assert_eq!(budget.lock().unwrap().bytes, 80 * 1024 * 1024 - 100);
    }
    #[test]
    fn foreground_and_transport_generations_cancel_queued_input_but_not_releases() {
        let epoch = Arc::new(AtomicU64::new(3));
        let command = Command::Input(vec![], epoch.clone(), 3, 10);
        assert!(!command.stale(10));
        assert!(command.stale(11));
        epoch.fetch_add(1, Ordering::AcqRel);
        assert!(command.stale(10));
        assert!(!Command::Release(vec![]).stale(11));
    }
}
