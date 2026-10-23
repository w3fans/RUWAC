use sha2::{Digest, Sha256};
use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
};
pub struct Receipt {
    pub path: PathBuf,
    pub sha256: String,
    pub bytes: usize,
    pub width: u32,
    pub height: u32,
}
pub const MAX_IMAGE_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_DESKTOP_TEXT: usize = 1024 * 1024;
/// File transport is local-only. No encoded image enters the JSON control frame.
pub fn stage_png(input: &Path, dir: &Path) -> Result<Receipt, String> {
    stage_png_with_limit(input, dir, MAX_IMAGE_BYTES)
}
pub fn stage_png_with_limit(input: &Path, dir: &Path, limit: usize) -> Result<Receipt, String> {
    let limit = limit.min(MAX_IMAGE_BYTES);
    let mut source = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
        .open(input)
        .map_err(|e| e.to_string())?;
    let m = source.metadata().map_err(|e| e.to_string())?;
    if !m.is_file() || m.len() > limit as u64 {
        return Err("PNG file exceeds configured cap".into());
    }
    let m = fs::symlink_metadata(dir).map_err(|e| e.to_string())?;
    if !m.is_dir() || m.uid() != unsafe { libc::geteuid() } || m.mode() & 0o077 != 0 {
        return Err("staging directory must be owned and 0700".into());
    }
    let path = dir.join(format!("{}.png", uuid::Uuid::new_v4().simple()));
    let result = (|| -> Result<Receipt, String> {
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
            .open(&path)
            .map_err(|e| e.to_string())?;
        let mut hash = Sha256::new();
        let mut bytes = 0;
        let mut chunk = [0u8; 65536];
        loop {
            let n = source.read(&mut chunk).map_err(|e| e.to_string())?;
            if n == 0 {
                break;
            }
            bytes += n;
            if bytes > limit {
                return Err("PNG stream exceeds cap".into());
            }
            hash.update(&chunk[..n]);
            file.write_all(&chunk[..n]).map_err(|e| e.to_string())?;
        }
        file.sync_all().map_err(|e| e.to_string())?;
        use std::io::{BufReader, Seek, SeekFrom};
        file.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
        let mut decoder = png::Decoder::new(BufReader::new(file));
        decoder.set_limits(png::Limits {
            bytes: MAX_IMAGE_BYTES,
        });
        let mut reader = decoder.read_info().map_err(|e| e.to_string())?;
        let (width, height) = (reader.info().width, reader.info().height);
        if width == 0 || height == 0 || width > 4096 || height > 4096 {
            return Err("PNG dimensions exceed 4096".into());
        }
        let n = reader
            .output_buffer_size()
            .ok_or("PNG allocation overflow")?;
        if n > MAX_IMAGE_BYTES {
            return Err("PNG decoded allocation cap".into());
        }
        reader
            .next_frame(&mut vec![0; n])
            .map_err(|e| e.to_string())?;
        Ok(Receipt {
            path: path.clone(),
            sha256: format!("{:x}", hash.finalize()),
            bytes,
            width,
            height,
        })
    })();
    if result.is_err() {
        let _ = fs::remove_file(&path);
    }
    result
}

