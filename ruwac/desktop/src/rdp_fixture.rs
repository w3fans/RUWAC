//! Disposable authenticated loopback server; test-only, no host accounts or services.
use super::*;
use ironrdp_cliprdr::backend::{ClipboardMessage, CliprdrBackendFactory};
use ironrdp_server::{
    BitmapUpdate, CliprdrServerFactory, Credentials, DesktopSize, DisplayUpdate, PixelFormat,
    RdpServer, RdpServerDisplay, RdpServerDisplayUpdates, ServerEvent, ServerEventSender,
};
use std::num::{NonZeroU16, NonZeroUsize};
struct Display;
struct Updates;
#[async_trait::async_trait]
impl RdpServerDisplay for Display {
    async fn size(&mut self) -> DesktopSize {
        DesktopSize {
            width: 320,
            height: 180,
        }
    }
    async fn updates(&mut self) -> anyhow::Result<Box<dyn RdpServerDisplayUpdates>> {
        Ok(Box::new(Updates))
    }
}
#[async_trait::async_trait]
impl RdpServerDisplayUpdates for Updates {
    async fn next_update(&mut self) -> anyhow::Result<Option<DisplayUpdate>> {
        tokio::time::sleep(Duration::from_millis(100)).await;
        Ok(Some(DisplayUpdate::Bitmap(BitmapUpdate {
            x: 0,
            y: 0,
            width: NonZeroU16::new(320).unwrap(),
            height: NonZeroU16::new(180).unwrap(),
            format: PixelFormat::BgrX32,
            data: bytes::Bytes::from(vec![75; 320 * 180 * 4]),
            stride: NonZeroUsize::new(1280).unwrap(),
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
#[test]
fn authenticated_loopback_framebuffer_and_cliprdr() {
    use std::{
        fs,
        os::unix::fs::{DirBuilderExt, PermissionsExt},
    };
    let root = std::env::temp_dir().join(ruwac_protocol::id("rdp_auth"));
    fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
    let cert = root.join("cert.pem");
    let key = root.join("key.pem");
    assert!(
        std::process::Command::new("openssl")
            .args([
                "req",
                "-x509",
                "-newkey",
                "rsa:2048",
                "-nodes",
                "-days",
                "1",
                "-subj",
                "/CN=RUWAC-fixture",
                "-addext",
                "subjectAltName=IP:127.0.0.1",
                "-keyout"
            ])
            .arg(&key)
            .arg("-out")
            .arg(&cert)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap()
            .success()
    );
    fs::set_permissions(&key, fs::Permissions::from_mode(0o600)).unwrap();
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(tokio::task::LocalSet::new().run_until(async {
        for tab in 0..4 {
            let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
            listener.set_nonblocking(true).unwrap();
            let port = listener.local_addr().unwrap().port();
            let identity = ironrdp_server::TlsIdentityCtx::init_from_paths(&cert, &key).unwrap();
            let (send, recv) = ServerEvent::create_channel();
            let server_bridge = Bridge::default();
            let mut server = RdpServer::builder()
                .with_addr(([127, 0, 0, 1], port))
                .with_hybrid(
                    ironrdp_server::tokio_rustls::TlsAcceptor::from(Arc::new(
                        ironrdp_server::tokio_rustls::rustls::ServerConfig::builder()
                            .with_no_client_auth()
                            .with_single_cert(identity.certs, identity.priv_key)
                            .unwrap(),
                    )),
                    identity.pub_key,
                )
                .with_no_input()
                .with_display_handler(Display)
                .with_cliprdr_factory(Some(Box::new(Factory(server_bridge.clone()))))
                .build();
            server.set_credentials(Some(Credentials {
                username: "fixture".into(),
                password: "generated-local-only".into(),
                domain: None,
            }));
            // Own event channel is already wired by the builder; use its sender rather than recv.
            drop(send);
            drop(recv);
            let server_send = server.event_sender().clone();
            let listener = tokio::net::TcpListener::from_std(listener).unwrap();
            let server_task = tokio::task::spawn_local(async move {
                let (stream, _) = listener.accept().await.unwrap();
                server.run_connection(stream).await
            });
            let state = Arc::new(Mutex::new(State::default()));
            let bridge = state.lock().unwrap().bridge.clone();
            bridge.epoch.store(1, Ordering::Release);
            let halt = Arc::new(AtomicBool::new(false));
            let (tx, rx) = tokio::sync::mpsc::channel(4);
            let mut task = tokio::task::spawn_local(run(
                Profile {
                    desktop_size: (1280, 720),
                    clipboard: true,
                    graphics: false,
                    handover: None,
                    host: "127.0.0.1".into(),
                    port,
                    user: "fixture".into(),
                    reference: String::new(),
                    reconnect: true,
                    ca: Some(cert.to_string_lossy().into_owned()),
                },
                if tab == 3 {
                    "invalid-generated-password".into()
                } else {
                    "generated-local-only".into()
                },
                rx,
                halt.clone(),
                state.clone(),
            ));
            if tab == 3 {
                let result = tokio::time::timeout(Duration::from_secs(5), &mut task)
                    .await
                    .unwrap()
                    .unwrap();
                assert_eq!(
                    result.unwrap_err(),
                    "RDP authentication/session setup failed"
                );
                assert!(state.lock().unwrap().frame.is_none());
                let _ = tokio::time::timeout(Duration::from_secs(3), server_task).await;
                continue;
            }
            let budget = Arc::default();
            let exercise = async {
                let mut ready = false;
                let mut frames = false;
                let mut offered = false;
                let mut round = 0usize;
                let mut response = false;
                let mut reverse = false;
                let text = format!("Fixture {tab} · Živjo Łódź 🦀\n");
                while round < 10 || !reverse || !frames {
                    tokio::time::sleep(Duration::from_millis(10)).await;
                    if state.lock().unwrap().frame.take().is_some() {
                        frames = true;
                    }
                    for e in bridge.drain() {
                        match e {
                            Event::Initialize => {
                                tx.send(
                                    QueueBudget::reserve(&budget, Command::Copy(Vec::new()))
                                        .unwrap(),
                                )
                                .await
                                .unwrap();
                            }
                            Event::Ready => ready = true,
                            Event::Offer {
                                selected: Some((id, _)),
                                ..
                            } => {
                                tx.send(
                                    QueueBudget::reserve(&budget, Command::Request(id)).unwrap(),
                                )
                                .await
                                .unwrap();
                            }
                            Event::Data(data) => {
                                assert_eq!(
                                    crate::cliprdr::decode(Format::Text, &data.unwrap()).unwrap(),
                                    text.as_bytes()
                                );
                                round += 1;
                                response = true;
                            }
                            Event::Request(_) => {
                                tx.send(
                                    QueueBudget::reserve(
                                        &budget,
                                        Command::Reply(Some(
                                            crate::cliprdr::encode(Format::Text, text.as_bytes())
                                                .unwrap(),
                                        )),
                                    )
                                    .unwrap(),
                                )
                                .await
                                .unwrap();
                            }
                            _ => (),
                        }
                    }
                    for e in server_bridge.drain() {
                        match e {
                            Event::Request(_) => {
                                server_send
                                    .send(ServerEvent::Clipboard(ClipboardMessage::SendFormatData(
                                        FormatDataResponse::new_data(
                                            crate::cliprdr::encode(Format::Text, text.as_bytes())
                                                .unwrap(),
                                        ),
                                    )))
                                    .unwrap();
                            }
                            Event::Offer {
                                selected: Some((id, _)),
                                ..
                            } => {
                                server_send
                                    .send(ServerEvent::Clipboard(
                                        ClipboardMessage::SendInitiatePaste(id),
                                    ))
                                    .unwrap();
                            }
                            Event::Data(data) => {
                                assert_eq!(
                                    crate::cliprdr::decode(Format::Text, &data.unwrap()).unwrap(),
                                    text.as_bytes()
                                );
                                reverse = true;
                            }
                            _ => (),
                        }
                    }
                    if ready && (!offered || response) && round < 10 {
                        server_send
                            .send(ServerEvent::Clipboard(ClipboardMessage::SendInitiateCopy(
                                crate::cliprdr::formats(true, false),
                            )))
                            .unwrap();
                        offered = true;
                        response = false;
                    }
                    if round == 10 && !reverse {
                        tx.send(
                            QueueBudget::reserve(
                                &budget,
                                Command::Copy(crate::cliprdr::formats(true, false)),
                            )
                            .unwrap(),
                        )
                        .await
                        .unwrap();
                        round += 1;
                    }
                    if task.is_finished() {
                        panic!(
                            "RDP transport ended before acceptance: {:?}",
                            (&mut task).await.unwrap()
                        );
                    }
                }
            };
            tokio::time::timeout(Duration::from_secs(20), exercise)
                .await
                .expect("Authenticated fixture/frame/CLIPRDR deadline");
            halt.store(true, Ordering::Relaxed);
            let result = task.await.unwrap();
            assert!(result.is_ok(), "{result:?}");
            server_send
                .send(ServerEvent::Quit("fixture complete".into()))
                .ok();
            let _ = server_task.await;
        }
    }));
    fs::remove_dir_all(root).unwrap();
}
