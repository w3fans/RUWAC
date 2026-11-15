pub mod attachment;
pub mod image_offer;
pub mod shell;
use portable_pty::{Child, CommandBuilder, MasterPty, PtySize};
use std::{
    io::{Read, Write},
    os::fd::{AsRawFd, FromRawFd, OwnedFd},
    sync::{Arc, Mutex, mpsc},
    thread,
};
pub struct Pty {
    pub pid: u32,
    master: Box<dyn MasterPty + Send>,
    input: Option<mpsc::SyncSender<Vec<u8>>>,
    child: Arc<Mutex<Box<dyn Child + Send + Sync>>>,
}
impl Pty {
    pub fn spawn(
        argv: &[String],
        rows: u16,
        cols: u16,
        output: impl FnMut(Vec<u8>) + Send + 'static,
    ) -> Result<Self, String> {
        if argv.is_empty() || argv.iter().any(|s| s.contains('\0')) {
            return Err("argv must be a nonempty NUL-free array".into());
        }
        let pair = portable_pty::native_pty_system()
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| e.to_string())?;
        let mut reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
        // Keep a separately owned descriptor for readiness checks. The master's
        // original descriptor may close/reuse when a pane is dropped.
        let descriptor = pair
            .master
            .as_raw_fd()
            .ok_or("PTY descriptor unavailable")?;
        let descriptor = unsafe { libc::fcntl(descriptor, libc::F_DUPFD_CLOEXEC, 0) };
        if descriptor < 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        let readiness = unsafe { OwnedFd::from_raw_fd(descriptor) };
        let mut writer = pair.master.take_writer().map_err(|e| e.to_string())?;
        let mut cmd = CommandBuilder::new(&argv[0]);
        cmd.args(&argv[1..]);
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");
        // The daemon may have been started by a noninteractive, colourless
        // launcher. A new interactive terminal has its own colour capability.
        // Explicit child commands can still opt out with `env NO_COLOR=1 ...`.
        cmd.env_remove("NO_COLOR");
        cmd.env_remove("BASH_ENV");
        cmd.env_remove("ENV");
        // Checkpoint selection belongs to the engine/frontend that starts it.
        // A shell launching an independent or test daemon must not reuse this
        // engine's checkpoint. Keep RUWAC_SOCKET for ordinary client routing.
        cmd.env_remove("RUWAC_STATE_FILE");
        let child = pair.slave.spawn_command(cmd).map_err(|e| e.to_string())?;
        let pid = child.process_id().ok_or("child PID unavailable")?;
        drop(pair.slave);
        let (tx, rx) = mpsc::sync_channel::<Vec<u8>>(64);
        thread::spawn(move || {
            while let Ok(b) = rx.recv() {
                if writer.write_all(&b).and_then(|_| writer.flush()).is_err() {
                    break;
                }
            }
        });
        let child = Arc::new(Mutex::new(child));
        let reader_child = child.clone();
        thread::spawn(move || {
            let mut output = output;
            let mut b = [0; 8192];
            let mut drained = 0usize;
            loop {
                let exited = reader_child
                    .lock()
                    .unwrap()
                    .try_wait()
                    .ok()
                    .flatten()
                    .is_some();
                let mut poll = libc::pollfd {
                    fd: readiness.as_raw_fd(),
                    events: libc::POLLIN,
                    revents: 0,
                };
                let ready = unsafe { libc::poll(&mut poll, 1, if exited { 0 } else { 100 }) };
                if ready < 0 {
                    if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                        continue;
                    }
                    if exited {
                        break;
                    }
                    thread::sleep(std::time::Duration::from_millis(100));
                    continue;
                }
                if ready == 0 {
                    if exited {
                        break;
                    }
                    continue;
                }
                let n = match reader.read(&mut b) {
                    Ok(n) => n,
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => continue,
                    Err(_) => 0,
                };
                if n == 0 {
                    if exited {
                        break;
                    }
                    // A live process can close and later reopen its terminal.
                    // Retry without spinning on POLLHUP or declaring it ended.
                    thread::sleep(std::time::Duration::from_millis(100));
                    continue;
                }
                output(b[..n].to_vec());
                if exited {
                    drained += n;
                    // An inherited writer must not keep a pane alive forever
                    // by continuously producing new output.
                    if drained >= 8 * 1024 * 1024 {
                        break;
                    }
                }
            }
            // One reader serializes all output and reports end only after its
            // final drain. No callback can publish data after this event.
            output(Vec::new());
        });
        Ok(Self {
            pid,
            master: pair.master,
            input: Some(tx),
            child,
        })
    }
    pub fn foreground_group(&self) -> Option<i32> {
        self.master.process_group_leader()
    }
    pub fn send(&self, b: Vec<u8>) -> Result<(), String> {
        self.input
            .as_ref()
            .ok_or("pane closed")?
            .try_send(b)
            .map_err(|e| e.to_string())
    }
    pub fn resize(&self, rows: u16, cols: u16) -> Result<(), String> {
        self.master
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| e.to_string())
    }
    pub fn exit_code(&self) -> Option<u32> {
        self.child
            .lock()
            .unwrap()
            .try_wait()
            .ok()
            .flatten()
            .map(|s| s.exit_code())
    }
    pub fn exited(&self) -> bool {
        self.child
            .lock()
            .unwrap()
            .try_wait()
            .ok()
            .flatten()
            .is_some()
    }
}
impl Drop for Pty {
    fn drop(&mut self) {
        self.input.take();
        let child = self.child.clone();
        let pid = self.pid;
        let fg = self
            .master
            .process_group_leader()
            .filter(|fg| unsafe { libc::getsid(*fg) == pid as i32 && libc::getpgid(*fg) == *fg });
        let mut c = child.lock().unwrap();
        if c.try_wait().ok().flatten().is_none() {
            // Only the process group established by this owned PTY child is signalled.
            unsafe {
                if let Some(fg) = fg {
                    libc::kill(-fg, libc::SIGTERM);
                }
                if libc::getpgid(pid as i32) == pid as i32 {
                    libc::kill(-(pid as i32), libc::SIGTERM);
                } else {
                    let _ = c.kill();
                }
            }
        }
        drop(c);
        thread::spawn(move || {
            for _ in 0..20 {
                if child.lock().unwrap().try_wait().ok().flatten().is_some() {
                    break;
                }
                thread::sleep(std::time::Duration::from_millis(25));
            }
            let mut c = child.lock().unwrap();
            unsafe {
                if let Some(fg) = fg {
                    if libc::getsid(fg) == pid as i32 && libc::getpgid(fg) == fg {
                        libc::kill(-fg, libc::SIGKILL);
                    }
                }
            }
            if c.try_wait().ok().flatten().is_none() {
                unsafe {
                    if libc::getpgid(pid as i32) == pid as i32 {
                        libc::kill(-(pid as i32), libc::SIGKILL);
                    } else {
                        let _ = c.kill();
                    }
                }
            }
            let _ = c.wait();
        });
    }
}

