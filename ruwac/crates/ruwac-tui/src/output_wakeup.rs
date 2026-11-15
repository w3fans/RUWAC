//! Optional output hints; the ordinary render deadlines remain the fallback.
use ruwac_protocol::{read_frame_with_limit, request, write_frame};
use serde_json::json;
use std::{
    io,
    net::Shutdown,
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::{ffi::OsStrExt, net::UnixStream},
    },
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU8, Ordering},
        mpsc::SyncSender,
    },
    thread::JoinHandle,
};

#[derive(Debug)]
pub(super) enum Ingress {
    Bytes(Vec<u8>),
    End,
    Output,
}

#[derive(Default)]
struct State {
    remaining: AtomicU8,
    pending: AtomicBool,
}

#[derive(Default)]
pub(super) struct OutputWake {
    state: Arc<State>,
    control: Option<UnixStream>,
    reader: Option<JoinHandle<()>>,
}

impl OutputWake {
    pub(super) fn new(socket: &Path, tx: SyncSender<Ingress>) -> Self {
        match connect(socket).and_then(super::verified_stream) {
            Ok(stream) => Self::from_stream(stream, tx),
            Err(_) => Self::default(),
        }
    }

    fn from_stream(mut stream: UnixStream, tx: SyncSender<Ingress>) -> Self {
        let Ok(control) = stream.try_clone() else {
            return Self::default();
        };
        let state = Arc::new(State::default());
        let worker = state.clone();
        let reader = std::thread::Builder::new()
            .name("ruwac-output".into())
            .spawn(move || {
                let _ = read_events(&mut stream, &worker, tx);
                // Close even while the main thread still owns the control clone.
                let _ = stream.shutdown(Shutdown::Both);
            });
        match reader {
            Ok(reader) => Self {
                state,
                control: Some(control),
                reader: Some(reader),
            },
            Err(_) => {
                let _ = control.shutdown(Shutdown::Both);
                Self::default()
            }
        }
    }

    pub(super) fn arm(&self) {
        // A daemon revision also covers unrelated background panes. Bound the
        // extra render work per input batch instead of following every revision
        // throughout the recent-input window. Polling remains the fallback.
        self.state.remaining.store(2, Ordering::Relaxed);
    }
    pub(super) fn disarm(&self) {
        self.state.remaining.store(0, Ordering::Relaxed);
    }
    pub(super) fn received(&self) {
        self.state.pending.store(false, Ordering::Relaxed);
    }
}

impl Drop for OutputWake {
    fn drop(&mut self) {
        if let Some(stream) = &self.control {
            let _ = stream.shutdown(Shutdown::Both);
        }
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}

fn connect(path: &Path) -> io::Result<UnixStream> {
    super::verified_path(path)?;
    let bytes = path.as_os_str().as_bytes();
    // The zeroed address retains its terminating NUL after the bounded pathname.
    let mut address: libc::sockaddr_un = unsafe { std::mem::zeroed() };
    if bytes.is_empty() || bytes.contains(&0) || bytes.len() >= address.sun_path.len() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid socket path",
        ));
    }
    address.sun_family = libc::AF_UNIX as libc::sa_family_t;
    for (out, byte) in address.sun_path.iter_mut().zip(bytes) {
        *out = *byte as libc::c_char;
    }
    // An auxiliary connection must never wait for a full listen backlog.
    let fd = unsafe {
        libc::socket(
            libc::AF_UNIX,
            libc::SOCK_STREAM | libc::SOCK_CLOEXEC | libc::SOCK_NONBLOCK,
            0,
        )
    };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // Take ownership immediately, including every subsequent error path.
    let stream = unsafe { UnixStream::from_raw_fd(fd) };
    let length = std::mem::offset_of!(libc::sockaddr_un, sun_path) + bytes.len() + 1;
    if unsafe {
        libc::connect(
            stream.as_raw_fd(),
            (&address as *const libc::sockaddr_un).cast(),
            length as libc::socklen_t,
        )
    } != 0
    {
        return Err(io::Error::last_os_error());
    }
    stream.set_nonblocking(false)?;
    Ok(stream)
}

