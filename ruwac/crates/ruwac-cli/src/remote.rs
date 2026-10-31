//! Authenticated OpenSSH transport. Request data never enters the remote shell command.
use ruwac_protocol::{MAX_FRAME, Request};
use serde_json::Value;
use std::{
    io::{self, Read, Write},
    path::PathBuf,
    process::{Command, Stdio},
    sync::mpsc,
    time::Duration,
};

pub struct Remote {
    pub host: String,
    pub binary: String,
    pub socket: Option<PathBuf>,
}
fn quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}
impl Remote {
    pub fn validate(&self) -> io::Result<()> {
        if self.host.is_empty()
            || self.host.len() > 255
            || self.host.starts_with('-')
            || !self
                .host
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._-@[]:".contains(&b))
            || self.binary.is_empty()
            || self.binary.starts_with('-')
            || self.binary.chars().any(char::is_control)
            || self.socket.as_ref().is_some_and(|p| {
                p.to_str()
                    .is_none_or(|s| s.is_empty() || s.chars().any(char::is_control))
            })
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid SSH destination, binary or socket",
            ));
        }
        Ok(())
    }
    fn command(&self, tty: bool, args: &[&str]) -> Command {
        let mut words = vec![self.binary.clone()];
        if let Some(socket) = &self.socket {
            words.extend(["--socket".into(), socket.to_string_lossy().into()]);
        }
        words.extend(args.iter().map(|s| s.to_string()));
        let mut c = Command::new("ssh");
        c.args([
            if tty { "-tt" } else { "-T" },
            "-o",
            "BatchMode=yes",
            "-o",
            "StrictHostKeyChecking=yes",
            "-o",
            "ConnectTimeout=10",
            "-o",
            "ServerAliveInterval=15",
            "-o",
            "ServerAliveCountMax=2",
            "-o",
            "ForwardAgent=no",
            "-o",
            "ClearAllForwardings=yes",
            "--",
        ])
        .arg(&self.host)
        .arg(words.iter().map(|s| quote(s)).collect::<Vec<_>>().join(" "));
        c
    }
    fn exchange(&self, command: &[&str], input: Vec<u8>) -> io::Result<Value> {
        self.exchange_bounded(command, input, MAX_FRAME)
    }
    fn exchange_bounded(
        &self,
        command: &[&str],
        input: Vec<u8>,
        input_cap: usize,
    ) -> io::Result<Value> {
        if input.len() > input_cap {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "request exceeds 1 MiB",
            ));
        }
        let mut child = self
            .command(false, command)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()?;
        let mut stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let (tx, rx) = mpsc::sync_channel(1);
        // Reading and writing run together so a rejecting peer cannot deadlock a large request.
        let writer = std::thread::spawn(move || stdin.write_all(&input));
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            let result = stdout
                .take((MAX_FRAME + 1) as u64)
                .read_to_end(&mut bytes)
                .map(|_| bytes);
            let _ = tx.send(result);
        });
        let output = match rx.recv_timeout(Duration::from_secs(70)) {
            Ok(v) => v,
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "SSH response exceeded 70 seconds",
                ));
            }
        };
        let bytes = match output {
            Ok(v) => v,
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(e);
            }
        };
        if bytes.len() > MAX_FRAME {
            let _ = child.kill();
            let _ = child.wait();
            return Err(io::Error::other("SSH response exceeds 1 MiB"));
        }
        // EOF need not mean process termination: keep the same overall deadline bounded.
        let end = std::time::Instant::now() + Duration::from_secs(2);
        let status = loop {
            if let Some(s) = child.try_wait()? {
                break s;
            }
            if std::time::Instant::now() >= end {
                let _ = child.kill();
                let _ = child.wait();
                return Err(io::Error::other("SSH peer did not exit after response"));
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        let write = writer
            .join()
            .map_err(|_| io::Error::other("SSH input worker failed"))?;
        let value: Value = serde_json::from_slice(&bytes)
            .map_err(|_| io::Error::other("SSH peer returned no valid RUWAC JSON response"))?;
        if !status.success() && value.get("error").is_none() {
            return Err(io::Error::other("SSH command failed"));
        }
        if value.get("error").is_none() {
            write?;
        }
        Ok(value)
    }
    pub fn image_transfer(&self, input: Vec<u8>) -> io::Result<Value> {
        self.exchange_bounded(
            &["image-receive"],
            input,
            ruwac_clipboard::MAX_IMAGE_BYTES + 8196,
        )
    }
    pub fn rpc(&self, request: &Request) -> io::Result<Value> {
        let value = self.exchange(&["request"], serde_json::to_vec(request)?)?;
        if value["version"] != 0 || value["id"] != request.id {
            return Err(io::Error::other("remote protocol/correlation rejected"));
        }
        if value.get("error").is_some() {
            println!("{value}");
            return Err(io::Error::other("remote request failed"));
        }
        Ok(value)
    }
    pub fn start(&self) -> io::Result<Value> {
        let v = self.exchange(&["start"], Vec::new())?;
        if v["version"] != 0 || v["result"]["ready"] != true {
            return Err(io::Error::other("remote startup response rejected"));
        }
        Ok(v)
    }
    pub fn shell_argv(&self) -> io::Result<Vec<String>> {
        let value = self.exchange(&["shell-argv"], Vec::new()).map_err(|_| {
            io::Error::other(
                "Remote login-shell discovery failed; update the remote RUWAC CLI or use raw RPC with explicit remote argv",
            )
        })?;
        let argv = value["result"]["argv"].as_array().filter(|a| a.len() == 2);
        if value["version"] != 0 || value.get("error").is_some() {
            return Err(io::Error::other("Remote login-shell response rejected"));
        }
        let Some(argv) = argv else {
            return Err(io::Error::other("Remote login-shell argv rejected"));
        };
        let shell = argv[0].as_str().filter(|s| {
            s.len() <= 4096
                && std::path::Path::new(s).is_absolute()
                && !s.chars().any(char::is_control)
        });
        if argv[1] != "-l" || shell.is_none() {
            return Err(io::Error::other("Remote login-shell argv rejected"));
        }
        Ok(vec![shell.unwrap().into(), "-l".into()])
    }
    pub fn gui(&self, request: &Request) -> io::Result<Value> {
        let value = self.exchange(&["gui-request"], serde_json::to_vec(request)?)?;
        if value["version"] != 0 || value["id"] != request.id {
            return Err(io::Error::other("remote GUI correlation rejected"));
        }
        Ok(value)
    }
    pub fn doctor(&self, args: &[String]) -> io::Result<Value> {
        let mut command = vec!["doctor"];
        command.extend(args.iter().map(String::as_str));
        let v = self.exchange(&command, Vec::new())?;
        if v["version"] != 0 || !v["result"]["ok"].is_boolean() || !v["result"]["checks"].is_array()
        {
            return Err(io::Error::other("remote diagnostics response rejected"));
        }
        Ok(v)
    }
    pub fn attach(&self, session: &str) -> io::Result<()> {
        self.attach_with_options(session, None, false)
    }
    pub fn attach_with_options(
        &self,
        session: &str,
        window: Option<&str>,
        viewer: bool,
    ) -> io::Result<()> {
        if !ruwac_protocol::valid_id(session, "session")
            || window.is_some_and(|w| !ruwac_protocol::valid_id(w, "window"))
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid session ID",
            ));
        }
        if viewer {
            let caps = self.rpc(&ruwac_protocol::request(
                "capabilities",
                serde_json::json!({}),
            ))?;
            if caps["result"]["non_sizing_viewer"] != 1 {
                return Err(io::Error::other(
                    "Remote daemon cannot attach a non-sizing viewer; update it or choose an ordinary attachment explicitly",
                ));
            }
        }
        let mut args = vec!["attach", "--session", session];
        if let Some(window) = window {
            args.extend(["--window", window]);
        }
        if viewer {
            args.push("--viewer");
        }
        let status = self.command(true, &args).status()?;
        if status.success() {
            Ok(())
        } else {
            Err(io::Error::other("remote terminal attachment failed"))
        }
    }
}