#[cfg(test)]
mod output_drain_tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn drains_final_bytes_before_one_end_and_ignores_inherited_writer() {
        let (events, received) = mpsc::channel();
        let mut first = true;
        let pty = Pty::spawn(
            &[
                "/usr/bin/python3".into(),
                "-c".into(),
                "import os,signal,time; signal.signal(signal.SIGHUP,signal.SIG_IGN); os.write(1,b'A'*4096); child=os.fork(); (time.sleep(3),os.write(1,b'LATE_BACKGROUND'),os._exit(0)) if child==0 else None; os.write(1,b'B'*4096+b'FINAL'); os._exit(0)".into(),
            ],
            24,
            80,
            move |data| {
                if first && !data.is_empty() {
                    first = false;
                    // Model backpressure from the bounded daemon actor queue.
                    thread::sleep(Duration::from_millis(700));
                }
                let _ = events.send(data);
            },
        ).unwrap();
        let start = Instant::now();
        let mut chunks = Vec::new();
        let mut end_time = None;
        while start.elapsed() < Duration::from_secs(4) {
            match received.recv_timeout(Duration::from_millis(100)) {
                Ok(data) => {
                    if data.is_empty() && end_time.is_none() {
                        end_time = Some(start.elapsed());
                    }
                    chunks.push(data);
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
                Err(mpsc::RecvTimeoutError::Timeout) => (),
            }
        }
        assert!(pty.exited());
        assert!(
            end_time.is_some_and(|t| t < Duration::from_secs(2)),
            "The pane must end while its descendant still holds the slave PTY"
        );
        let ends: Vec<_> = chunks
            .iter()
            .enumerate()
            .filter_map(|(i, b)| b.is_empty().then_some(i))
            .collect();
        assert_eq!(ends.len(), 1, "Exactly one terminal-end event is required");
        assert_eq!(
            ends[0] + 1,
            chunks.len(),
            "Output arrived after the terminal-end event"
        );
        let data = chunks[..ends[0]].concat();
        let mut expected = vec![b'A'; 4096];
        expected.extend(vec![b'B'; 4096]);
        expected.extend_from_slice(b"FINAL");
        assert_eq!(
            data, expected,
            "The complete child's final bytes must precede end"
        );
    }
    #[test]
    fn closing_standard_streams_waits_for_the_own_process_status() {
        let (events, received) = mpsc::channel();
        let pty = Pty::spawn(&[
            "/usr/bin/python3".into(), "-c".into(),
            "import os,time; os.close(0); os.close(1); os.close(2); time.sleep(.5); os._exit(7)".into(),
        ],24,80,move |data| {let _=events.send(data);}).unwrap();
        assert!(
            matches!(
                received.recv_timeout(Duration::from_millis(200)),
                Err(mpsc::RecvTimeoutError::Timeout)
            ),
            "Closed stdio must not be reported as process exit"
        );
        assert!(
            received
                .recv_timeout(Duration::from_secs(2))
                .unwrap()
                .is_empty()
        );
        assert_eq!(pty.exit_code(), Some(7));
        assert!(matches!(
            received.recv_timeout(Duration::from_millis(200)),
            Err(mpsc::RecvTimeoutError::Disconnected)
        ));
    }