fn read_events(stream: &mut UnixStream, state: &State, tx: SyncSender<Ingress>) -> io::Result<()> {
    let sent = request("events.subscribe", json!({}));
    write_frame(stream, &sent)?;
    let answer = read_frame_with_limit(stream, 4096)?;
    if answer["version"] != 0 || answer["id"] != sent.id || answer["result"]["subscribed"] != true {
        return Err(io::Error::other("output subscription unavailable"));
    }
    // Shutdown on the control clone cancels this read; no idle timer is needed.
    stream.set_read_timeout(None)?;
    loop {
        let event = read_frame_with_limit(stream, 4096)?;
        if event["version"] != 0 {
            return Err(io::Error::other("output event version rejected"));
        }
        if event["event"] == "state.changed"
            && event["revision"].as_u64().is_some()
            && state.remaining.load(Ordering::Relaxed) > 0
            && !state.pending.swap(true, Ordering::Relaxed)
        {
            let permitted = state
                .remaining
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_sub(1))
                .is_ok();
            if !permitted || tx.try_send(Ingress::Output).is_err() {
                // A full keyboard queue never blocks or loses keyboard bytes.
                state.pending.store(false, Ordering::Relaxed);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{Read, Write},
        sync::mpsc,
        time::{Duration, Instant},
    };

    fn ack(peer: &mut UnixStream) {
        let req = read_frame_with_limit(peer, 4096).unwrap();
        assert_eq!(req["method"], "events.subscribe");
        write_frame(
            peer,
            &json!({"version":0,"id":req["id"],"result":{"subscribed":true}}),
        )
        .unwrap();
    }
    fn hint(peer: &mut UnixStream, n: u64) {
        write_frame(
            peer,
            &json!({"version":0,"event":"state.changed","revision":n}),
        )
        .unwrap();
    }
    fn finished(wake: &OutputWake) {
        let end = Instant::now() + Duration::from_secs(2);
        while !wake.reader.as_ref().unwrap().is_finished() {
            assert!(Instant::now() < end, "notification reader did not finish");
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    #[test]
    fn input_batch_allows_only_two_extra_wakes_until_rearmed() {
        let (stream, mut peer) = UnixStream::pair().unwrap();
        let (tx, rx) = mpsc::sync_channel(2);
        let (go, wait) = mpsc::channel();
        let server = std::thread::spawn(move || {
            ack(&mut peer);
            for n in 0..3 {
                wait.recv().unwrap();
                hint(&mut peer, n);
            }
        });
        let wake = OutputWake::from_stream(stream, tx);
        wake.arm();
        for _ in 0..2 {
            go.send(()).unwrap();
            assert!(matches!(
                rx.recv_timeout(Duration::from_secs(2)),
                Ok(Ingress::Output)
            ));
            wake.received();
        }
        go.send(()).unwrap();
        server.join().unwrap();
        finished(&wake);
        assert!(rx.try_recv().is_err());
        assert_eq!(wake.state.remaining.load(Ordering::Relaxed), 0);
        wake.arm();
        assert_eq!(wake.state.remaining.load(Ordering::Relaxed), 2);
        wake.disarm();
        assert_eq!(wake.state.remaining.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn repeated_notifications_rearm_after_the_main_loop_receives_them() {
        let (stream, mut peer) = UnixStream::pair().unwrap();
        let (tx, rx) = mpsc::sync_channel(2);
        let (go, wait) = mpsc::channel();
        let server = std::thread::spawn(move || {
            ack(&mut peer);
            for n in 1..=2 {
                wait.recv().unwrap();
                hint(&mut peer, n);
            }
        });
        let wake = OutputWake::from_stream(stream, tx);
        wake.arm();
        for _ in 0..2 {
            go.send(()).unwrap();
            assert!(matches!(
                rx.recv_timeout(Duration::from_secs(2)),
                Ok(Ingress::Output)
            ));
            wake.received();
        }
        server.join().unwrap();
        finished(&wake);
    }

    #[test]
    fn quiet_output_does_not_enqueue_hints_and_bursts_coalesce() {
        for active in [false, true] {
            let (stream, mut peer) = UnixStream::pair().unwrap();
            let (tx, rx) = mpsc::sync_channel(2);
            let (go, wait) = mpsc::channel();
            let server = std::thread::spawn(move || {
                ack(&mut peer);
                wait.recv().unwrap();
                for n in 0..1000 {
                    hint(&mut peer, n);
                }
            });
            let wake = OutputWake::from_stream(stream, tx);
            if active {
                wake.arm();
            }
            go.send(()).unwrap();
            server.join().unwrap();
            finished(&wake);
            if active {
                assert!(matches!(rx.try_recv(), Ok(Ingress::Output)));
            }
            assert!(rx.try_recv().is_err());
        }
    }

    #[test]
    fn full_input_queue_preserves_keyboard_bytes_without_blocking_reader() {
        let (stream, mut peer) = UnixStream::pair().unwrap();
        let (tx, rx) = mpsc::sync_channel(2);
        tx.send(Ingress::Bytes(vec![b'a'])).unwrap();
        tx.send(Ingress::Bytes(vec![b'b'])).unwrap();
        let (go, wait) = mpsc::channel();
        let server = std::thread::spawn(move || {
            ack(&mut peer);
            wait.recv().unwrap();
            for n in 0..1000 {
                hint(&mut peer, n);
            }
        });
        let wake = OutputWake::from_stream(stream, tx);
        wake.arm();
        go.send(()).unwrap();
        server.join().unwrap();
        finished(&wake);
        for byte in [b'a', b'b'] {
            assert!(matches!(rx.try_recv(), Ok(Ingress::Bytes(v)) if v == [byte]));
        }
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn rejected_subscription_closes_peer_while_control_clone_is_alive() {
        for invalid in 0..3 {
            let (stream, mut peer) = UnixStream::pair().unwrap();
            peer.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
            let (tx, rx) = mpsc::sync_channel(2);
            let (closed, wait) = mpsc::channel();
            let server = std::thread::spawn(move || {
                let req = read_frame_with_limit(&mut peer, 4096).unwrap();
                let answer = match invalid {
                    0 => json!({"version":0,"id":req["id"],"error":{"code":"METHOD_NOT_FOUND"}}),
                    1 => json!({"version":1,"id":req["id"],"result":{"subscribed":true}}),
                    _ => json!({"version":0,"id":{"wrong":true},"result":{"subscribed":true}}),
                };
                write_frame(&mut peer, &answer).unwrap();
                assert_eq!(peer.read(&mut [0]).unwrap(), 0);
                closed.send(()).unwrap();
            });
            let wake = OutputWake::from_stream(stream, tx);
            wake.arm();
            wait.recv_timeout(Duration::from_secs(2)).unwrap();
            finished(&wake);
            assert!(rx.try_recv().is_err());
            server.join().unwrap();
        }
    }

    #[test]
    fn oversized_notification_header_is_rejected_without_reading_its_body() {
        let (stream, mut peer) = UnixStream::pair().unwrap();
        peer.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        let (tx, _rx) = mpsc::sync_channel(2);
        let server = std::thread::spawn(move || {
            ack(&mut peer);
            peer.write_all(&9000u32.to_be_bytes()).unwrap();
            assert_eq!(peer.read(&mut [0]).unwrap(), 0);
        });
        let wake = OutputWake::from_stream(stream, tx);
        server.join().unwrap();
        finished(&wake);
    }

    #[test]
    fn drop_cancels_a_reader_waiting_for_the_subscription_ack() {
        let (stream, mut peer) = UnixStream::pair().unwrap();
        peer.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        let (tx, _rx) = mpsc::sync_channel(2);
        let (ready, wait) = mpsc::channel();
        let server = std::thread::spawn(move || {
            read_frame_with_limit(&mut peer, 4096).unwrap();
            ready.send(()).unwrap();
            assert_eq!(peer.read(&mut [0]).unwrap(), 0);
        });
        let wake = OutputWake::from_stream(stream, tx);
        wait.recv_timeout(Duration::from_secs(2)).unwrap();
        let began = Instant::now();
        drop(wake);
        assert!(began.elapsed() < Duration::from_secs(1));
        server.join().unwrap();
    }

    #[test]
    fn optional_connection_does_not_wait_for_a_full_listen_backlog() {
        use std::os::unix::{
            fs::{DirBuilderExt, PermissionsExt},
            net::UnixListener,
        };
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("ruwac-wake-{}-{unique}", std::process::id()));
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&root)
            .unwrap();
        let path = root.join("control.sock");
        let listener = UnixListener::bind(&path).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert_eq!(unsafe { libc::listen(listener.as_raw_fd(), 0) }, 0);
        let _first = UnixStream::connect(&path).unwrap();
        let (release, wait) = mpsc::channel();
        let server = std::thread::spawn(move || {
            let _ = wait.recv_timeout(Duration::from_secs(2));
            let _ = listener.accept().unwrap();
        });
        let (tx, _rx) = mpsc::sync_channel(2);
        let began = Instant::now();
        let wake = OutputWake::new(&path, tx);
        let elapsed = began.elapsed();
        release.send(()).unwrap();
        server.join().unwrap();
        assert!(wake.reader.is_none());
        assert!(elapsed < Duration::from_secs(1));
        drop(wake);
        std::fs::remove_dir_all(root).unwrap();
    }
}
