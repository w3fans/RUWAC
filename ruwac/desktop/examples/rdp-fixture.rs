//! Test-only localhost RDP server. Run against a new private fixture directory.
#[path = "../src/cliprdr.rs"]
mod cliprdr;
use cliprdr::{Backend, Bridge, Event, Format};
use ironrdp_cliprdr::{
    backend::{ClipboardMessage, CliprdrBackendFactory},
    pdu::FormatDataResponse,
};
use ironrdp_server::*;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    num::{NonZeroU16, NonZeroUsize},
    os::unix::fs::MetadataExt,
    path::PathBuf,
    sync::Arc,
    time::Duration,
};
fn fixture_monotonic_ns() -> anyhow::Result<u64> {
    let mut stamp = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    anyhow::ensure!(
        unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut stamp) } == 0,
        "fixture clock unavailable"
    );
    Ok(stamp.tv_sec as u64 * 1_000_000_000 + stamp.tv_nsec as u64)
}
fn fixture_stamp(path: &std::path::Path, value: serde_json::Value) -> anyhow::Result<()> {
    use std::os::unix::fs::OpenOptionsExt;
    let temporary = path.with_extension(format!("{}.tmp", fixture_monotonic_ns()?));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&temporary)?;
    std::io::Write::write_all(&mut file, &serde_json::to_vec(&value)?)?;
    drop(file);
    std::fs::rename(temporary, path)?;
    Ok(())
}
/// Receipt for generated loopback input only; never stores typed text.
struct InputReceipt {
    path: PathBuf,
    units: Vec<u16>,
    mouse: Vec<String>,
    keys: Vec<serde_json::Value>,
}
impl RdpServerInputHandler for InputReceipt {
    fn keyboard(&mut self, event: KeyboardEvent) {
        let key = match &event {
            KeyboardEvent::Pressed { code, extended } => {
                Some(json!({"code":code,"down":true,"extended":extended}))
            }
            KeyboardEvent::Released { code, extended } => {
                Some(json!({"code":code,"down":false,"extended":extended}))
            }
            _ => None,
        };
        if let Some(key) = key {
            if self.keys.len() == 4096 {
                self.keys.remove(0);
            }
            self.keys.push(key);
            let _ = fixture_stamp(
                &self.path.with_extension("keys.json"),
                json!({"events":self.keys}),
            );
        }
        if let KeyboardEvent::UnicodePressed(u) = event {
            self.units.push(u);
            let bytes: Vec<u8> = self.units.iter().flat_map(|v| v.to_le_bytes()).collect();
            let _=std::fs::write(&self.path,serde_json::to_vec(&json!({"units":self.units.len(),"sha256":format!("{:x}",Sha256::digest(&bytes))})).unwrap());
        }
    }
    fn mouse(&mut self, event: MouseEvent) {
        if self.mouse.len() == 1000 {
            self.mouse.remove(0);
        }
        self.mouse.push(format!("{event:?}"));
        let _ = fixture_stamp(
            &self.path.with_extension("mouse.json"),
            json!({"events":self.mouse}),
        );
    }
}
struct Display {
    marker: Option<PathBuf>,
    size: Arc<std::sync::Mutex<DesktopSize>>,
    tx: tokio::sync::mpsc::Sender<DesktopSize>,
    rx: Option<tokio::sync::mpsc::Receiver<DesktopSize>>,
}
struct Updates {
    size: Arc<std::sync::Mutex<DesktopSize>>,
    rx: tokio::sync::mpsc::Receiver<DesktopSize>,
    clock: tokio::time::Interval,
    marker: Option<PathBuf>,
    sequence: u32,
}
impl Display {
    fn new(marker: Option<PathBuf>) -> Self {
        let (tx, rx) = tokio::sync::mpsc::channel(1);
        Self {
            size: Arc::new(std::sync::Mutex::new(DesktopSize {
                width: if marker.is_some() { 640 } else { 320 },
                height: if marker.is_some() { 480 } else { 180 },
            })),
            tx,
            rx: Some(rx),
            marker,
        }
    }
}
#[async_trait::async_trait]
impl RdpServerDisplay for Display {
    async fn size(&mut self) -> DesktopSize {
        *self.size.lock().unwrap()
    }
    async fn updates(&mut self) -> anyhow::Result<Box<dyn RdpServerDisplayUpdates>> {
        let rx = self.rx.take().unwrap_or_else(|| {
            let (tx, rx) = tokio::sync::mpsc::channel(1);
            self.tx = tx;
            rx
        });
        let mut clock = tokio::time::interval(Duration::from_millis(200));
        clock.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        Ok(Box::new(Updates {
            size: self.size.clone(),
            rx,
            clock,
            marker: self.marker.clone(),
            sequence: 0,
        }))
    }
    fn request_layout(
        &mut self,
        layout: ironrdp::displaycontrol::pdu::DisplayControlMonitorLayout,
    ) {
        if self.marker.is_some() {
            return;
        }
        if let Some(m) = layout.monitors().first() {
            let (w, h) = m.dimensions();
            if w <= 4096 && h <= 4096 {
                let _ = self.tx.try_send(DesktopSize {
                    width: w as u16,
                    height: h as u16,
                });
            }
        }
    }
}
#[async_trait::async_trait]
impl RdpServerDisplayUpdates for Updates {
    async fn next_update(&mut self) -> anyhow::Result<Option<DisplayUpdate>> {
        tokio::select! {Some(size)=self.rx.recv()=>{*self.size.lock().unwrap()=size;return Ok(Some(DisplayUpdate::Resize(size)));},_=self.clock.tick()=>()}
        let size = *self.size.lock().unwrap();
        let (w, h) = (size.width as usize, size.height as usize);
        self.sequence = self.sequence.wrapping_add(1);
        let generated_ns = if self.marker.is_some() {
            fixture_monotonic_ns()?
        } else {
            0
        };
        let mut data = vec![0; w * h * 4];
        for (y, row) in data.chunks_mut(w * 4).enumerate() {
            for (x, p) in row.chunks_mut(4).enumerate() {
                p.copy_from_slice(&if self.marker.is_some() {
                    [211, (self.sequence >> 8) as u8, self.sequence as u8, 255]
                } else {
                    [x as u8, y as u8, 90, 255]
                });
            }
        }
        if let Some(path) = &self.marker {
            fixture_stamp(
                path,
                json!({"sequence":self.sequence,"generated_ns":generated_ns,"width":w,"height":h}),
            )?;
        }
        Ok(Some(DisplayUpdate::Bitmap(BitmapUpdate {
            x: 0,
            y: 0,
            width: NonZeroU16::new(size.width).unwrap(),
            height: NonZeroU16::new(size.height).unwrap(),
            format: PixelFormat::BgrX32,
            data: bytes::Bytes::from(data),
            stride: NonZeroUsize::new(w * 4).unwrap(),
        })))
    }
}
struct Factory(Bridge);
impl CliprdrBackendFactory for Factory {
    fn build_cliprdr_backend(&self) -> Box<dyn ironrdp_cliprdr::backend::CliprdrBackend> {
        Box::new(Backend::new(self.0.clone()))
    }
}
impl ServerEventSender for Factory {
    fn set_sender(&mut self, _: tokio::sync::mpsc::UnboundedSender<ServerEvent>) {}
}
impl CliprdrServerFactory for Factory {}
fn main() -> anyhow::Result<()> {
    let dir = PathBuf::from(
        std::env::args_os()
            .nth(1)
            .ok_or_else(|| anyhow::anyhow!("Private fixture directory required"))?,
    );
    let m = std::fs::symlink_metadata(&dir)?;
    anyhow::ensure!(
        m.is_dir() && m.uid() == unsafe { libc::geteuid() } && m.mode() & 0o077 == 0,
        "Fixture must be owned, private and not a symlink"
    );
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    rt.block_on(tokio::task::LocalSet::new().run_until(run(dir)))
}
async fn run(dir: PathBuf) -> anyhow::Result<()> {
    let mut ports = Vec::new();
    for tab in 0..3 {
        let identity =
            TlsIdentityCtx::init_from_paths(&dir.join("cert.pem"), &dir.join("key.pem"))?;
        let config = tokio_rustls::rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(identity.certs, identity.priv_key)?;
        let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(config));
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await?;
        let port = listener.local_addr()?.port();
        ports.push(port);
        let bridge = Bridge::default();
        let mut server = RdpServer::builder()
            .with_addr(([127, 0, 0, 1], port))
            .with_hybrid(acceptor, identity.pub_key)
            .with_input_handler(InputReceipt {
                path: dir.join(format!("input-{port}.json")),
                units: Vec::new(),
                mouse: Vec::new(),
                keys: Vec::new(),
            })
            .with_display_handler(Display::new(
                (std::env::var("RUWAC_FIXTURE_FRAME_MARKERS").as_deref() == Ok("1"))
                    .then(|| dir.join(format!("frame-marker-{tab}.json"))),
            ))
            .with_bitmap_codecs(ironrdp::pdu::rdp::capability_sets::BitmapCodecs(Vec::new()))
            .with_cliprdr_factory(Some(Box::new(Factory(bridge.clone()))))
            .with_honor_client_desktop_size(
                std::env::var_os("RUWAC_FIXTURE_HONOR_CLIENT_SIZE").is_some(),
            )
            .build();
        server.set_credentials(Some(Credentials {
            username: "fixture".into(),
            password: "generated-local-only".into(),
            domain: None,
        }));
        let sender = server.event_sender().clone();
        let accepted_marker = (std::env::var("RUWAC_FIXTURE_FRAME_MARKERS").as_deref() == Ok("1"))
            .then(|| dir.join(format!("accepted-marker-{tab}.json")));
        let d = dir.clone();
        tokio::task::spawn_local(async move {
            loop {
                match listener.accept().await {
                    Ok((s, _)) => {
                        if let Some(path) = &accepted_marker {
                            if fixture_monotonic_ns()
                                .and_then(|ns| fixture_stamp(path, json!({"accepted_ns":ns})))
                                .is_err()
                            {
                                break;
                            }
                        }
                        let _ = server.run_connection(s).await;
                    }
                    Err(_) => break,
                }
            }
        });
        tokio::task::spawn_local(async move {
            let mut seq = 0;
            let mut offered = Vec::new();
            let mut offered_kind = Format::Text;
            let mut offered_text: Option<Vec<u8>> = None;
            let mut requested = Format::Text;
            let mut received = 0;
            let mut request_count = 0u64;
            let mut data_delay = Duration::ZERO;
            let mut delayed: Option<(tokio::time::Instant, Option<Vec<u8>>)> = None;
            loop {
                tokio::time::sleep(Duration::from_millis(10)).await;
                if let Ok(b) = std::fs::read(d.join(format!("server-{tab}-request.json"))) {
                    if let Ok(q) = serde_json::from_slice::<serde_json::Value>(&b) {
                        if let Some(n) = q["seq"].as_u64() {
                            if n > seq {
                                seq = n;
                                let result = (|| -> Result<(), String> {
                                    if q["action"].as_str() == Some("disconnect") {
                                        sender
                                            .send(ServerEvent::Quit(
                                                "owned reconnect fixture".into(),
                                            ))
                                            .map_err(|_| "Fixture ended")?;
                                        return Ok(());
                                    }
                                    offered_kind = match q["format"].as_str() {
                                        Some("png" | "both") => Format::Png,
                                        Some("dib") => Format::Dib,
                                        Some("dibv5") => Format::DibV5,
                                        _ => Format::Text,
                                    };
                                    let name = q["file"]
                                        .as_str()
                                        .ok_or("Generated payload file required")?;
                                    if name.contains('/') || name.contains("..") {
                                        return Err("Payload must be fixture-local".into());
                                    }
                                    use std::io::Read;
                                    use std::os::unix::fs::OpenOptionsExt;
                                    let f = std::fs::OpenOptions::new()
                                        .read(true)
                                        .custom_flags(libc::O_NOFOLLOW)
                                        .open(d.join(name))
                                        .map_err(|_| "Cannot open fixture payload")?;
                                    offered.clear();
                                    f.take(64 * 1024 * 1024 + 1)
                                        .read_to_end(&mut offered)
                                        .map_err(|_| "Cannot read fixture payload")?;
                                    cliprdr::encode(offered_kind, &offered)?;
                                    offered_text = None;
                                    if q["format"] == "both" {
                                        let name = q["text_file"]
                                            .as_str()
                                            .ok_or("Text fixture required")?;
                                        if name.contains('/') || name.contains("..") {
                                            return Err("Text payload must be fixture-local".into());
                                        }
                                        let f = std::fs::OpenOptions::new()
                                            .read(true)
                                            .custom_flags(libc::O_NOFOLLOW)
                                            .open(d.join(name))
                                            .map_err(|_| "Cannot open text fixture")?;
                                        let mut text = Vec::new();
                                        f.take(1024 * 1024 + 1)
                                            .read_to_end(&mut text)
                                            .map_err(|_| "Cannot read text fixture")?;
                                        cliprdr::encode(Format::Text, &text)?;
                                        offered_text = Some(text);
                                    }

                                    data_delay = Duration::from_millis(
                                        q["delay_ms"].as_u64().unwrap_or(0).min(1000),
                                    );
                                    let mut formats = if offered_kind == Format::Png {
                                        vec![ironrdp_cliprdr::pdu::ClipboardFormat::new(cliprdr::PNG_ID).with_name(ironrdp_cliprdr::pdu::ClipboardFormatName::new_static("PNG"))]
                                    } else {
                                        vec![ironrdp_cliprdr::pdu::ClipboardFormat::new(
                                            offered_kind.id(),
                                        )]
                                    };
                                    if offered_text.is_some() {
                                        formats.push(ironrdp_cliprdr::pdu::ClipboardFormat::new(
                                            Format::Text.id(),
                                        ));
                                    }
                                    sender
                                        .send(ServerEvent::Clipboard(
                                            ClipboardMessage::SendInitiateCopy(formats),
                                        ))
                                        .map_err(|_| "Fixture server ended")?;
                                    Ok(())
                                })();
                                let _=std::fs::write(d.join(format!("server-{tab}-response.json")),json!({"seq":seq,"result":{"offered":result.is_ok(),"error":result.err()}}).to_string());
                            }
                        }
                    }
                }
                for e in bridge.drain() {
                    match e {
                        Event::Request(id) => {
                            if delayed.is_none() {
                                request_count += 1;
                                let data = if id == Format::Text.id() && offered_text.is_some() {
                                    cliprdr::encode(Format::Text, offered_text.as_ref().unwrap())
                                        .ok()
                                } else {
                                    cliprdr::encode(offered_kind, &offered).ok()
                                };
                                delayed = Some((tokio::time::Instant::now() + data_delay, data));
                                let _ = std::fs::write(
                                    d.join(format!("server-{tab}-pending.json")),
                                    json!({"requests":request_count}).to_string(),
                                );
                            }
                        }
                        Event::Offer {
                            selected: Some((id, kind)),
                            ..
                        } => {
                            requested = kind;
                            let _ = sender.send(ServerEvent::Clipboard(
                                ClipboardMessage::SendInitiatePaste(id),
                            ));
                        }
                        Event::Data(data) => {
                            received += 1;
                            let result = data.and_then(|b| cliprdr::decode(requested, &b));
                            let receipt = match result {
                                Ok(b) => {
                                    let mut r = json!({"received":received,"mime":requested.mime(),"bytes":b.len(),"sha256":format!("{:x}",Sha256::digest(&b))});
                                    if requested != Format::Text {
                                        if let Ok(mut reader) =
                                            png::Decoder::new(std::io::Cursor::new(&b)).read_info()
                                        {
                                            let mut pixels =
                                                vec![0; reader.output_buffer_size().unwrap_or(0)];
                                            if let Ok(info) = reader.next_frame(&mut pixels) {
                                                r["pixel_sha256"] = json!(format!(
                                                    "{:x}",
                                                    Sha256::digest(&pixels[..info.buffer_size()])
                                                ));
                                                r["width"] = json!(info.width);
                                                r["height"] = json!(info.height);
                                            }
                                        }
                                    }
                                    r
                                }
                                Err(e) => json!({"received":received,"error":e}),
                            };
                            let _ = std::fs::write(
                                d.join(format!("server-{tab}-receipt.json")),
                                receipt.to_string(),
                            );
                        }
                        _ => (),
                    }
                }
                if delayed
                    .as_ref()
                    .is_some_and(|(at, _)| tokio::time::Instant::now() >= *at)
                {
                    let (_, data) = delayed.take().unwrap();
                    let _ = sender.send(ServerEvent::Clipboard(ClipboardMessage::SendFormatData(
                        match data {
                            Some(b) => FormatDataResponse::new_data(b),
                            None => FormatDataResponse::new_error(),
                        },
                    )));
                }
            }
        });
    }
    // Existence announces complete readiness; readers must never observe an empty file.
    fixture_stamp(
        &dir.join("server-ready.json"),
        json!({"ports":ports,"server":"ironrdp-server 0.13.0","security":"TLS verified + CredSSP/NLA","generated_desktop":[320,180]}),
    )?;
    std::future::pending::<()>().await;
    Ok(())
}