    #[test]
    fn final_bursts_of_different_sizes_are_complete_before_end() {
        for size in [1usize, 8191, 8192, 65537, 1024 * 1024] {
            let (events, received) = mpsc::channel();
            let pty=Pty::spawn(&[
                "/usr/bin/python3".into(),"-c".into(),
                format!("import sys; sys.stdout.buffer.write(b'X'*{size}+b'FINAL'); sys.stdout.buffer.flush()"),
            ],24,80,move |data|{let _=events.send(data);}).unwrap();
            let mut data = Vec::new();
            loop {
                let chunk = received
                    .recv_timeout(Duration::from_secs(2))
                    .expect("Child output/end deadline");
                if chunk.is_empty() {
                    break;
                }
                data.extend(chunk);
            }
            assert_eq!(data.len(), size + 5);
            assert!(data[..size].iter().all(|b| *b == b'X'));
            assert_eq!(&data[size..], b"FINAL");
            assert_eq!(pty.exit_code(), Some(0));
            assert!(matches!(
                received.recv_timeout(Duration::from_millis(200)),
                Err(mpsc::RecvTimeoutError::Disconnected)
            ));
        }
    }

    #[test]
    fn a_continuous_inherited_writer_cannot_keep_the_pane_alive() {
        let (events, received) = mpsc::channel();
        let pty = Pty::spawn(
            &[
                "/usr/bin/python3".into(),
                "-c".into(),
                r#"import os,signal
signal.signal(signal.SIGHUP,signal.SIG_IGN)
if os.fork()!=0:
    os._exit(0)
try:
    while True:
        os.write(1,b'Z'*8192)
except OSError:
    os._exit(0)
"#
                .into(),
            ],
            24,
            80,
            move |data| {
                let _ = events.send(data);
            },
        )
        .unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let data = received
                .recv_timeout(left)
                .expect("Continuous orphan writer did not end the pane");
            if data.is_empty() {
                break;
            }
        }
        assert_eq!(pty.exit_code(), Some(0));
        assert!(matches!(
            received.recv_timeout(Duration::from_millis(200)),
            Err(mpsc::RecvTimeoutError::Disconnected)
        ));
        drop(pty);
    }
    #[test]
    fn temporarily_closed_standard_streams_can_resume_output() {
        let (events, received) = mpsc::channel();
        let pty=Pty::spawn(&[
            "/usr/bin/python3".into(),"-c".into(),
            "import os,time; os.close(0); os.close(1); os.close(2); time.sleep(.2); fd=os.open('/dev/tty',os.O_WRONLY); os.write(fd,b'REOPENED'); os.close(fd); os._exit(9)".into(),
        ],24,80,move |data|{let _=events.send(data);}).unwrap();
        let mut output = Vec::new();
        loop {
            let chunk = received.recv_timeout(Duration::from_secs(2)).unwrap();
            if chunk.is_empty() {
                break;
            }
            output.extend(chunk);
        }
        assert_eq!(pty.exit_code(), Some(9));
        assert_eq!(output, b"REOPENED");
        assert!(matches!(
            received.recv_timeout(Duration::from_millis(200)),
            Err(mpsc::RecvTimeoutError::Disconnected)
        ));
    }
}
