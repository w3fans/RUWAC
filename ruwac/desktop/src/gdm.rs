//! Local GDM recognition. Geometry is evidence; credentials never enter OCR.
use serde::Deserialize;
use std::{
    fs,
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
    os::unix::process::CommandExt,
    path::PathBuf,
    process::{Command, Stdio},
    sync::mpsc,
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Users,
    Username,
    Password,
    Unknown,
    Ambiguous,
    Unavailable,
}
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Observation {
    pub kind: Kind,
    pub rect: Option<[u16; 4]>,
}
impl Observation {
    pub fn validate(&self, w: u16, h: u16) -> bool {
        match (self.kind, self.rect) {
            (Kind::Users | Kind::Username | Kind::Password, Some([x, y, r, b])) => {
                x < r && y < b && r <= w && b <= h
            }
            (Kind::Unknown | Kind::Ambiguous | Kind::Unavailable, None) => true,
            _ => false,
        }
    }
    pub fn center(&self) -> Option<(u16, u16)> {
        let [x, y, r, b] = self.rect?;
        Some((x + r.checked_sub(x)? / 2, y + b.checked_sub(y)? / 2))
    }
}
/// `trusted`: this sign-in already passed a recognized login screen, so
/// fields are accepted without the Fedora logo.
pub fn recognize(
    w: u16,
    h: u16,
    pixels: Vec<u8>,
    trusted: bool,
) -> mpsc::Receiver<Result<Observation, String>> {
    let (tx, rx) = mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let _ = tx.send(run(w, h, pixels, trusted));
    });
    rx
}
fn run(w: u16, h: u16, pixels: Vec<u8>, trusted: bool) -> Result<Observation, String> {
    let failed = || "GNOME recognition unavailable; no login input sent".to_string();
    let n = usize::from(w) * usize::from(h);
    if w < 200 || h < 200 || w > 4096 || h > 4096 || n > 4 * 1024 * 1024 || pixels.len() != n * 4 {
        return Err(failed());
    }
    let base = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .ok_or_else(failed)?;
    let meta = fs::symlink_metadata(&base).map_err(|_| failed())?;
    if !meta.is_dir() || meta.uid() != unsafe { libc::geteuid() } || meta.mode() & 0o077 != 0 {
        return Err(failed());
    }
    let dir = base.join(ruwac_protocol::id("gdm_capture"));
    fs::DirBuilder::new()
        .mode(0o700)
        .create(&dir)
        .map_err(|_| failed())?;
    struct Cleanup(PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    let _cleanup = Cleanup(dir.clone());
    let path = dir.join("frame.png");
    let file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(&path)
        .map_err(|_| failed())?;
    let mut encoder = png::Encoder::new(file, u32::from(w), u32::from(h));
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    let rgb: Vec<u8> = pixels
        .chunks_exact(4)
        .flat_map(|b| [b[2], b[1], b[0]])
        .collect();
    encoder
        .write_header()
        .and_then(|mut writer| writer.write_image_data(&rgb))
        .map_err(|_| failed())?;
    let mut child = Command::new("python3")
        .arg("-c")
        .arg(include_str!("../../../scripts/gdm-recognize.py"))
        .arg(&path)
        .args(trusted.then_some("--trusted-sequence"))
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .map_err(|_| failed())?;
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        if child.try_wait().map_err(|_| failed())?.is_some() {
            break;
        }
        if Instant::now() >= deadline {
            unsafe {
                libc::kill(-(child.id() as i32), libc::SIGKILL);
            }
            let _ = child.wait();
            return Err(failed());
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    use std::io::Read;
    let mut reply = Vec::new();
    child
        .stdout
        .take()
        .ok_or_else(failed)?
        .take(4097)
        .read_to_end(&mut reply)
        .map_err(|_| failed())?;
    if reply.len() > 4096 {
        return Err(failed());
    }
    let observation: Observation = serde_json::from_slice(&reply).map_err(|_| failed())?;
    if !observation.validate(w, h) {
        return Err(failed());
    }
    Ok(observation)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    UserList,
    Username,
    Password,
    Submitted,
    Complete,
    Stopped,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    Click(u16, u16),
    Username,
    Password,
    Wait,
    Stop,
}
pub struct Sequence {
    pub step: Step,
}
impl Default for Sequence {
    fn default() -> Self {
        Self {
            step: Step::UserList,
        }
    }
}
impl Sequence {
    pub fn observe(&mut self, observation: &Observation) -> Action {
        if matches!(
            observation.kind,
            Kind::Users | Kind::Username | Kind::Password
        ) && !observation.rect.is_some_and(|[x, y, r, b]| x < r && y < b)
        {
            return self.stop();
        }
        match (self.step, observation.kind) {
            (Step::UserList, Kind::Users) => match observation.center() {
                Some((x, y)) => {
                    self.step = Step::Username;
                    Action::Click(x, y)
                }
                None => self.stop(),
            },
            (Step::Username, Kind::Username) => {
                self.step = Step::Password;
                Action::Username
            }
            (Step::Password, Kind::Password) => {
                self.step = Step::Submitted;
                Action::Password
            }
            (_, Kind::Unavailable | Kind::Ambiguous) => self.stop(),
            (Step::Submitted | Step::Complete | Step::Stopped, _) => Action::Wait,
            _ => Action::Wait,
        }
    }
    pub fn stop(&mut self) -> Action {
        self.step = Step::Stopped;
        Action::Stop
    }
    pub fn authenticated_handover(&mut self) -> bool {
        if self.step == Step::Submitted {
            self.step = Step::Complete;
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn credential_actions_require_the_ordered_observed_forms_and_are_once_only() {
        let mut s = Sequence::default();
        let o = |kind| Observation {
            kind,
            rect: Some([10, 20, 200, 50]),
        };
        assert_eq!(s.observe(&o(Kind::Password)), Action::Wait);
        assert!(!s.authenticated_handover());
        assert_eq!(s.observe(&o(Kind::Users)), Action::Click(105, 35));
        assert_eq!(s.observe(&o(Kind::Password)), Action::Wait);
        assert_eq!(s.observe(&o(Kind::Username)), Action::Username);
        assert_eq!(s.observe(&o(Kind::Username)), Action::Wait);
        assert_eq!(s.observe(&o(Kind::Password)), Action::Password);
        assert_eq!(s.observe(&o(Kind::Password)), Action::Wait);
        assert!(s.authenticated_handover());
    }
    #[test]
    fn ambiguous_fields_and_invalid_geometry_never_authorize_input() {
        let mut s = Sequence::default();
        assert_eq!(
            s.observe(&Observation {
                kind: Kind::Ambiguous,
                rect: None
            }),
            Action::Stop
        );
        assert_eq!(
            s.observe(&Observation {
                kind: Kind::Password,
                rect: Some([1, 1, 200, 20])
            }),
            Action::Wait
        );
        assert!(
            !Observation {
                kind: Kind::Password,
                rect: Some([30, 20, 10, 10])
            }
            .validate(640, 480)
        );
        assert!(
            !Observation {
                kind: Kind::Password,
                rect: None
            }
            .validate(640, 480)
        );
        let mut invalid = Sequence::default();
        assert_eq!(
            invalid.observe(&Observation {
                kind: Kind::Users,
                rect: None
            }),
            Action::Stop
        );
        assert_eq!(
            invalid.observe(&Observation {
                kind: Kind::Username,
                rect: Some([30, 20, 10, 10])
            }),
            Action::Stop
        );
    }
}