/// Metadata-only policy. It never reads or writes desktop clipboard payloads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClipScope {
    Clipboard,
    Primary,
    Internal,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClipPolicy {
    Bidirectional,
    LocalToRemote,
    RemoteToLocal,
    Manual,
    Disabled,
}
#[derive(Clone, Debug)]
pub struct RemoteOffer {
    pub view: String,
    pub mime: String,
    pub generation: u64,
    pub owner_focus_epoch: u64,
    pub size: usize,
    pub scope: ClipScope,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OfferDecision {
    Eligible,
    Cached,
    Rejected,
}
#[derive(Default)]
pub struct FocusBroker {
    active: Option<String>,
    epoch: u64,
    transfer_generation: u64,
    desktop_disabled: bool,
    pub last_transfer: Option<DesktopTransfer>,
    offers: std::collections::HashMap<String, RemoteOffer>,
}
impl FocusBroker {
    pub fn set_desktop_disabled(&mut self, disabled: bool) {
        self.desktop_disabled = disabled;
        self.local_copy();
    }
    pub fn desktop_enabled(&self) -> bool {
        !self.desktop_disabled
    }
    pub fn begin_transfer(
        &mut self,
        scope: ClipScope,
        mime: &str,
        direction: TransferDirection,
    ) -> Option<DesktopTransfer> {
        if self.desktop_disabled {
            return None;
        }
        let view = self.active.clone()?;
        if scope == ClipScope::Internal || !matches!(mime, "text/plain" | "image/png") {
            return None;
        }
        self.transfer_generation = self.transfer_generation.saturating_add(1);
        let ticket = DesktopTransfer {
            view,
            scope,
            mime: mime.into(),
            generation: self.transfer_generation,
            focus_epoch: self.epoch,
            direction,
            state: TransferState::Streaming,
            bytes: 0,
        };
        self.last_transfer = Some(ticket.clone());
        Some(ticket)
    }
    pub fn transfer_valid(&self, ticket: &DesktopTransfer) -> bool {
        self.active.as_deref() == Some(ticket.view.as_str())
            && self.epoch == ticket.focus_epoch
            && self
                .last_transfer
                .as_ref()
                .is_some_and(|t| t.generation == ticket.generation)
    }
    pub fn finish_transfer(
        &mut self,
        ticket: &DesktopTransfer,
        bytes: usize,
        state: TransferState,
    ) {
        if self
            .last_transfer
            .as_ref()
            .is_some_and(|t| t.generation == ticket.generation)
        {
            let valid = self.transfer_valid(ticket);
            let mut completed = ticket.clone();
            completed.bytes = bytes;
            completed.state = if valid {
                state
            } else {
                TransferState::Cancelled
            };
            self.last_transfer = Some(completed);
        }
    }
    pub fn active(&self) -> Option<&str> {
        self.active.as_deref()
    }
    pub fn epoch(&self) -> u64 {
        self.epoch
    }
    /// Changing focus invalidates in-flight promotions, without promoting cached data.
    pub fn focus(&mut self, view: Option<&str>) {
        if self.active.as_deref() != view {
            self.active = view.map(str::to_owned);
            self.epoch = self.epoch.saturating_add(1);
        }
    }
    /// Explicit local copy takes ownership even when the focused view is unchanged.
    pub fn local_copy(&mut self) {
        self.epoch = self.epoch.saturating_add(1);
    }
    pub fn offer(&mut self, offer: RemoteOffer, policy: ClipPolicy) -> OfferDecision {
        if self.desktop_disabled
            || policy == ClipPolicy::Disabled
            || offer.view.is_empty()
            || offer.view.len() > 128
            || offer.generation == 0
            || offer.size
                > if offer.mime == "image/png" {
                    MAX_IMAGE_BYTES
                } else {
                    MAX_DESKTOP_TEXT
                }
            || !matches!(
                offer.mime.as_str(),
                "text/plain" | "text/plain;charset=utf-8" | "image/png"
            )
            || offer.scope != ClipScope::Clipboard
        {
            return OfferDecision::Rejected;
        }
        if let Some(previous) = self.offers.get(&offer.view) {
            if offer.generation <= previous.generation {
                return OfferDecision::Rejected;
            }
        } else if self.offers.len() >= 64 {
            return OfferDecision::Rejected;
        }
        let eligible = matches!(
            policy,
            ClipPolicy::Bidirectional | ClipPolicy::RemoteToLocal
        ) && self.active.as_deref() == Some(offer.view.as_str())
            && offer.owner_focus_epoch == self.epoch;
        self.offers.insert(offer.view.clone(), offer);
        if eligible {
            OfferDecision::Eligible
        } else {
            OfferDecision::Cached
        }
    }
    pub fn disconnect(&mut self, view: &str) {
        self.offers.remove(view);
        if self.active.as_deref() == Some(view) {
            self.focus(None);
        }
    }
}
#[cfg(test)]
mod broker_tests {
    use super::*;
    fn offer(view: &str, generation: u64, epoch: u64) -> RemoteOffer {
        RemoteOffer {
            view: view.into(),
            mime: "text/plain".into(),
            generation,
            owner_focus_epoch: epoch,
            size: 10,
            scope: ClipScope::Clipboard,
        }
    }
    #[test]
    fn repeated_keyboard_focus_keeps_clipboard_lease_but_owner_changes_and_copy_cancel() {
        let mut broker = FocusBroker::default();
        broker.focus(Some("terminal-a"));
        let epoch = broker.epoch();
        for _ in 0..1000 {
            broker.focus(Some("terminal-a"));
            assert_eq!(broker.epoch(), epoch);
        }
        broker.local_copy();
        assert_ne!(broker.epoch(), epoch);
        let copied = broker.epoch();
        broker.focus(Some("rdp-a"));
        assert_ne!(broker.epoch(), copied);
        broker.focus(None);
        assert_eq!(broker.active(), None);
    }
    #[test]
    fn inactive_offer_never_steals_or_auto_promotes_on_focus() {
        let mut b = FocusBroker::default();
        b.focus(Some("a"));
        let epoch = b.epoch();
        assert_eq!(
            b.offer(offer("b", 1, epoch), ClipPolicy::Bidirectional),
            OfferDecision::Cached
        );
        b.focus(Some("b"));
        assert_eq!(
            b.offer(offer("b", 2, epoch), ClipPolicy::Bidirectional),
            OfferDecision::Cached
        );
        assert_eq!(
            b.offer(offer("b", 3, b.epoch()), ClipPolicy::Bidirectional),
            OfferDecision::Eligible
        );
    }
    #[test]
    fn local_copy_cancels_delayed_remote_offer() {
        let mut b = FocusBroker::default();
        b.focus(Some("a"));
        let epoch = b.epoch();
        b.local_copy();
        assert_eq!(
            b.offer(offer("a", 1, epoch), ClipPolicy::Bidirectional),
            OfferDecision::Cached
        );
    }
    #[test]
    fn generations_prevent_echo_and_manual_policy_is_explicit() {
        let mut b = FocusBroker::default();
        b.focus(Some("a"));
        let o = offer("a", 1, b.epoch());
        assert_eq!(
            b.offer(o.clone(), ClipPolicy::Manual),
            OfferDecision::Cached
        );
        assert_eq!(
            b.offer(o, ClipPolicy::Bidirectional),
            OfferDecision::Rejected
        );
    }
    #[test]
    fn scopes_payload_limits_and_disabled_policy_are_separate() {
        let mut b = FocusBroker::default();
        b.focus(Some("a"));
        let mut o = offer("a", 1, b.epoch());
        o.scope = ClipScope::Internal;
        assert_eq!(
            b.offer(o.clone(), ClipPolicy::Bidirectional),
            OfferDecision::Rejected
        );
        o.scope = ClipScope::Primary;
        assert_eq!(
            b.offer(o.clone(), ClipPolicy::Bidirectional),
            OfferDecision::Rejected
        );
        o.scope = ClipScope::Clipboard;
        o.size = 8 * 1024 * 1024 + 1;
        assert_eq!(
            b.offer(o.clone(), ClipPolicy::Bidirectional),
            OfferDecision::Rejected
        );
        o.size = 1;
        assert_eq!(b.offer(o, ClipPolicy::Disabled), OfferDecision::Rejected);
    }
    #[test]
    fn disconnect_invalidates_in_flight_owner_epoch() {
        let mut b = FocusBroker::default();
        b.focus(Some("a"));
        let epoch = b.epoch();
        b.disconnect("a");
        assert_eq!(
            b.offer(offer("a", 1, epoch), ClipPolicy::Bidirectional),
            OfferDecision::Cached
        );
    }
}

/// Future embedded RDP boundary; no network backend is implemented here.
#[derive(Clone, Debug)]
pub struct RdpView {
    pub id: String,
    pub connected: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransferDirection {
    DesktopToView,
    ViewToDesktop,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransferState {
    Offered,
    Streaming,
    Complete,
    Cancelled,
}
pub trait ViewClipboardAdapter {
    fn view_id(&self) -> &str;
    fn cancel(&mut self);
}
impl ViewClipboardAdapter for RdpView {
    fn view_id(&self) -> &str {
        &self.id
    }
    fn cancel(&mut self) {
        self.connected = false;
    }
}
#[cfg(test)]
mod rdp_boundary_tests {
    use super::*;
    #[test]
    fn three_views_and_stale_producers() {
        let mut broker = FocusBroker::default();
        let views = [
            RdpView {
                id: "rdp_a".into(),
                connected: true,
            },
            RdpView {
                id: "rdp_b".into(),
                connected: true,
            },
            RdpView {
                id: "rdp_c".into(),
                connected: true,
            },
        ];
        for generation in 1..=200 {
            let active = (generation % 3) as usize;
            broker.focus(Some(views[active].view_id()));
            let epoch = broker.epoch();
            for (i, view) in views.iter().enumerate() {
                let decision = broker.offer(
                    RemoteOffer {
                        view: view.id.clone(),
                        mime: "image/png".into(),
                        generation,
                        owner_focus_epoch: epoch,
                        size: 15 * 1024 * 1024,
                        scope: ClipScope::Clipboard,
                    },
                    ClipPolicy::Bidirectional,
                );
                assert_eq!(
                    decision,
                    if i == active {
                        OfferDecision::Eligible
                    } else {
                        OfferDecision::Cached
                    }
                );
            }
            broker.local_copy();
            assert_eq!(
                broker.offer(
                    RemoteOffer {
                        view: views[active].id.clone(),
                        mime: "text/plain".into(),
                        generation: generation + 1000,
                        owner_focus_epoch: epoch,
                        size: 1,
                        scope: ClipScope::Clipboard
                    },
                    ClipPolicy::Bidirectional
                ),
                OfferDecision::Cached
            );
            // Remove the synthetic higher generation so the next independent round starts cleanly.
            for view in &views {
                broker.disconnect(&view.id);
            }
        }
    }
}
#[cfg(test)]
mod image_limits_tests {
    use super::*;
    #[test]
    fn cap_dimensions_symlink_and_cleanup() {
        use std::os::unix::fs::PermissionsExt;
        let root = std::env::temp_dir().join(format!("ruwac-image-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        let stage = root.join("stage");
        fs::create_dir(&stage).unwrap();
        fs::set_permissions(&stage, fs::Permissions::from_mode(0o700)).unwrap();
        let input = root.join("fixture.png");
        {
            let mut encoder = png::Encoder::new(std::fs::File::create(&input).unwrap(), 64, 64);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            encoder
                .write_header()
                .unwrap()
                .write_image_data(&vec![0; 64 * 64 * 4])
                .unwrap();
        }
        assert!(stage_png_with_limit(&input, &stage, 10).is_err());
        assert_eq!(fs::read_dir(&stage).unwrap().count(), 0);
        let receipt = stage_png(&input, &stage).unwrap();
        assert_eq!((receipt.width, receipt.height), (64, 64));
        assert_eq!(fs::metadata(&receipt.path).unwrap().mode() & 0o777, 0o600);
        fs::remove_file(receipt.path).unwrap();
        let link = root.join("link.png");
        std::os::unix::fs::symlink(&input, &link).unwrap();
        assert!(stage_png(&link, &stage).is_err());
        {
            let mut encoder = png::Encoder::new(std::fs::File::create(&input).unwrap(), 5000, 1);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            encoder
                .write_header()
                .unwrap()
                .write_image_data(&vec![0; 5000 * 4])
                .unwrap();
        }
        assert!(stage_png(&input, &stage).is_err());
        assert_eq!(fs::read_dir(&stage).unwrap().count(), 0);
        fs::write(&input, b"not a PNG").unwrap();
        assert!(stage_png(&input, &stage).is_err());
        assert_eq!(fs::read_dir(&stage).unwrap().count(), 0);
        fs::remove_dir_all(root).unwrap();
    }
}

#[derive(Clone, Debug)]
pub struct DesktopTransfer {
    pub view: String,
    pub scope: ClipScope,
    pub mime: String,
    pub generation: u64,
    pub focus_epoch: u64,
    pub direction: TransferDirection,
    pub state: TransferState,
    pub bytes: usize,
}
#[cfg(test)]
mod desktop_transfer_tests {
    use super::*;
    #[test]
    fn stale_focus_owner_and_newer_read_cancel_transfer() {
        let mut broker = FocusBroker::default();
        broker.focus(Some("terminal-a"));
        let old = broker
            .begin_transfer(
                ClipScope::Clipboard,
                "text/plain",
                TransferDirection::DesktopToView,
            )
            .unwrap();
        assert!(broker.transfer_valid(&old));
        broker.local_copy();
        assert!(!broker.transfer_valid(&old));
        broker.finish_transfer(&old, 0, TransferState::Complete);
        assert_eq!(
            broker.last_transfer.as_ref().unwrap().state,
            TransferState::Cancelled
        );
        let first = broker
            .begin_transfer(
                ClipScope::Primary,
                "text/plain",
                TransferDirection::DesktopToView,
            )
            .unwrap();
        let second = broker
            .begin_transfer(
                ClipScope::Clipboard,
                "image/png",
                TransferDirection::DesktopToView,
            )
            .unwrap();
        assert!(!broker.transfer_valid(&first));
        assert!(broker.transfer_valid(&second));
        broker.focus(Some("terminal-b"));
        assert!(!broker.transfer_valid(&second));
    }
}
#[cfg(test)]
mod direction_policy_tests {
    use super::*;
    #[test]
    fn all_five_policies_and_disabled_desktop() {
        for (policy, decision) in [
            (ClipPolicy::Bidirectional, OfferDecision::Eligible),
            (ClipPolicy::RemoteToLocal, OfferDecision::Eligible),
            (ClipPolicy::LocalToRemote, OfferDecision::Cached),
            (ClipPolicy::Manual, OfferDecision::Cached),
            (ClipPolicy::Disabled, OfferDecision::Rejected),
        ] {
            let mut b = FocusBroker::default();
            b.focus(Some("rdp"));
            let o = RemoteOffer {
                view: "rdp".into(),
                mime: "text/plain".into(),
                generation: 1,
                owner_focus_epoch: b.epoch(),
                size: 0,
                scope: ClipScope::Clipboard,
            };
            assert_eq!(b.offer(o.clone(), policy), decision);
            b.set_desktop_disabled(true);
            assert_eq!(
                b.offer(RemoteOffer { generation: 2, ..o }, policy),
                OfferDecision::Rejected
            );
        }
    }
}
