//! Bounded bridge to IronRDP's CLIPRDR state machine. Payloads never enter daemon IPC.
use ironrdp_cliprdr::{backend::CliprdrBackend, pdu::*};
use ruwac_clipboard::{MAX_DESKTOP_TEXT, MAX_IMAGE_BYTES};
use std::collections::VecDeque;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
pub const PNG_ID: ClipboardFormatId = ClipboardFormatId(0xc001);
pub const MIME_PNG_ID: ClipboardFormatId = ClipboardFormatId(0xc002);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Text,
    Png,
    Dib,
    DibV5,
}
impl Format {
    pub fn id(self) -> ClipboardFormatId {
        match self {
            Self::Text => ClipboardFormatId::CF_UNICODETEXT,
            Self::Png => PNG_ID,
            Self::Dib => ClipboardFormatId::CF_DIB,
            Self::DibV5 => ClipboardFormatId::CF_DIBV5,
        }
    }
    pub fn mime(self) -> &'static str {
        if self == Self::Text {
            "text/plain"
        } else {
            "image/png"
        }
    }
}
pub fn formats(text: bool, image: bool) -> Vec<ClipboardFormat> {
    let mut f = Vec::new();
    if text {
        f.push(ClipboardFormat::new(Format::Text.id()));
    }
    if image {
        f.push(ClipboardFormat::new(PNG_ID).with_name(ClipboardFormatName::new_static("PNG")));
        f.push(
            ClipboardFormat::new(MIME_PNG_ID)
                .with_name(ClipboardFormatName::new_static("image/png")),
        );
        for x in [Format::DibV5, Format::Dib] {
            f.push(ClipboardFormat::new(x.id()));
        }
    }
    f
}
pub fn select(formats: &[ClipboardFormat]) -> Option<(ClipboardFormatId, Format)> {
    if let Some(f) = formats.iter().find(|f| {
        f.name().is_some_and(|n| {
            n.value().eq_ignore_ascii_case("PNG") || n.value().eq_ignore_ascii_case("image/png")
        })
    }) {
        return Some((f.id(), Format::Png));
    }
    for kind in [Format::DibV5, Format::Dib, Format::Text] {
        if formats.iter().any(|f| f.id() == kind.id()) {
            return Some((kind.id(), kind));
        }
    }
    None
}
/// One preferred image wire format plus Unicode text. Data is requested lazily.
pub fn select_all(formats: &[ClipboardFormat]) -> Vec<(ClipboardFormatId, Format)> {
    let mut result = Vec::new();
    if let Some(image) = select(formats).filter(|(_, k)| *k != Format::Text) {
        result.push(image);
    }
    if formats.iter().any(|f| f.id() == Format::Text.id()) {
        result.push((Format::Text.id(), Format::Text));
    }
    result
}
pub enum Event {
    Reset,
    Initialize,
    Ready,
    Offer {
        generation: u64,
        epoch: u64,
        selected: Option<(ClipboardFormatId, Format)>,
        formats: Vec<(ClipboardFormatId, Format)>,
    },
    Request(ClipboardFormatId),
    Data(Result<Vec<u8>, String>),
    Accepted(bool),
}
impl Event {
    fn bytes(&self) -> usize {
        if let Self::Data(Ok(b)) = self {
            b.len()
        } else {
            0
        }
    }
}
#[derive(Default)]
pub struct Queue {
    events: VecDeque<Event>,
    bytes: usize,
}
#[derive(Clone, Default)]
pub struct Bridge {
    pub epoch: Arc<AtomicU64>,
    pub overflow: Arc<AtomicBool>,
    queue: Arc<Mutex<Queue>>,
}
impl std::fmt::Debug for Bridge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("CLIPRDR bridge (payloads redacted)")
    }
}
impl Bridge {
    fn push(&self, event: Event) {
        let mut q = self.queue.lock().unwrap();
        if q.events.len() >= 16 || q.bytes + event.bytes() > MAX_IMAGE_BYTES {
            self.overflow.store(true, Ordering::Relaxed);
            return;
        }
        q.bytes += event.bytes();
        q.events.push_back(event);
    }
    pub fn reset(&self) {
        {
            let mut q = self.queue.lock().unwrap();
            q.events.clear();
            q.bytes = 0;
        }
        self.overflow.store(false, Ordering::Relaxed);
        self.epoch.store(0, Ordering::Release);
        self.push(Event::Reset);
    }
    pub fn drain(&self) -> Vec<Event> {
        let mut q = self.queue.lock().unwrap();
        q.bytes = 0;
        q.events.drain(..).collect()
    }
}
#[derive(Debug)]
pub struct Backend {
    pub bridge: Bridge,
    generation: u64,
}
impl Backend {
    pub fn new(bridge: Bridge) -> Self {
        Self {
            bridge,
            generation: 0,
        }
    }
}
impl CliprdrBackend for Backend {
    fn temporary_directory(&self) -> &str {
        ""
    }
    fn client_capabilities(&self) -> ClipboardGeneralCapabilityFlags {
        ClipboardGeneralCapabilityFlags::USE_LONG_FORMAT_NAMES
    }
    fn on_ready(&mut self) {
        self.bridge.push(Event::Ready)
    }
    fn on_request_format_list(&mut self) {
        self.bridge.push(Event::Initialize)
    }
    fn on_process_negotiated_capabilities(&mut self, _: ClipboardGeneralCapabilityFlags) {}
    fn on_remote_copy(&mut self, f: &[ClipboardFormat]) {
        self.generation = self.generation.saturating_add(1);
        self.bridge.push(Event::Offer {
            generation: self.generation,
            epoch: self.bridge.epoch.load(Ordering::Acquire),
            selected: select(f),
            formats: select_all(f),
        });
    }
    fn on_format_data_request(&mut self, r: FormatDataRequest) {
        self.bridge.push(Event::Request(r.format));
    }
    fn on_format_data_response(&mut self, r: FormatDataResponse<'_>) {
        self.bridge.push(Event::Data(if r.is_error() {
            Err("Remote clipboard refused data".into())
        } else if r.data().len() > MAX_IMAGE_BYTES {
            Err("Remote clipboard exceeds cap".into())
        } else {
            Ok(r.data().to_vec())
        }));
    }
    fn on_format_list_response(&mut self, ok: bool) {
        self.bridge.push(Event::Accepted(ok));
    }
    fn on_file_contents_request(&mut self, _: FileContentsRequest) {}
    fn on_file_contents_response(&mut self, _: FileContentsResponse<'_>) {}
    fn on_lock(&mut self, _: LockDataId) {}
    fn on_unlock(&mut self, _: LockDataId) {}
}
pub fn encode(kind: Format, data: &[u8]) -> Result<Vec<u8>, String> {
    if kind == Format::Text {
        let text = std::str::from_utf8(data).map_err(|_| "Invalid UTF-8")?;
        if data.len() > MAX_DESKTOP_TEXT || text.contains('\0') {
            return Err("Text exceeds cap or contains NUL".into());
        }
        let normalized = text.replace("\r\n", "\n").replace('\n', "\r\n");
        return Ok(normalized
            .encode_utf16()
            .chain(std::iter::once(0))
            .flat_map(u16::to_le_bytes)
            .collect());
    }
    validate_png(data)?;
    use ironrdp_cliprdr_format::bitmap::*;
    let out = match kind {
        Format::Png => Ok(data.to_vec()),
        Format::Dib => png_to_cf_dib(data),
        Format::DibV5 => png_to_cf_dibv5(data),
        _ => unreachable!(),
    }
    .map_err(|e| e.to_string())?;
    if out.len() > MAX_IMAGE_BYTES {
        return Err("Encoded bitmap exceeds cap".into());
    }
    Ok(out)
}
pub fn decode(kind: Format, data: &[u8]) -> Result<Vec<u8>, String> {
    if kind == Format::Text {
        if data.len() > MAX_DESKTOP_TEXT * 4 + 2 || data.len() % 2 != 0 || !data.ends_with(&[0, 0])
        {
            return Err("Invalid or oversized UTF-16 clipboard".into());
        }
        let words: Vec<_> = data[..data.len() - 2]
            .chunks_exact(2)
            .map(|b| u16::from_le_bytes([b[0], b[1]]))
            .collect();
        if words.contains(&0) {
            return Err("Embedded NUL clipboard".into());
        }
        let s = String::from_utf16(&words)
            .map_err(|_| "Invalid UTF-16 surrogate")?
            .replace("\r\n", "\n");
        if s.len() > MAX_DESKTOP_TEXT {
            return Err("Decoded text exceeds 1 MiB".into());
        }
        return Ok(s.into_bytes());
    }
    if data.len() > MAX_IMAGE_BYTES {
        return Err("Bitmap exceeds cap".into());
    }
    if kind != Format::Png {
        if data.len() < 40 {
            return Err("Truncated DIB header".into());
        }
        let w = i32::from_le_bytes(data[4..8].try_into().unwrap());
        let h = i32::from_le_bytes(data[8..12].try_into().unwrap());
        if w <= 0
            || w > 4096
            || h == 0
            || h.unsigned_abs() > 4096
            || (w as u64) * u64::from(h.unsigned_abs()) * 4 > MAX_IMAGE_BYTES as u64
        {
            return Err("DIB dimensions exceed cap".into());
        }
    }
    use ironrdp_cliprdr_format::bitmap::*;
    let out = match kind {
        Format::Png => Ok(data.to_vec()),
        Format::Dib => dib_to_png(data),
        Format::DibV5 => dibv5_to_png(data),
        _ => unreachable!(),
    }
    .map_err(|e| e.to_string())?;
    validate_png(&out)?;
    Ok(out)
}
pub fn validate_png(data: &[u8]) -> Result<(), String> {
    if data.len() > MAX_IMAGE_BYTES {
        return Err("PNG exceeds cap".into());
    }
    let mut d = png::Decoder::new(std::io::Cursor::new(data));
    d.set_limits(png::Limits {
        bytes: MAX_IMAGE_BYTES,
    });
    let mut r = d.read_info().map_err(|e| e.to_string())?;
    if r.info().width == 0
        || r.info().height == 0
        || r.info().width > 4096
        || r.info().height > 4096
    {
        return Err("PNG dimensions exceed cap".into());
    }
    let size = r.output_buffer_size().ok_or("PNG allocation overflow")?;
    if size > MAX_IMAGE_BYTES {
        return Err("PNG allocation cap".into());
    }
    r.next_frame(&mut vec![0; size])
        .map_err(|e| e.to_string())?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unicode_roundtrip_and_bounds() {
        for s in [
            "Živjo Łódź 🦀\nsecond".into(),
            "x".repeat(10240),
            "z".repeat(MAX_DESKTOP_TEXT),
        ] {
            assert_eq!(
                decode(Format::Text, &encode(Format::Text, s.as_bytes()).unwrap()).unwrap(),
                s.as_bytes()
            );
        }
        assert!(decode(Format::Text, &[0x00, 0xd8, 0, 0]).is_err());
        assert!(encode(Format::Text, &vec![b'x'; MAX_DESKTOP_TEXT + 1]).is_err());
    }
    #[test]
    fn queue_is_bounded_and_format_names_are_portable() {
        let bridge = Bridge::default();
        for _ in 0..17 {
            bridge.push(Event::Ready);
        }
        assert!(bridge.overflow.load(Ordering::Relaxed));
        assert_eq!(bridge.drain().len(), 16);
        let mut f = formats(true, true);
        f[1].id = ClipboardFormatId(0xc123);
        assert_eq!(select(&f), Some((ClipboardFormatId(0xc123), Format::Png)));
    }
    #[test]
    fn png_dib_pixels_roundtrip() {
        for (w, h) in [(64, 64), (65, 7), (2560, 1440)] {
            let mut bytes = Vec::new();
            let pixels: Vec<u8> = (0..w * h)
                .flat_map(|i| [(i % 251) as u8, 42, 129, (i % 255) as u8])
                .collect();
            {
                let mut e = png::Encoder::new(&mut bytes, w, h);
                e.set_color(png::ColorType::Rgba);
                e.set_depth(png::BitDepth::Eight);
                e.write_header().unwrap().write_image_data(&pixels).unwrap();
            }
            for kind in [Format::Png, Format::DibV5] {
                let out = decode(kind, &encode(kind, &bytes).unwrap()).unwrap();
                let mut d = png::Decoder::new(std::io::Cursor::new(out))
                    .read_info()
                    .unwrap();
                let mut b = vec![0; d.output_buffer_size().unwrap()];
                let info = d.next_frame(&mut b).unwrap();
                assert_eq!(&b[..info.buffer_size()], &pixels);
            }
            let out = decode(Format::Dib, &encode(Format::Dib, &bytes).unwrap()).unwrap();
            let mut reader = png::Decoder::new(std::io::Cursor::new(out))
                .read_info()
                .unwrap();
            let mut buffer = vec![0; reader.output_buffer_size().unwrap()];
            let info = reader.next_frame(&mut buffer).unwrap();
            let rgb: Vec<u8> = pixels
                .chunks_exact(4)
                .flat_map(|p| p[..3].iter().copied())
                .collect();
            assert_eq!(&buffer[..info.buffer_size()], rgb.as_slice());
            assert!(decode(Format::Dib, &[0; 40]).is_err());
        }
    }
}
ironrdp_core::impl_as_any!(Backend);

/// Reject oversized/malformed SVC fragments before IronRDP's reassembly allocates.
#[derive(Default)]
pub struct FragmentGuard {
    pending: Option<(usize, usize)>,
}
impl FragmentGuard {
    pub fn check(&mut self, payload: &[u8]) -> Result<(), String> {
        use ironrdp::pdu::rdp::vc::{ChannelControlFlags, ChannelPduHeader};
        let mut cursor = ironrdp_core::ReadCursor::new(payload);
        let header: ChannelPduHeader = ironrdp_core::decode_cursor(&mut cursor)
            .map_err(|_| "Invalid clipboard fragment header")?;
        let length = header.length as usize;
        let size = cursor.remaining().len();
        if length > MAX_IMAGE_BYTES + 8 {
            return Err("CLIPRDR reassembly cap exceeded".into());
        }
        if header.flags.contains(ChannelControlFlags::FLAG_FIRST) {
            if self.pending.is_some() {
                return Err("Overlapping clipboard fragments".into());
            }
            self.pending = Some((length, 0));
        }
        let Some((declared, used)) = self.pending.as_mut() else {
            return Err("Clipboard fragment without first fragment".into());
        };
        if *declared != length || size > length.saturating_sub(*used) {
            return Err("Invalid clipboard fragment length".into());
        }
        *used += size;
        if header.flags.contains(ChannelControlFlags::FLAG_LAST) {
            if *used != length {
                return Err("Truncated clipboard fragments".into());
            }
            self.pending = None;
        }
        Ok(())
    }
}
#[cfg(test)]
mod fragment_tests {
    use super::*;
    fn chunk(len: u32, flags: u32, data: &[u8]) -> Vec<u8> {
        [
            len.to_le_bytes().as_slice(),
            flags.to_le_bytes().as_slice(),
            data,
        ]
        .concat()
    }
    #[test]
    fn reassembly_is_capped_before_allocation() {
        let mut guard = FragmentGuard::default();
        assert!(
            guard
                .check(&chunk((MAX_IMAGE_BYTES + 9) as u32, 3, &[]))
                .is_err()
        );
        assert!(guard.check(&chunk(3, 1, &[1])).is_ok());
        assert!(guard.check(&chunk(3, 2, &[2, 3])).is_ok());
        assert!(guard.check(&chunk(1, 2, &[1])).is_err());
        assert!(guard.check(&chunk(3, 3, &[1])).is_err());
    }
    #[test]
    fn gnome_named_png_preserves_peer_assigned_id() {
        let offered = ClipboardFormat::new(ClipboardFormatId(0xd011))
            .with_name(ClipboardFormatName::new_static("image/png"));
        assert_eq!(
            select(&[offered]),
            Some((ClipboardFormatId(0xd011), Format::Png))
        );
        let local = formats(false, true);
        assert!(
            local.iter().any(
                |f| f.id() == MIME_PNG_ID && f.name().is_some_and(|n| n.value() == "image/png")
            )
        );
        assert!(
            local
                .iter()
                .any(|f| f.id() == PNG_ID && f.name().is_some_and(|n| n.value() == "PNG"))
        );
    }
}
