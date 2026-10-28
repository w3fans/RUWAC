mod copy;
mod status_commands;
use ruwac_protocol::{MAX_TEXT, request, write_frame};
use serde_json::{Value, json};
use std::{
    collections::HashSet,
    io::{self, Read, Write},
    os::unix::net::UnixStream,
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};
static STOP: AtomicBool = AtomicBool::new(false);
/// The next live frame starts from a cleared screen (first frame, resize).
static CLEAR: AtomicBool = AtomicBool::new(true);
extern "C" fn signal(_: i32) {
    STOP.store(true, Ordering::Relaxed);
}
#[derive(Debug, PartialEq)]
pub enum Input {
    Keyboard(Vec<u8>),
    Mouse(Vec<u8>),
    Paste(Vec<u8>),
    /// The outer terminal's OSC 52 answer with the desktop mouse selection.
    Selection(String),
    Rejected(&'static str),
}
#[derive(Default)]
pub struct Decoder {
    buf: Vec<u8>,
    paste: Option<Vec<u8>>,
    discard_paste: bool,
}
impl Decoder {
    pub fn feed(&mut self, b: &[u8]) -> Vec<Input> {
        self.buf.extend_from_slice(b);
        let mut out = Vec::new();
        loop {
            if self.paste.is_some() {
                if let Some(i) = find(&self.buf, b"\x1b[201~") {
                    let mut p = self.paste.take().unwrap();
                    if !self.discard_paste {
                        p.extend_from_slice(&self.buf[..i]);
                    }
                    self.buf.drain(..i + 6);
                    if self.discard_paste {
                        self.discard_paste = false;
                        continue;
                    }
                    if p.len() > MAX_TEXT {
                        out.push(Input::Rejected("paste exceeds 256 KiB"))
                    } else {
                        out.push(Input::Paste(p))
                    }
                    continue;
                }
                let n = self.buf.len().saturating_sub(5);
                if self.discard_paste {
                    self.buf.drain(..n);
                    break;
                }
                if self.paste.as_ref().unwrap().len() + n > MAX_TEXT {
                    self.paste = Some(Vec::new());
                    self.discard_paste = true;
                    self.buf.drain(..n);
                    out.push(Input::Rejected("paste exceeds 256 KiB"));
                    break;
                }
                self.paste.as_mut().unwrap().extend(self.buf.drain(..n));
                break;
            }
            if self.buf.is_empty() {
                break;
            }
            if self.buf.starts_with(b"\x1b[200~") {
                self.buf.drain(..6);
                self.paste = Some(Vec::new());
                continue;
            }
            if self.buf[0] != 27 {
                let n = self
                    .buf
                    .iter()
                    .position(|b| *b == 27)
                    .unwrap_or(self.buf.len());
                let b = &self.buf[..n];
                let valid = match std::str::from_utf8(b) {
                    Ok(_) => n,
                    Err(e) if e.error_len().is_none() => e.valid_up_to(),
                    Err(_) => {
                        self.buf.drain(..n);
                        out.push(Input::Rejected("input is not UTF-8"));
                        continue;
                    }
                };
                if valid == 0 {
                    break;
                }
                out.push(Input::Keyboard(self.buf.drain(..valid).collect()));
                continue;
            }
            if self.buf.len() == 1 {
                break;
            }
            if self.buf[1] == b'[' {
                if self.buf.len() < 3 {
                    break;
                }
                let end = self.buf[2..]
                    .iter()
                    .position(|b| (0x40..=0x7e).contains(b))
                    .map(|n| n + 3);
                let Some(mut n) = end else {
                    if self.buf.len() > 4096 {
                        out.push(Input::Keyboard(self.buf.drain(..).collect()));
                    }
                    break;
                };
                if self.buf[2] == b'M' {
                    if self.buf.len() < 6 {
                        break;
                    }
                    n = 6;
                }
                let b: Vec<_> = self.buf.drain(..n).collect();
                let mouse = b.starts_with(b"\x1b[<") && matches!(b.last(), Some(b'M' | b'm'))
                    || b.starts_with(b"\x1b[M");
                let cap = b.last() == Some(&b'c')
                    && (b.starts_with(b"\x1b[?") || b.starts_with(b"\x1b[>"))
                    && b[3..b.len() - 1]
                        .iter()
                        .all(|x| x.is_ascii_digit() || *x == b';');
                if mouse {
                    out.push(Input::Mouse(b))
                } else if !cap {
                    out.push(Input::Keyboard(b))
                }
                continue;
            }
            if self.buf[1] == b'O' {
                if self.buf.len() < 3 {
                    break;
                }
                out.push(Input::Keyboard(self.buf.drain(..3).collect()));
                continue;
            }
            if self.buf[1] == b']' {
                let end = self
                    .buf
                    .iter()
                    .position(|b| *b == 7)
                    .map(|i| i + 1)
                    .or_else(|| find(&self.buf, b"\x1b\\").map(|i| i + 2));
                // A selection answer carries up to 128 KiB as base64.
                let limit = if self.buf.starts_with(b"\x1b]52;") {
                    SELECTION_LIMIT * 4 / 3 + 64
                } else {
                    4096
                };
                let Some(n) = end else {
                    if self.buf.len() > limit {
                        out.push(Input::Keyboard(self.buf.drain(..).collect()));
                    }
                    break;
                };
                let b: Vec<_> = self.buf.drain(..n).collect();
                let cap = (b.starts_with(b"\x1b]10;rgb:") || b.starts_with(b"\x1b]11;rgb:"))
                    && b.iter().all(|x| x.is_ascii());
                if let Some(data) = b.strip_prefix(b"\x1b]52;p;") {
                    let data = data
                        .strip_suffix(b"\x07")
                        .or_else(|| data.strip_suffix(b"\x1b\\"))
                        .unwrap_or(data);
                    if let Some(text) = base64_decode(data)
                        .and_then(|t| String::from_utf8(t).ok())
                        .filter(|t| t.len() <= SELECTION_LIMIT)
                    {
                        out.push(Input::Selection(text));
                    }
                } else if !cap {
                    out.push(Input::Keyboard(b))
                }
                continue;
            }
            out.push(Input::Keyboard(self.buf.drain(..2).collect()));
        }
        out
    }
    pub fn escape_timeout(&mut self) -> Vec<Input> {
        if self.paste.is_none() && self.buf == [27] {
            self.buf.clear();
            vec![Input::Keyboard(vec![27])]
        } else {
            vec![]
        }
    }
}
/// Mouse selections exchanged with the outer terminal over OSC 52 PRIMARY.
const SELECTION_LIMIT: usize = 128 * 1024;
const BASE64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
fn base64_encode(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, b)| n | u32::from(*b) << (16 - 8 * i));
        for i in 0..4 {
            out.push(if i <= chunk.len() {
                BASE64[(n >> (18 - 6 * i) & 63) as usize] as char
            } else {
                '='
            });
        }
    }
    out
}
fn base64_decode(data: &[u8]) -> Option<Vec<u8>> {
    let data = data
        .strip_suffix(b"==")
        .or_else(|| data.strip_suffix(b"="))
        .unwrap_or(data);
    let mut out = Vec::with_capacity(data.len() * 3 / 4);
    let (mut acc, mut bits) = (0u32, 0);
    for c in data {
        acc = acc << 6 | BASE64.iter().position(|b| b == c)? as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Some(out)
}
/// Offer a mouse selection to the outer terminal's desktop PRIMARY (OSC 52);
/// terminals without OSC 52 ignore it and the session buffer still has it.
fn offer_selection(text: &str) {
    if text.is_empty() || text.len() > SELECTION_LIMIT {
        return;
    }
    let mut stdout = io::stdout().lock();
    let _ = write!(stdout, "\x1b]52;p;{}\x07", base64_encode(text.as_bytes()));
    let _ = stdout.flush();
}
fn find(b: &[u8], needle: &[u8]) -> Option<usize> {
    b.windows(needle.len()).position(|x| x == needle)
}
struct Raw {
    term: libc::termios,
    term_handler: libc::sighandler_t,
    int_handler: libc::sighandler_t,
}
impl Raw {
    fn enter() -> io::Result<Self> {
        unsafe {
            if libc::isatty(0) != 1 || libc::isatty(1) != 1 {
                return Err(io::Error::other("attach requires a terminal"));
            }
            let mut old = std::mem::zeroed();
            if libc::tcgetattr(0, &mut old) != 0 {
                return Err(io::Error::last_os_error());
            }
            let mut raw = old;
            libc::cfmakeraw(&mut raw);
            if libc::tcsetattr(0, libc::TCSANOW, &raw) != 0 {
                return Err(io::Error::last_os_error());
            }
            let th = libc::signal(libc::SIGTERM, signal as *const () as libc::sighandler_t);
            let ih = libc::signal(libc::SIGINT, signal as *const () as libc::sighandler_t);
            let guard = Self {
                term: old,
                term_handler: th,
                int_handler: ih,
            };
            io::stdout().write_all(b"\x1b[22;0t\x1b[?1049h\x1b[?2004h\x1b[?25l")?;
            io::stdout().flush()?;
            Ok(guard)
        }
    }
}
impl Drop for Raw {
    fn drop(&mut self) {
        let _=io::stdout().write_all(b"\x1b[?1000l\x1b[?1002l\x1b[?1003l\x1b[?1006l\x1b[?1004l\x1b[?2004l\x1b[?25h\x1b[0m\x1b[?1049l\x1b[23;0t");
        let _ = io::stdout().flush();
        unsafe {
            libc::tcsetattr(0, libc::TCSANOW, &self.term);
            libc::signal(libc::SIGTERM, self.term_handler);
            libc::signal(libc::SIGINT, self.int_handler);
        }
    }
}
fn terminal_dimensions() -> (u16, u16) {
    unsafe {
        let mut s: libc::winsize = std::mem::zeroed();
        if libc::ioctl(0, libc::TIOCGWINSZ, &mut s) != 0 {
            return (24, 80);
        }
        (
            if s.ws_row == 0 { 24 } else { s.ws_row },
            if s.ws_col == 0 { 80 } else { s.ws_col },
        )
    }
}
fn dimensions(limits: ruwac_protocol::TerminalLimits) -> (u16, u16) {
    let (rows, cols) = terminal_dimensions();
    limits.clamp(rows.saturating_sub(1), cols)
}

fn raw_call(s: &mut UnixStream, m: &str, v: Value) -> io::Result<Value> {
    let sent = request(m, v);
    write_frame(s, &sent)?;
    s.set_read_timeout(Some(Duration::from_secs(if m == "client.render" {
        15
    } else {
        5
    })))?;
    let mut r = ruwac_protocol::read_frame_with_limit(
        s,
        if m == "client.render" {
            ruwac_protocol::MAX_RENDER_FRAME
        } else {
            ruwac_protocol::MAX_FRAME
        },
    )?;
    if r["version"] != 0 || r["id"] != sent.id {
        return Err(io::Error::other("Protocol/correlation rejected"));
    }
    if let Some(e) = r.get("error") {
        return Err(io::Error::other(e.to_string()));
    }
    Ok(r["result"].take())
}
fn call(s: &mut UnixStream, m: &str, mut v: Value) -> io::Result<Value> {
    if matches!(m, "session.create" | "window.create" | "pane.create") && v.get("argv").is_none() {
        v["argv"] = json!(ruwac_pty::shell::default_terminal_argv().map_err(io::Error::other)?);
    }
    let mut result = ruwac_protocol::client_call(m, v, |m, v| raw_call(s, m, v))?;
    if m == "client.render" && result["unchanged"] != true && result.get("session_name").is_none() {
        if let Ok(sessions) = call(s, "session.list", json!({})) {
            if let Some(session) = sessions
                .as_array()
                .and_then(|rows| rows.iter().find(|row| row["id"] == result["session"]))
            {
                result["session_name"] = session["name"].clone();
                result["rows"] = session["rows"].clone();
                result["cols"] = session["cols"].clone();
            }
        }
        if let Ok(windows) = call(s, "window.list", json!({"session":result["session"]})) {
            if let Some(rows) = windows.as_array() {
                result["window_index"] =
                    json!(rows.iter().position(|row| row["id"] == result["window"]));
                result["windows"] = json!(
                    rows.iter()
                        .enumerate()
                        .map(
                            |(index, row)| json!({"id":row["id"],"index":index,"name":row["name"]})
                        )
                        .collect::<Vec<_>>()
                );
            }
        }
    }
    Ok(result)
}
fn sgr(c: &Value, fg: bool) -> String {
    let base = if fg { 38 } else { 48 };
    if let Some(a) = c.as_array() {
        format!("{base};2;{};{};{}", a[0], a[1], a[2])
    } else {
        let n = c.as_u64().unwrap_or(256);
        if n < 8 {
            format!("{}", if fg { 30 } else { 40 } + n)
        } else if n < 16 {
            format!("{}", if fg { 90 } else { 100 } + n - 8)
        } else if n < 256 {
            format!("{base};5;{n}")
        } else if n >= 259 && n <= 266 {
            format!("{base};5;{}", n - 259)
        } else {
            if fg { "39" } else { "49" }.into()
        }
    }
}
fn safe(s: &str) -> String {
    s.chars().filter(|c| !c.is_control()).collect()
}
fn window_identity(v: &Value) -> String {
    format!(
        "{} · {}:{}",
        elide(&safe(v["session_name"].as_str().unwrap_or("session")), 48),
        v["window_index"].as_u64().unwrap_or(0),
        elide(
            &safe(&status_commands::name(v, &v["window"], &v["name"])),
            96
        )
    )
}
struct StatusLayout {
    text: String,
    hits: Vec<(usize, usize, String)>,
}
fn elide(text: &str, columns: usize) -> String {
    if copy::width(text) <= columns {
        text.into()
    } else if columns == 0 {
        String::new()
    } else {
        format!("{}…", copy::clip(text, columns - 1))
    }
}
fn status_layout(v: &Value, size: (u16, u16), status: &str) -> StatusLayout {
    let columns = size.1 as usize;
    let rows = v["windows"].as_array().map(Vec::as_slice).unwrap_or(&[]);
    let shared = if v["rows"].as_u64().unwrap_or(size.0 as u64) != size.0 as u64
        || v["cols"].as_u64().unwrap_or(size.1 as u64) != size.1 as u64
    {
        format!(
            " · shared PTY {}×{}; view {}×{}",
            v["cols"], v["rows"], size.1, size.0
        )
    } else {
        String::new()
    };
    let identity = window_identity(v);
    let prefix_limit = if rows.is_empty() {
        columns
    } else {
        (columns * 2 / 3).min(columns.saturating_sub(10))
    };
    let prefix = if status.trim().is_empty() {
        format!("RUWAC {identity}{shared}")
    } else {
        // Preserve the actual notice before spending the remaining prefix
        // space on identity; the dedicated window list still shows selection.
        let notice = elide(&safe(status), prefix_limit.saturating_sub(15));
        let identity = elide(
            &identity,
            prefix_limit.saturating_sub(9 + copy::width(&notice)),
        );
        format!("RUWAC {identity} · {notice}{shared}")
    };
    let prefix = elide(&prefix, prefix_limit);
    let mut layout = StatusLayout {
        text: prefix,
        hits: Vec::new(),
    };
    if rows.is_empty() || columns.saturating_sub(copy::width(&layout.text)) < 4 {
        return layout;
    }
    layout.text.push_str(" · ");
    let base = copy::width(&layout.text);
    let available = columns.saturating_sub(base);
    let cap = (available / rows.len().min(4)).max(1);
    let active = rows
        .iter()
        .position(|r| r["id"] == v["window"])
        .unwrap_or(0);
    let labels: Vec<_> = rows
        .iter()
        .map(|r| {
            let selected = r["id"] == v["window"];
            let raw = format!(
                "{}:{}",
                r["index"].as_u64().unwrap_or(0),
                safe(&status_commands::name(v, &r["id"], &r["name"]))
            );
            format!(
                "{}{}",
                elide(&raw, cap.saturating_sub(usize::from(selected))),
                if selected { "*" } else { "" }
            )
        })
        .collect();
    let span = |start: usize, end: usize| {
        labels[start..=end]
            .iter()
            .map(|l| copy::width(l))
            .sum::<usize>()
            + end
            - start
    };
    let mut start = 0;
    if span(0, rows.len() - 1) > available {
        start = active;
        while start > 0
            && span(start - 1, active)
                + usize::from(start > 1) * 2
                + usize::from(active + 1 < rows.len()) * 2
                <= available
        {
            start -= 1;
        }
    }
    let mut used = 0;
    if start > 0 {
        layout.text.push_str("‹ ");
        if let Some(id) = rows[start - 1]["id"].as_str() {
            layout.hits.push((base, base + 2, id.into()));
        }
        used = 2;
    }
    let mut i = start;
    while i < rows.len() {
        let space = usize::from(i > start);
        let reserve = usize::from(i + 1 < rows.len()) * 2;
        let room = available.saturating_sub(used + space + reserve);
        if room == 0 {
            break;
        }
        if space > 0 {
            layout.text.push(' ');
            used += 1;
        }
        let label = elide(&labels[i], room);
        let width = copy::width(&label);
        if let Some(id) = rows[i]["id"].as_str() {
            layout
                .hits
                .push((base + used, base + used + width, id.into()));
        }
        layout.text.push_str(&label);
        used += width;
        i += 1;
    }
    if i < rows.len() && used + 2 <= available {
        layout.text.push_str(" ›");
        if let Some(id) = rows[i]["id"].as_str() {
            layout.hits.push((base + used, base + used + 2, id.into()));
        }
    }
    layout
}
fn status_line(v: &Value, size: (u16, u16), status: &str) -> String {
    status_layout(v, size, status).text
}
fn status_window_at(v: &Value, size: (u16, u16), status: &str, column: usize) -> Option<String> {
    status_layout(v, size, status)
        .hits
        .into_iter()
        .find_map(|(left, right, id)| (left <= column && column < right).then_some(id))
}
fn status_mouse(
    v: &Value,
    bytes: &[u8],
    screen: (u16, u16),
    status: &str,
) -> (bool, Option<String>) {
    let Some(text) = std::str::from_utf8(bytes)
        .ok()
        .and_then(|s| s.strip_prefix("\x1b[<"))
    else {
        return (false, None);
    };
    let Some(last) = text
        .as_bytes()
        .last()
        .copied()
        .filter(|b| matches!(b, b'M' | b'm'))
    else {
        return (false, None);
    };
    let parts: Vec<_> = text[..text.len() - 1]
        .split(';')
        .map(str::parse::<usize>)
        .collect();
    let [Ok(button), Ok(x), Ok(y)] = parts.as_slice() else {
        return (false, None);
    };
    if *y != screen.0 as usize {
        return (false, None);
    }
    let target = if last == b'M' && *button == 0 && *x > 0 {
        status_window_at(v, (screen.0.saturating_sub(1), screen.1), status, x - 1)
    } else {
        None
    };
    (true, target)
}

// Draw only reserved cells. Older daemons have adjacent pane rectangles;
// their application content must never be overwritten by a decorative line.
fn divider_cells(v: &Value) -> std::collections::BTreeMap<(u64, u64), u8> {
    let mut out = std::collections::BTreeMap::new();
    let Some(panes) = v["panes"].as_array() else {
        return out;
    };
    let rects: Vec<_> = panes
        .iter()
        .map(|p| {
            let r = &p["rect"];
            (
                r["x"].as_u64().unwrap_or(0),
                r["y"].as_u64().unwrap_or(0),
                r["cols"].as_u64().unwrap_or(0).min(1024),
                r["rows"].as_u64().unwrap_or(0).min(512),
            )
        })
        .collect();
    let empty = |x, y| {
        !rects
            .iter()
            .any(|&(a, b, w, h)| a <= x && x < a + w && b <= y && y < b + h)
    };
    for &(x, y, w, h) in &rects {
        if x > 0 {
            for row in y..y + h {
                if empty(x - 1, row) {
                    *out.entry((row, x - 1)).or_insert(0) |= 1;
                }
            }
        }
        if y > 0 {
            for col in x..x + w {
                if empty(col, y - 1) {
                    *out.entry((y - 1, col)).or_insert(0) |= 2;
                }
            }
        }
    }
    out
}
/// Two render frames show the same content (revision numbers aside).
fn same_frame(a: &Value, b: &Value) -> bool {
    let (Some(a), Some(b)) = (a.as_object(), b.as_object()) else {
        return false;
    };
    a.len() == b.len()
        && a.iter()
            .all(|(k, v)| k == "revision" || b.get(k).is_some_and(|w| w == v))
}
/// Acknowledge a new daemon revision even when another session caused it.
/// Otherwise every later poll asks for the same full, unchanged view again.
fn accept_render_frame(snapshot: &mut Value, mut fresh: Value) -> bool {
    if fresh["unchanged"] == true {
        return false;
    }
    // Client-only status metadata must not turn every daemon poll into a full
    // redraw, especially when RUWAC clients are nested inside one another.
    if let (Some(commands), Some(object)) = (snapshot.get("_commands"), fresh.as_object_mut()) {
        object.insert("_commands".into(), commands.clone());
    }
    let changed = !same_frame(&fresh, snapshot);
    *snapshot = fresh;
    changed
}
fn observe_running_window(frame: &Value, observed: &mut HashSet<String>) {
    if let Some(windows) = frame["windows"].as_array() {
        observed.retain(|id| {
            windows
                .iter()
                .any(|w| w["id"].as_str() == Some(id.as_str()))
        });
    }
    if frame["panes"]
        .as_array()
        .is_some_and(|panes| panes.iter().any(|p| p["ended"] != true))
    {
        if let Some(window) = frame["window"].as_str() {
            observed.insert(window.into());
        }
    }
}
/// Window index for Alt+0…9 (ESC followed by a digit).
fn alt_digit(key: &copy::Key) -> Option<usize> {
    match key {
        copy::Key::Sequence(s) => s
            .strip_prefix('\x1b')
            .filter(|d| d.len() == 1)
            .and_then(|d| d.chars().next())
            .and_then(|c| c.to_digit(10))
            .map(|d| d as usize),
        _ => None,
    }
}
fn mouse_wanted(v: &Value) -> bool {
    mouse_request(v).is_some()
}
/// The outer terminal's mouse mode: the program's own (clicks 1000, drags
/// 1002, all motion 1003), or drags for the client's scrollback and selection.
fn mouse_request(v: &Value) -> Option<u16> {
    let p = v["panes"]
        .as_array()?
        .iter()
        .find(|p| p["id"] == v["active_pane"])?;
    let g = &p["grid"];
    if g["mouse"] != true {
        // The client scrolls history and selects for programs without mouse
        // support; Shift keeps the outer terminal's own selection.
        Some(1002)
    } else if g["mouse_motion"] == true {
        Some(1003)
    } else if g["mouse_drag"] == true {
        Some(1002)
    } else {
        Some(1000)
    }
}
/// (button, column, row, release) from an SGR mouse report; 1-based.
fn sgr_mouse(bytes: &[u8]) -> Option<(u64, u64, u64, bool)> {
    let text = std::str::from_utf8(bytes).ok()?.strip_prefix("\x1b[<")?;
    let release = text.ends_with('m');
    if !release && !text.ends_with('M') {
        return None;
    }
    let mut parts = text[..text.len() - 1].split(';').map(str::parse::<u64>);
    Some((
        parts.next()?.ok()?,
        parts.next()?.ok()?,
        parts.next()?.ok()?,
        release,
    ))
}
/// An SGR mouse report from the outer terminal mapped to the pane under the
/// pointer: (pane, structured event in pane coordinates, pane wants mouse).
fn route_mouse(v: &Value, bytes: &[u8]) -> Option<(String, Value, bool)> {
    let (button, x, y, release) = sgr_mouse(bytes)?;
    let (col, row) = (x.checked_sub(1)?, y.checked_sub(1)?);
    v["panes"].as_array()?.iter().find_map(|p| {
        let r = &p["rect"];
        let (px, py) = (r["x"].as_u64()?, r["y"].as_u64()?);
        let (w, h) = (r["cols"].as_u64()?, r["rows"].as_u64()?);
        (px <= col && col < px + w && py <= row && row < py + h).then(|| {
            (
                p["id"].as_str().unwrap_or("").to_owned(),
                json!({"button":button,"x":col - px + 1,"y":row - py + 1,"release":release}),
                p["grid"]["mouse"] == true,
            )
        })
    })
}
const STATUS_STYLE: &str = "\x1b[0;38;2;16;32;48;48;2;152;198;232m";

/// Shared live/copy SGR: selection reverses colours without discarding attributes.
fn cell_style(cell: &Value, selected: bool) -> String {
    let st = format!(
        "0;{};{};{};{}",
        sgr(&cell[2], true),
        sgr(&cell[3], false),
        if cell[4] == true { 1 } else { 22 },
        if (cell[5] == true) != selected { 7 } else { 27 }
    );
    let mut st = st;
    let extra = &cell[6];
    if extra["italic"] == true {
        st.push_str(";3");
    }
    if extra["dim"] == true {
        st.push_str(";2");
    }
    if extra["hidden"] == true {
        st.push_str(";8");
    }
    if extra["strike"] == true {
        st.push_str(";9");
    }
    match extra["underline"].as_str() {
        Some("single") => st.push_str(";4"),
        Some("double") => st.push_str(";4:2"),
        Some("curly") => st.push_str(";4:3"),
        Some("dotted") => st.push_str(";4:4"),
        Some("dashed") => st.push_str(";4:5"),
        _ => (),
    }
    if let Some(rgb) = extra["underline_color"].as_array() {
        st.push_str(&format!(";58;2;{};{};{}", rgb[0], rgb[1], rgb[2]));
    } else if let Some(index) = extra["underline_color"].as_u64().filter(|i| *i <= 255) {
        st.push_str(&format!(";58;5;{index}"));
    }
    st
}
fn draw(v: &Value, status: &str) -> io::Result<()> {
    let mut out = format!(
        "\x1b]2;RUWAC {}\x07\x1b[?25l\x1b[0m\x1b[H{}",
        window_identity(v).chars().take(160).collect::<String>(),
        if CLEAR.swap(false, Ordering::Relaxed) {
            "\x1b[2J"
        } else {
            ""
        }
    );

    let (screen_rows, screen_cols) = terminal_dimensions();
    // Panes repaint every cell they own; erase only what lies outside them,
    // before drawing, so frames replace each other without a blank flash.
    let rects: Vec<(u64, u64, u64, u64)> = v["panes"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|p| {
            let r = &p["rect"];
            (
                r["x"].as_u64().unwrap_or(0),
                r["y"].as_u64().unwrap_or(0),
                r["cols"].as_u64().unwrap_or(0),
                r["rows"].as_u64().unwrap_or(0),
            )
        })
        .collect();
    for row in 0..u64::from(screen_rows.saturating_sub(1)) {
        let right = rects
            .iter()
            .filter(|&&(_, y, _, h)| y <= row && row < y + h)
            .map(|&(x, _, w, _)| x + w)
            .max()
            .unwrap_or(0);
        if right < u64::from(screen_cols) {
            out.push_str(&format!("\x1b[{};{}H\x1b[0m\x1b[K", row + 1, right + 1));
        }
    }
    let active = v["active_pane"].as_str().unwrap_or("");
    let mut cursor = None;
    if let Some(panes) = v["panes"].as_array() {
        for p in panes {
            let g = &p["grid"];
            let offset = p["rect"]["y"].as_u64().unwrap_or(0);
            let left = p["rect"]["x"].as_u64().unwrap_or(0);
            if p["id"] == active {
                let c = &g["cursor"];
                if c["visible"] == true {
                    cursor = Some((
                        offset + c["row"].as_u64().unwrap_or(0),
                        left + c["col"].as_u64().unwrap_or(0),
                    ))
                }
            }
            if let Some(rows) = g["cells"].as_array() {
                for (r, cells) in rows.iter().enumerate() {
                    out.push_str(&format!("\x1b[{};{}H", offset + r as u64 + 1, left + 1));
                    let mut style = String::new();
                    for cell in cells.as_array().unwrap() {
                        if cell[1] == true {
                            continue;
                        }
                        let st = cell_style(cell, false);
                        if st != style {
                            out.push_str(&format!("\x1b[{st}m"));
                            style = st;
                        }
                        out.push_str(&safe(cell[0].as_str().unwrap_or(" ")));
                    }
                }
            }
        }
    }
    for ((row, col), direction) in divider_cells(v) {
        let glyph = match direction {
            1 => "│",
            2 => "─",
            _ => "┼",
        };
        out.push_str(&format!(
            "\x1b[{};{}H\x1b[0;38;2;120;170;205m{glyph}\x1b[0m",
            row + 1,
            col + 1
        ));
    }
    // The daemon bounds the PTY grid, but chrome belongs to the physical
    // terminal. Keep the bar at its bottom even beyond the engine grid limits.
    let line = status_line(v, (screen_rows.saturating_sub(1), screen_cols), status);
    out.push_str(&format!(
        "\x1b[{};1H{}{}{}\x1b[0m",
        screen_rows,
        STATUS_STYLE,
        line,
        " ".repeat((screen_cols as usize).saturating_sub(copy::width(&line)))
    ));
    // Request mouse reports only while the active program uses the mouse;
    // otherwise the outer terminal keeps native selection and middle-click
    // (touchpad three-finger tap) paste.
    out.push_str(match mouse_request(v) {
        Some(1003) => "\x1b[?1002l\x1b[?1000h\x1b[?1003h\x1b[?1006h",
        Some(1002) => "\x1b[?1003l\x1b[?1000h\x1b[?1002h\x1b[?1006h",
        Some(_) => "\x1b[?1002l\x1b[?1003l\x1b[?1000h\x1b[?1006h",
        None => "\x1b[?1000l\x1b[?1002l\x1b[?1003l\x1b[?1006l",
    });
    if let Some((r, c)) = cursor {
        out.push_str(&format!("\x1b[{};{}H\x1b[?25h", r + 1, c + 1));
    }
    let mut stdout = io::stdout().lock();
    stdout.write_all(out.as_bytes())?;
    stdout.flush()
}
#[derive(Default)]
struct Config {
    prefix: u8,
    remaps: std::collections::HashMap<char, char>,
    double_prefix_previous: bool,
}
impl Config {
    fn load() -> io::Result<Self> {
        let mut c = Self {
            prefix: 1,
            ..Self::default()
        };
        if let Ok(value) = std::env::var("RUWAC_SHORTCUTS") {
            let map: std::collections::HashMap<String, String> =
                serde_json::from_str(&value).map_err(io::Error::other)?;
            if let Some(action) = map.get("double_prefix") {
                c.double_prefix_previous = match action.as_str() {
                    "literal" => false,
                    "previous" => true,
                    _ => {
                        return Err(io::Error::other(
                            "double_prefix must be literal or previous",
                        ));
                    }
                };
            }
            if let Some(prefix) = map.get("prefix") {
                let letter = prefix
                    .strip_prefix("<Control>")
                    .or_else(|| prefix.strip_prefix("Ctrl+"))
                    .or_else(|| prefix.strip_prefix("Ctrl-"))
                    .or_else(|| prefix.strip_prefix('^'));
                c.prefix = letter
                    .filter(|s| s.len() == 1 && s.as_bytes()[0].is_ascii_alphabetic())
                    .map(|s| s.as_bytes()[0].to_ascii_lowercase() & 31)
                    .ok_or_else(|| {
                        io::Error::other("TUI prefix must be Control+A through Control+Z")
                    })?;
            }
        }
        if let Ok(value) = std::env::var("RUWAC_KEYMAP") {
            let map: std::collections::HashMap<String, String> =
                serde_json::from_str(&value).map_err(io::Error::other)?;
            for (from, to) in map {
                let from = if from == "Tab" {
                    Some('\t')
                } else {
                    from.chars().next().filter(|_| from.len() == 1)
                };
                if let Some(from) = from {
                    let to = if to == "Tab" {
                        '\t'
                    } else {
                        to.chars().next().filter(|_| to.len() == 1).ok_or_else(|| {
                            io::Error::other("TUI remaps require one implemented prefix command")
                        })?
                    };
                    if !"acnpodS|\t[]\"?:z0123456789".contains(to) {
                        return Err(io::Error::other("Unsupported TUI prefix remap"));
                    }
                    c.remaps.insert(from, to);
                }
            }
        }
        Ok(c)
    }
    fn label(&self) -> String {
        format!("^{}", (self.prefix + b'@') as char)
    }
}
struct WindowRow {
    id: String,
    name: String,
}
enum Mode {
    Live,
    Copy(copy::Copy),
    Chooser {
        rows: Vec<WindowRow>,
        index: usize,
        typed: String,
    },
    Prompt(String),
    Confirm {
        panes: Vec<String>,
        label: String,
    },
    Help(usize),
}
fn history_rows(page: &Value, legacy: bool) -> io::Result<Vec<copy::Line>> {
    if legacy {
        let flags = page["wrapped"]
            .as_array()
            .filter(|a| a.len() == 1)
            .ok_or_else(|| io::Error::other("Invalid legacy history row"))?;
        let wrapped = flags[0]
            .as_bool()
            .ok_or_else(|| io::Error::other("Invalid legacy wrap flag"))?;
        let text = page["text"]
            .as_str()
            .ok_or_else(|| io::Error::other("Missing legacy text"))?;
        let text = if wrapped {
            text
        } else {
            text.strip_suffix('\n')
                .ok_or_else(|| io::Error::other("Missing history newline"))?
        };
        if text.chars().any(char::is_control) {
            return Err(io::Error::other("History row controls rejected"));
        }
        return Ok(vec![copy::Line {
            text: text.into(),
            wrapped,
        }]);
    }
    page["rows"]
        .as_array()
        .ok_or_else(|| io::Error::other("Canonical history rows unavailable"))?
        .iter()
        .map(|row| {
            let text = row["text"]
                .as_str()
                .filter(|t| !t.chars().any(char::is_control))
                .ok_or_else(|| io::Error::other("Invalid canonical row"))?;
            let wrapped = row["wrapped"]
                .as_bool()
                .ok_or_else(|| io::Error::other("Canonical wrap flag missing"))?;
            Ok(copy::Line {
                text: text.into(),
                wrapped,
            })
        })
        .collect()
}

fn history(s: &mut UnixStream, pane: &str) -> io::Result<copy::Copy> {
    let metadata = match call(s, "pane.history.freeze", json!({"pane":pane})) {
        Ok(metadata) if metadata["snapshot"].as_str().is_some() => metadata,
        Ok(_) => return Err(io::Error::other("History snapshot ID missing")),
        Err(e)
            if serde_json::from_str::<Value>(&e.to_string())
                .ok()
                .is_some_and(|v| v["code"] == "METHOD_NOT_FOUND") =>
        {
            // Preserved older engines expose epoch-checked canonical history,
            // but cannot pin it. Never restart them or combine changing pages.
            let mut page = call(s, "pane.history", json!({"pane":pane,"offset":0,"lines":1}))?;
            let render = call(s, "client.render", json!({}))?;
            let panes = call(s, "pane.list", json!({"session":render["session"]}))?;
            let geometry = panes
                .as_array()
                .and_then(|rows| rows.iter().find(|p| p["id"] == pane))
                .ok_or_else(|| io::Error::other("History pane geometry unavailable"))?;
            page["cols"] = geometry["cols"].clone();
            page["cursor_row"] = json!(page["total_lines"].as_u64().unwrap_or(1).saturating_sub(1));
            page["cursor_col"] = json!(0);
            page
        }
        Err(e) => return Err(e),
    };
    let pin = metadata["snapshot"].as_str().map(str::to_owned);
    let result = (|| -> io::Result<copy::Copy> {
        let total = metadata["total_lines"]
            .as_u64()
            .filter(|n| *n > 0)
            .ok_or_else(|| io::Error::other("History snapshot is empty"))?
            as usize;
        let cols = metadata["cols"]
            .as_u64()
            .filter(|n| *n > 0 && *n <= u64::from(ruwac_protocol::TerminalLimits::CURRENT.cols))
            .ok_or_else(|| io::Error::other("History columns rejected"))?
            as usize;
        // Long-lived sessions keep the newest lines in view: at most 50,000
        // lines and 8 MiB, dropping the oldest instead of refusing.
        let mut lines = std::collections::VecDeque::new();
        // v0.1.x has no canonical rows: one row per response is the only
        // unambiguous way to retain Unicode, wide-character padding and wraps.
        let legacy_rows = pin.is_none() && metadata.get("rows").is_none();
        let mut offset = total.saturating_sub(if legacy_rows { 500 } else { 50_000 });
        // Index in the full history of the first line kept.
        let mut first = offset;
        let mut bytes = 0;
        while offset < total {
            let mut params = json!({"pane":pane,"offset":offset,"lines":if legacy_rows {1}else{500},"epoch":metadata["epoch"]});
            if let Some(pin) = &pin {
                params["snapshot"] = json!(pin);
            }
            let page = call(s, "pane.history", params)?;
            if page["epoch"] != metadata["epoch"] || page["total_lines"] != metadata["total_lines"]
            {
                return Err(io::Error::other("Inconsistent history snapshot rejected"));
            }
            let rows = history_rows(&page, legacy_rows)?;
            for row in &rows {
                bytes += row.text.len() + 1;
                lines.push_back(row.clone());
                while bytes > 8 * 1024 * 1024 {
                    match lines.pop_front() {
                        Some(old) => {
                            bytes -= old.text.len() + 1;
                            first += 1;
                        }
                        None => break,
                    }
                }
            }
            let next = page["next_offset"].as_u64().unwrap_or(0) as usize;
            if next != offset + rows.len() || next <= offset || next > total {
                return Err(io::Error::other("History paging failed"));
            }
            offset = next;
        }
        let mut copied = copy::Copy::new(
            lines.into(),
            cols,
            copy::Position {
                row: (metadata["cursor_row"].as_u64().unwrap_or(0) as usize).saturating_sub(first),
                col: metadata["cursor_col"].as_u64().unwrap_or(0) as usize,
            },
            metadata["dropped_lines"].as_u64().unwrap_or(0) as usize,
            pane.into(),
        );
        if legacy_rows {
            copied.message = "Legacy daemon: newest 500 rows · Esc returns to terminal".into();
        }
        Ok(copied)
    })();
    let release = if let Some(pin) = pin {
        call(s, "pane.history.release", json!({"snapshot":pin})).map(|_| ())
    } else {
        // One final epoch check prevents accepting an output/resize race after
        // the last unpinned page. The collected client copy is immutable.
        call(
            s,
            "pane.history",
            json!({"pane":pane,"epoch":metadata["epoch"],"offset":0,"lines":1}),
        )
        .and_then(|page| {
            if page["epoch"] == metadata["epoch"] && page["total_lines"] == metadata["total_lines"]
            {
                Ok(())
            } else {
                Err(io::Error::other("History changed; retry scrollback"))
            }
        })
    };
    match result {
        Ok(value) => {
            release?;
            Ok(value)
        }
        Err(e) => Err(e),
    }
}
fn windows(s: &mut UnixStream, session: &str, current: &Value) -> io::Result<Mode> {
    let v = call(s, "window.list", json!({"session":session}))?;
    let data = v
        .as_array()
        .ok_or_else(|| io::Error::other("Window list missing"))?;
    if data.is_empty() || data.len() > 128 {
        return Err(io::Error::other("Window chooser bound rejected"));
    }
    let mut rows = Vec::new();
    for row in data {
        let id = row["id"]
            .as_str()
            .filter(|id| ruwac_protocol::valid_id(id, "window"))
            .ok_or_else(|| io::Error::other("Window ID rejected"))?;
        rows.push(WindowRow {
            id: id.into(),
            name: safe(row["name"].as_str().unwrap_or("Window")),
        });
    }
    let index = rows
        .iter()
        .position(|row| current.as_str() == Some(&row.id))
        .unwrap_or(0);
    Ok(Mode::Chooser {
        rows,
        index,
        typed: String::new(),
    })
}
fn plain(s: &mut UnixStream, v: &Value, text: &mut String) -> io::Result<()> {
    if text.is_empty() {
        return Ok(());
    }
    let text = std::mem::take(text);
    match call(
        s,
        "client.input",
        json!({"pane":v["active_pane"],"kind":"keyboard","text":text}),
    ) {
        // Typing into restored history (e.g. after a reboot) or an exited
        // program opens a fresh shell window in the same session and delivers
        // the keys there; ended windows remain readable history.
        Err(e) if e.to_string().contains("PANE_EXITED") => {
            call(s, "window.create", json!({"session":v["session"]}))?;
            let fresh = call(s, "client.render", json!({}))?;
            // Shell startup can discard typeahead; wait for its prompt.
            let deadline = std::time::Instant::now() + Duration::from_secs(3);
            while std::time::Instant::now() < deadline {
                let screen = call(
                    s,
                    "pane.capture",
                    json!({"pane":fresh["active_pane"],"lines":50}),
                )?;
                if screen["text"]
                    .as_str()
                    .is_some_and(|t| !t.trim().is_empty())
                {
                    break;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            call(
                s,
                "client.input",
                json!({"pane":fresh["active_pane"],"kind":"keyboard","text":text}),
            )?;
            Ok(())
        }
        other => other.map(|_| ()),
    }
}
fn command(
    s: &mut UnixStream,
    session: &str,
    snapshot: &Value,
    text: &str,
    mode: &mut Mode,
) -> io::Result<()> {
    let trimmed = text.trim();
    let (name, arg) = trimmed
        .split_once(char::is_whitespace)
        .unwrap_or((trimmed, ""));
    let name = ruwac_protocol::screen_command(name);
    let arg = if name == "split" && arg.trim() == "-v" {
        "vertical"
    } else if name == "split" && arg.trim().is_empty() {
        "horizontal"
    } else {
        arg.trim()
    };
    if !arg.is_empty() && !matches!(name, "select" | "rename" | "split") {
        return Err(io::Error::other("This command takes no arguments"));
    }
    match name {
        "" => (),
        "new" => {
            call(s, "window.create", json!({"session":session}))?;
        }
        "next" | "prev" => {
            call(
                s,
                "window.cycle",
                json!({"session":session,"previous":name=="prev"}),
            )?;
        }
        "other" => {
            call(s, "window.previous", json!({"session":session}))?;
        }
        "select" => {
            let index: usize = arg
                .parse()
                .map_err(|_| io::Error::other("select requires a window index"))?;
            let rows = call(s, "window.list", json!({"session":session}))?;
            let row = rows
                .as_array()
                .and_then(|a| a.get(index))
                .ok_or_else(|| io::Error::other("Window index out of range"))?;
            call(s, "window.select", json!({"window":row["id"]}))?;
        }
        "rename" => {
            if arg.is_empty() {
                return Err(io::Error::other("rename requires a name"));
            }
            call(
                s,
                "window.rename",
                json!({"window":snapshot["window"],"name":arg}),
            )?;
        }
        "split" => {
            if !matches!(arg, "horizontal" | "vertical") {
                return Err(io::Error::other("split requires horizontal or vertical"));
            }
            call(
                s,
                "pane.create",
                json!({"window":snapshot["window"],"orientation":arg}),
            )?;
        }
        "zoom" | "only" => {
            call(
                s,
                "pane.layout",
                json!({"window":snapshot["window"],"zoom":name == "only" || snapshot["zoom"]!=true}),
            )?;
        }
        "copy" => {
            *mode = Mode::Copy(history(
                s,
                snapshot["active_pane"]
                    .as_str()
                    .ok_or_else(|| io::Error::other("Active pane missing"))?,
            )?)
        }
        "paste" => {
            call(s, "buffer.paste", json!({"pane":snapshot["active_pane"]}))?;
        }
        "kill" | "quit" => {
            let rows = call(s, "pane.list", json!({"session":session}))?;
            let panes = rows
                .as_array()
                .ok_or_else(|| io::Error::other("Pane list unavailable"))?
                .iter()
                .filter(|p| name == "quit" || p["window"] == snapshot["window"])
                .filter_map(|p| p["id"].as_str().map(str::to_owned))
                .collect();
            *mode = Mode::Confirm {
                panes,
                label: if name == "quit" {
                    "Close ALL windows in this session"
                } else {
                    "Kill this window and its programs"
                }
                .into(),
            };
        }
        "focus" => {
            let panes = snapshot["panes"]
                .as_array()
                .filter(|p| !p.is_empty())
                .ok_or_else(|| io::Error::other("Pane list empty"))?;
            let i = panes
                .iter()
                .position(|p| p["id"] == snapshot["active_pane"])
                .unwrap_or(0);
            call(
                s,
                "pane.select",
                json!({"pane":panes[(i+1)%panes.len()]["id"]}),
            )?;
        }
        "windows" => *mode = windows(s, session, &snapshot["window"])?,
        "help" => *mode = Mode::Help(0),
        "detach" => {
            call(s, "session.detach", json!({}))?;
            STOP.store(true, Ordering::Relaxed);
        }
        _ => {
            return Err(io::Error::other(
                "Unknown command; help lists implemented commands",
            ));
        }
    }
    Ok(())
}
fn draw_mode(
    mode: &mut Mode,
    snapshot: &Value,
    size: (u16, u16),
    status: &str,
    config: &Config,
) -> io::Result<()> {
    if matches!(mode, Mode::Live) {
        return draw(snapshot, &format!("{} · {}", config.label(), status));
    }
    let height = size.0 as usize;
    let columns = size.1 as usize;
    let mut out = if let Mode::Copy(copy) = mode {
        copy.preserve_live_styles(snapshot);
        copy.render(height, columns)
    } else {
        let mut out = String::from(
            "\x1b[?25l\x1b[?1000l\x1b[?1002l\x1b[?1003l\x1b[?1006l\x1b[0m\x1b[H\x1b[2J",
        );
        match mode {
            Mode::Chooser { rows, index, .. } => {
                let top = index.saturating_sub(height.saturating_sub(1));
                for (y, row) in rows.iter().enumerate().skip(top).take(height) {
                    out.push_str(&format!(
                        "\x1b[{};1H{}{}\x1b[0m",
                        y - top + 1,
                        if y == *index { "\x1b[7m" } else { "" },
                        copy::clip(&format!("{:>3}  {}", y, row.name), columns)
                    ));
                }
                out.push_str(&format!(
                    "\x1b[{};1H{STATUS_STYLE}{}\x1b[0m",
                    height + 1,
                    copy::clip(
                        &format!(
                            "WINDOWS · arrows/digits, Enter select, Esc cancel · {}",
                            status
                        ),
                        columns
                    )
                ));
            }
            Mode::Confirm { label, .. } => {
                out.push_str(&format!(
                    "\x1b[{};1H{STATUS_STYLE}{}\x1b[0m",
                    height + 1,
                    copy::clip(
                        &format!("{label}? y confirms; any other key cancels"),
                        columns
                    )
                ));
            }
            Mode::Prompt(text) => {
                out.push_str(&copy::clip("RUWAC command · help · new · next/prev · other · select N · rename NAME · split horizontal/vertical · zoom · copy · paste · detach",columns));
                out.push_str(&format!(
                    "\x1b[{};1H:{}\x1b[?25h",
                    height + 1,
                    copy::clip(text, columns.saturating_sub(1))
                ));
            }
            Mode::Help(offset) => {
                let text = format!(
                    "RUWAC Screen controls (prefix {})\nc new window; n/p next/previous; double prefix forwards to nested terminal; o previous window\na literal configured prefix; d detach; 0–9 select window\nS horizontal split; | vertical split; Tab change pane; z zoom\n[ copy/search; ] internal paste; \" window chooser; : command\nCopy: arrows/hjkl; g/G and Home/End; 0/^/$; w/b/e\nPage Up/Down or Ctrl+B/F; Ctrl+U/D half page; H/M/L viewport\nSpace or Enter first mark; move then mark again to copy\n/ forward search; ? reverse; n/N repeat; searches wrap\nv rectangular selection; c/C left/right margins; Esc cancel\nCounts before movement; Unicode graphemes stay intact\nInternal copy buffer is separate from desktop clipboard\nAuto-update (from shell): ruwac update -a on|off|status\nUpdates preserve running daemons and applications\nCopy snapshot: newest 8 MiB/50,000 rows; selection cap 256 KiB\nCommands: new, next, prev, other, select N, rename NAME\nsplit horizontal/vertical, zoom, copy, paste, windows, help, detach\nRUWAC_SHORTCUTS prefix: <Control>a through <Control>z\nRUWAC_KEYMAP remaps one prefix key to an implemented key\nCtrl/arrow escape sequences go to applications outside modals\nModal paste/mouse cannot reach a child; search/prompt paste is text\nEsc/Enter close help; arrows/Page Up/Down scroll",
                    config.label()
                );
                let lines: Vec<_> = text.lines().collect();
                *offset = (*offset).min(lines.len().saturating_sub(height));
                for (y, line) in lines.iter().skip(*offset).take(height).enumerate() {
                    out.push_str(&format!("\x1b[{};1H{}", y + 1, copy::clip(line, columns)));
                }
                out.push_str(&format!(
                    "\x1b[{};1H{STATUS_STYLE}HELP · Esc/Enter exit\x1b[0m",
                    height + 1
                ));
            }
            _ => (),
        }
        out
    };
    if !status.is_empty() && !matches!(mode, Mode::Copy(_) | Mode::Chooser { .. }) {
        out.push_str(&format!(
            "\x1b[{};1H{STATUS_STYLE}{}\x1b[0m",
            height + 1,
            copy::clip(status, columns)
        ));
    }
    let mut stdout = io::stdout().lock();
    stdout.write_all(out.as_bytes())?;
    stdout.flush()
}

fn verified_connect(socket: &Path) -> io::Result<UnixStream> {
    use std::os::unix::{
        fs::{FileTypeExt, MetadataExt},
        io::AsRawFd,
    };
    for (path, mode) in [
        (
            socket
                .parent()
                .ok_or_else(|| io::Error::other("Socket parent missing"))?,
            0o700,
        ),
        (socket, 0o600),
    ] {
        let m = std::fs::symlink_metadata(path)?;
        if m.uid() != unsafe { libc::geteuid() }
            || m.mode() & 0o777 != mode
            || m.file_type().is_symlink()
        {
            return Err(io::Error::other("Socket ownership/mode rejected"));
        }
    }
    if !std::fs::symlink_metadata(socket)?.file_type().is_socket() {
        return Err(io::Error::other("Not a socket"));
    }
    let s = UnixStream::connect(socket)?;
    let mut cred: libc::ucred = unsafe { std::mem::zeroed() };
    let mut n = std::mem::size_of_val(&cred) as libc::socklen_t;
    if unsafe {
        libc::getsockopt(
            s.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            &mut cred as *mut _ as *mut _,
            &mut n,
        )
    } != 0
        || cred.uid != unsafe { libc::geteuid() }
    {
        return Err(io::Error::other("Server peer UID rejected"));
    }
    s.set_read_timeout(Some(Duration::from_secs(5)))?;
    s.set_write_timeout(Some(Duration::from_secs(5)))?;
    Ok(s)
}
pub fn attach(socket: &Path, session: &str) -> io::Result<()> {
    attach_with_window(socket, session, None)
}
pub fn attach_with_window(socket: &Path, session: &str, window: Option<&str>) -> io::Result<()> {
    STOP.store(false, Ordering::Relaxed);
    let config = Config::load()?;
    let mut s = verified_connect(socket)?;
    let panes = call(&mut s, "pane.list", json!({"session":session}))?;
    let mut ancestor = std::process::id();
    for _ in 0..128 {
        if panes.as_array().is_some_and(|rows| {
            rows.iter()
                .any(|p| p["ended"] != true && p["pid"].as_u64() == Some(ancestor as u64))
        }) {
            return Err(io::Error::other(
                "Cannot attach a session from inside its own pane; use -S with a different session name for nesting",
            ));
        }
        let Ok(stat) = std::fs::read_to_string(format!("/proc/{ancestor}/stat")) else {
            break;
        };
        let Some(parent) = stat
            .rsplit_once(") ")
            .and_then(|(_, tail)| tail.split_whitespace().nth(1))
            .and_then(|p| p.parse::<u32>().ok())
        else {
            break;
        };
        if parent == 0 || parent == ancestor {
            break;
        }
        ancestor = parent;
    }
    let limits =
        ruwac_protocol::TerminalLimits::advertised(&call(&mut s, "capabilities", json!({}))?);
    if let Some(window) = window {
        if !ruwac_protocol::valid_id(window, "window")
            || !call(&mut s, "window.list", json!({"session":session}))?
                .as_array()
                .is_some_and(|rows| rows.iter().any(|w| w["id"].as_str() == Some(window)))
        {
            return Err(io::Error::other(
                "Requested window does not belong to the selected session",
            ));
        }
    }
    let _raw = Raw::enter()?;
    let mut screen_size = terminal_dimensions();
    // Left-button press on a pane without mouse support: (pane, column, row).
    let mut drag: Option<(String, usize, usize)> = None;
    // Middle-click awaiting the outer terminal's selection: (pane, asked at).
    let mut primary_paste: Option<(String, Instant)> = None;
    let mut size = dimensions(limits);
    call(
        &mut s,
        "session.attach",
        json!({"session":session,"rows":size.0,"cols":size.1}),
    )?;
    if let Some(window) = window {
        call(&mut s, "window.select", json!({"window":window}))?;
    }
    let (tx, rx) = mpsc::sync_channel(64);
    std::thread::spawn(move || {
        let mut stdin = io::stdin().lock();
        let mut b = [0; 8192];
        loop {
            match stdin.read(&mut b) {
                Ok(0) | Err(_) => {
                    let _ = tx.send(None);
                    break;
                }
                Ok(n) => {
                    if tx.send(Some(b[..n].to_vec())).is_err() {
                        break;
                    }
                }
            }
        }
    });
    let mut decoder = Decoder::default();
    let mut prefix = false;
    let mut mode = Mode::Live;
    let mut snapshot = call(&mut s, "client.render", json!({}))?;
    status_commands::refresh(&mut snapshot, |m, p| call(&mut s, m, p));
    let mut running_windows = HashSet::new();
    observe_running_window(&snapshot, &mut running_windows);
    let mut status = String::new();
    draw_mode(&mut mode, &snapshot, size, &status, &config)?;
    let mut rendered = Instant::now();
    let mut commands_refreshed = Instant::now();
    let mut input_at = Instant::now();
    while !STOP.load(Ordering::Relaxed) {
        // Briefly poll faster after input so local echo is not delayed by the
        // ordinary output refresh cadence. Quiet terminals keep their old rate.
        let interactive = input_at.elapsed() < Duration::from_millis(150);
        let events = match rx.recv_timeout(Duration::from_millis(if interactive { 4 } else { 10 }))
        {
            Ok(Some(b)) => {
                input_at = Instant::now();
                decoder.feed(&b)
            }
            Ok(None) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(_) => {
                if input_at.elapsed() > Duration::from_millis(40) {
                    decoder.escape_timeout()
                } else {
                    vec![]
                }
            }
        };
        let mut dirty = !events.is_empty();
        if primary_paste
            .as_ref()
            .is_some_and(|(_, at)| at.elapsed() > Duration::from_millis(400))
        {
            let (pane, _) = primary_paste.take().unwrap();
            if let Err(e) = call(&mut s, "buffer.paste", json!({"pane":pane})) {
                status = e.to_string();
            }
            dirty = true;
        }
        for event in events {
            match event {
                Input::Selection(text) => {
                    if let Some((pane, _)) = primary_paste.take() {
                        if let Err(e) = call(
                            &mut s,
                            "client.input",
                            json!({"pane":pane,"kind":"paste","text":text}),
                        ) {
                            status = e.to_string();
                        }
                    }
                }
                Input::Rejected(e) => {
                    prefix = false;
                    status = e.into();
                    if let Mode::Copy(c) = &mut mode {
                        c.message = status.clone();
                    }
                }
                Input::Mouse(b) => {
                    if let Mode::Copy(c) = &mut mode {
                        // Scrollback opened by the wheel or a drag.
                        let Some((button, x, y, release)) = sgr_mouse(&b) else {
                            continue;
                        };
                        let (height, row, col) = (
                            size.0 as usize,
                            y.saturating_sub(1) as usize,
                            x.saturating_sub(1) as usize,
                        );
                        match button {
                            64 => c.scroll(-3, height),
                            65 => {
                                c.scroll(3, height);
                                if c.at_bottom(height) && c.anchor.is_none() {
                                    mode = Mode::Live;
                                    status.clear();
                                }
                            }
                            0 if !release => {
                                c.point(row, col);
                                c.mark();
                            }
                            0 | 32 => {
                                c.point(row, col);
                                if release && c.anchor.is_some() {
                                    let text = c.selected(MAX_TEXT).map_err(io::Error::other)?;
                                    let bytes = text.len();
                                    offer_selection(&text);
                                    call(
                                        &mut s,
                                        "buffer.set",
                                        json!({"session":session,"text":text}),
                                    )?;
                                    mode = Mode::Live;
                                    status = format!("Copied {bytes} bytes; middle-click pastes");
                                }
                            }
                            _ => (),
                        }
                        continue;
                    }
                    if matches!(mode, Mode::Live) {
                        let (bar, target) = status_mouse(
                            &snapshot,
                            &b,
                            screen_size,
                            &format!("{} · {}", config.label(), status),
                        );
                        if bar {
                            if let Some(window) = target {
                                call(&mut s, "window.select", json!({"window":window}))?;
                                snapshot = call(&mut s, "client.render", json!({}))?;
                                prefix = false;
                                status.clear();
                            }
                            continue;
                        }
                        // Route to the pane under the pointer, in that pane's
                        // coordinates; the engine encodes it for the program.
                        let Some((pane, event, wants)) = route_mouse(&snapshot, &b) else {
                            continue;
                        };
                        let press = event["release"] != true
                            && event["button"].as_u64().is_some_and(|b| b & 32 == 0);
                        if press && snapshot["active_pane"].as_str() != Some(pane.as_str()) {
                            call(&mut s, "pane.select", json!({"pane":pane}))?;
                            snapshot = call(&mut s, "client.render", json!({}))?;
                        }
                        if wants {
                            let result = call(
                                &mut s,
                                "client.input",
                                json!({"pane":pane,"kind":"mouse","text":"","mouse_event":event}),
                            );
                            if let Err(e) = result {
                                status = e.to_string();
                            }
                            continue;
                        }
                        // A program without mouse support (e.g. a shell): the
                        // wheel opens scrollback, a drag selects into the
                        // session buffer and a middle-click pastes it.
                        let height = size.0 as usize;
                        let (x, y) = (
                            event["x"].as_u64().unwrap_or(1).saturating_sub(1) as usize,
                            event["y"].as_u64().unwrap_or(1).saturating_sub(1) as usize,
                        );
                        let release = event["release"] == true;
                        match event["button"].as_u64().unwrap_or(0) {
                            64 => match history(&mut s, &pane) {
                                Ok(mut c) => {
                                    c.align_bottom(height);
                                    c.scroll(-3, height);
                                    mode = Mode::Copy(c);
                                }
                                Err(e) => status = format!("Scrollback unavailable: {e}"),
                            },
                            0 if !release => drag = Some((pane.clone(), x, y)),
                            32 => {
                                if let Some((start_pane, sx, sy)) = drag.take() {
                                    if start_pane == pane {
                                        let rows = snapshot["panes"]
                                            .as_array()
                                            .and_then(|p| {
                                                p.iter().find(|p| p["id"] == pane.as_str())
                                            })
                                            .and_then(|p| p["rect"]["rows"].as_u64())
                                            .unwrap_or(height as u64)
                                            as usize;
                                        // Pane row r is history line len-rows+r.
                                        let offset = height.saturating_sub(rows);
                                        match history(&mut s, &pane) {
                                            Ok(mut c) => {
                                                c.align_bottom(height);
                                                c.point(offset + sy, sx);
                                                c.mark();
                                                c.point(offset + y, x);
                                                mode = Mode::Copy(c);
                                            }
                                            Err(e) => {
                                                status = format!("Selection unavailable: {e}")
                                            }
                                        }
                                    }
                                }
                            }
                            // Middle-click pastes the desktop mouse selection
                            // when the outer terminal answers OSC 52 promptly,
                            // otherwise the session buffer.
                            1 if !release => {
                                let mut stdout = io::stdout().lock();
                                stdout.write_all(b"\x1b]52;p;?\x07")?;
                                stdout.flush()?;
                                primary_paste = Some((pane.clone(), Instant::now()));
                            }
                            _ => {
                                if release {
                                    drag = None;
                                }
                            }
                        }
                    }
                }
                Input::Paste(b) => {
                    prefix = false;
                    match String::from_utf8(b) {
                        Ok(text) => match &mut mode {
                            Mode::Live => {
                                if let Err(e) = call(
                                    &mut s,
                                    "client.input",
                                    json!({"pane":snapshot["active_pane"],"kind":"paste","text":text}),
                                ) {
                                    status = e.to_string();
                                }
                            }
                            Mode::Copy(c) => c.paste_query(&text),
                            Mode::Prompt(prompt) => {
                                for c in text.chars().filter(|c| !c.is_control()) {
                                    if prompt.len() + c.len_utf8() > 4096 {
                                        break;
                                    }
                                    prompt.push(c);
                                }
                                status.clear();
                            }
                            _ => status = "Paste ignored in this view".into(),
                        },
                        Err(e) => status = e.to_string(),
                    }
                }
                Input::Keyboard(b) => {
                    let text = String::from_utf8(b).map_err(io::Error::other)?;
                    let mut pending = String::new();
                    for key in copy::keys(&text) {
                        let mut refresh = false;
                        let result = (|| -> io::Result<()> {
                            match &mut mode {
                                Mode::Confirm { panes, .. } => {
                                    if matches!(key, copy::Key::Char('y' | 'Y')) {
                                        for pane in std::mem::take(panes) {
                                            call(&mut s, "pane.close", json!({"pane":pane}))?;
                                        }
                                        if call(&mut s, "session.list", json!({}))?
                                            .as_array()
                                            .is_none_or(|rows| {
                                                !rows.iter().any(|r| r["id"] == session)
                                            })
                                        {
                                            STOP.store(true, Ordering::Relaxed);
                                        }
                                    }
                                    mode = Mode::Live;
                                    refresh = true;
                                }
                                Mode::Copy(c) => match c.key(&key, size.0 as usize) {
                                    copy::Action::Cancel => {
                                        mode = Mode::Live;
                                        status = "Copy cancelled; buffer unchanged".into();
                                        refresh = true;
                                    }
                                    copy::Action::Save => {
                                        let text =
                                            c.selected(MAX_TEXT).map_err(io::Error::other)?;
                                        let bytes = text.len();
                                        call(
                                            &mut s,
                                            "buffer.set",
                                            json!({"session":session,"text":text}),
                                        )?;
                                        mode = Mode::Live;
                                        status = format!("Copied {bytes} bytes internally");
                                        refresh = true;
                                    }
                                    _ => (),
                                },
                                Mode::Chooser { rows, index, typed } => {
                                    if key.escape() || matches!(key, copy::Key::Char('q' | '\x03'))
                                    {
                                        mode = Mode::Live;
                                        status = "Window choice cancelled".into();
                                        refresh = true;
                                    } else {
                                        match &key {
                                            copy::Key::Char('\r' | '\n') => {
                                                call(
                                                    &mut s,
                                                    "window.select",
                                                    json!({"window":rows[*index].id}),
                                                )?;
                                                mode = Mode::Live;
                                                status.clear();
                                                refresh = true;
                                            }
                                            copy::Key::Char('j' | '\x0e') => {
                                                *index = (*index + 1).min(rows.len() - 1);
                                                typed.clear();
                                            }
                                            copy::Key::Char('k' | '\x10') => {
                                                *index = index.saturating_sub(1);
                                                typed.clear();
                                            }
                                            copy::Key::Char('g') => {
                                                *index = 0;
                                                typed.clear();
                                            }
                                            copy::Key::Char('G') => {
                                                *index = rows.len() - 1;
                                                typed.clear();
                                            }
                                            copy::Key::Char(c @ '0'..='9') => {
                                                if typed.len() >= 3 {
                                                    typed.clear();
                                                }
                                                typed.push(*c);
                                                if let Ok(n) = typed.parse::<usize>() {
                                                    if n < rows.len() {
                                                        *index = n;
                                                    }
                                                }
                                            }
                                            copy::Key::Sequence(seq) => {
                                                typed.clear();
                                                match seq.as_str() {
                                                    "\x1b[A" | "\x1bOA" => {
                                                        *index = index.saturating_sub(1)
                                                    }
                                                    "\x1b[B" | "\x1bOB" => {
                                                        *index = (*index + 1).min(rows.len() - 1)
                                                    }
                                                    "\x1b[5~" => {
                                                        *index =
                                                            index.saturating_sub(size.0 as usize)
                                                    }
                                                    "\x1b[6~" => {
                                                        *index = (*index + size.0 as usize)
                                                            .min(rows.len() - 1)
                                                    }
                                                    _ => (),
                                                }
                                            }
                                            _ => (),
                                        }
                                    }
                                }
                                Mode::Prompt(prompt) => {
                                    if key.escape() || matches!(key, copy::Key::Char('\x03')) {
                                        mode = Mode::Live;
                                        status = "Command cancelled".into();
                                        refresh = true;
                                    } else {
                                        match &key {
                                            copy::Key::Char('\r' | '\n') => {
                                                let text = prompt.clone();
                                                let mut next = Mode::Live;
                                                command(
                                                    &mut s, session, &snapshot, &text, &mut next,
                                                )?;
                                                mode = next;
                                                status.clear();
                                                refresh = true;
                                            }
                                            copy::Key::Char('\x7f' | '\x08') => {
                                                copy::backspace(prompt);
                                                status.clear();
                                            }
                                            copy::Key::Char(c) if !c.is_control() => {
                                                if prompt.len() + c.len_utf8() <= 4096 {
                                                    prompt.push(*c);
                                                }
                                                status.clear();
                                            }
                                            _ => (),
                                        }
                                    }
                                }
                                Mode::Help(offset) => {
                                    if key.escape()
                                        || matches!(key, copy::Key::Char('\r' | '\n' | 'q' | '?'))
                                    {
                                        mode = Mode::Live;
                                        status.clear();
                                        refresh = true;
                                    } else {
                                        match &key {
                                            copy::Key::Char('j') => *offset += 1,
                                            copy::Key::Char('k') => {
                                                *offset = offset.saturating_sub(1)
                                            }
                                            copy::Key::Sequence(seq) => match seq.as_str() {
                                                "\x1b[A" => *offset = offset.saturating_sub(1),
                                                "\x1b[B" => *offset += 1,
                                                "\x1b[5~" => {
                                                    *offset = offset.saturating_sub(size.0 as usize)
                                                }
                                                "\x1b[6~" => *offset += size.0 as usize,
                                                _ => (),
                                            },
                                            _ => (),
                                        }
                                    }
                                }
                                Mode::Live => {
                                    // Alt+0…9 selects window 0…9, as labelled in the status bar.
                                    if let Some(index) = alt_digit(&key).filter(|_| !prefix) {
                                        plain(&mut s, &snapshot, &mut pending)?;
                                        let rows = call(
                                            &mut s,
                                            "window.list",
                                            json!({"session":session}),
                                        )?;
                                        match rows.as_array().and_then(|a| a.get(index)) {
                                            Some(row) => {
                                                call(
                                                    &mut s,
                                                    "window.select",
                                                    json!({"window":row["id"]}),
                                                )?;
                                                snapshot =
                                                    call(&mut s, "client.render", json!({}))?;
                                                status.clear();
                                            }
                                            None => status = format!("No window {index}"),
                                        }
                                        refresh = true;
                                        return Ok(());
                                    }
                                    if prefix {
                                        prefix = false;
                                        plain(&mut s, &snapshot, &mut pending)?;
                                        status.clear();
                                        if matches!(&key,copy::Key::Char(c)if *c as u32==config.prefix as u32)
                                        {
                                            if config.double_prefix_previous {
                                                call(
                                                    &mut s,
                                                    "window.previous",
                                                    json!({"session":session}),
                                                )?;
                                            } else {
                                                call(
                                                    &mut s,
                                                    "client.input",
                                                    json!({"pane":snapshot["active_pane"],"kind":"keyboard","text":(config.prefix as char).to_string()}),
                                                )?;
                                            }
                                        } else {
                                            let key = if key.escape() {
                                                copy::Key::Char('\x1b')
                                            } else {
                                                key
                                            };
                                            let copy::Key::Char(mut binding) = key else {
                                                return Err(io::Error::other(
                                                    "Unsupported prefix key; ? opens help",
                                                ));
                                            };
                                            binding = ruwac_protocol::screen_key(binding);
                                            if binding != '\t'
                                                && (1..=26).contains(&(binding as u32))
                                            {
                                                binding = (binding as u8 + b'a' - 1) as char;
                                            }
                                            binding = ruwac_protocol::screen_key(
                                                *config.remaps.get(&binding).unwrap_or(&binding),
                                            );
                                            match binding {
                                                'a' => {
                                                    call(
                                                        &mut s,
                                                        "client.input",
                                                        json!({"pane":snapshot["active_pane"],"kind":"keyboard","text":(config.prefix as char).to_string()}),
                                                    )?;
                                                }
                                                'k' => command(
                                                    &mut s, session, &snapshot, "kill", &mut mode,
                                                )?,
                                                'A' => mode = Mode::Prompt("rename ".into()),
                                                'c' => {
                                                    call(
                                                        &mut s,
                                                        "window.create",
                                                        json!({"session":session}),
                                                    )?;
                                                }
                                                'o' => {
                                                    call(
                                                        &mut s,
                                                        "window.previous",
                                                        json!({"session":session}),
                                                    )?;
                                                }
                                                'n' | 'p' => {
                                                    call(
                                                        &mut s,
                                                        "window.cycle",
                                                        json!({"session":session,"previous":binding=='p'}),
                                                    )?;
                                                }
                                                'd' => {
                                                    call(&mut s, "session.detach", json!({}))?;
                                                    STOP.store(true, Ordering::Relaxed);
                                                }
                                                'S' | 's' | '|' => {
                                                    call(
                                                        &mut s,
                                                        "pane.create",
                                                        json!({"window":snapshot["window"],"orientation":if binding=='|'{"vertical"}else{"horizontal"}}),
                                                    )?;
                                                }
                                                '[' => {
                                                    mode = Mode::Copy(history(
                                                        &mut s,
                                                        snapshot["active_pane"]
                                                            .as_str()
                                                            .ok_or_else(|| {
                                                                io::Error::other(
                                                                    "Active pane missing",
                                                                )
                                                            })?,
                                                    )?)
                                                }
                                                ']' => {
                                                    call(
                                                        &mut s,
                                                        "buffer.paste",
                                                        json!({"pane":snapshot["active_pane"]}),
                                                    )?;
                                                }
                                                'z' => {
                                                    call(
                                                        &mut s,
                                                        "pane.layout",
                                                        json!({"window":snapshot["window"],"zoom":snapshot["zoom"]!=true}),
                                                    )?;
                                                }
                                                '"' => {
                                                    mode = windows(
                                                        &mut s,
                                                        session,
                                                        &snapshot["window"],
                                                    )?
                                                }
                                                ':' => mode = Mode::Prompt(String::new()),
                                                '?' => mode = Mode::Help(0),
                                                '\t' => {
                                                    let panes = snapshot["panes"]
                                                        .as_array()
                                                        .filter(|a| !a.is_empty())
                                                        .ok_or_else(|| {
                                                            io::Error::other("Pane list empty")
                                                        })?;
                                                    let index = panes
                                                        .iter()
                                                        .position(|p| {
                                                            p["id"] == snapshot["active_pane"]
                                                        })
                                                        .unwrap_or(0);
                                                    call(
                                                        &mut s,
                                                        "pane.select",
                                                        json!({"pane":panes[(index+1)%panes.len()]["id"]}),
                                                    )?;
                                                }
                                                '0'..='9' => {
                                                    let windows = call(
                                                        &mut s,
                                                        "window.list",
                                                        json!({"session":session}),
                                                    )?;
                                                    let window = windows
                                                        .as_array()
                                                        .and_then(|a| {
                                                            a.get(binding as usize - '0' as usize)
                                                        })
                                                        .ok_or_else(|| {
                                                            io::Error::other(
                                                                "Window index unavailable",
                                                            )
                                                        })?;
                                                    call(
                                                        &mut s,
                                                        "window.select",
                                                        json!({"window":window["id"]}),
                                                    )?;
                                                }
                                                _ => {
                                                    return Err(io::Error::other(
                                                        "Unsupported prefix key; ? opens help",
                                                    ));
                                                }
                                            }
                                        }
                                        refresh = true;
                                    } else if matches!(&key,copy::Key::Char(c)if *c as u32==config.prefix as u32)
                                    {
                                        plain(&mut s, &snapshot, &mut pending)?;
                                        prefix = true;
                                    } else {
                                        pending.push_str(&key.bytes());
                                    }
                                }
                            }
                            Ok(())
                        })();
                        if let Err(e) = result {
                            status = e.to_string();
                            if let Mode::Copy(c) = &mut mode {
                                c.message = status.clone();
                            }
                        }
                        if STOP.load(Ordering::Relaxed) {
                            break;
                        }
                        if refresh {
                            snapshot = call(&mut s, "client.render", json!({}))?;
                        }
                    }
                    if !pending.is_empty() {
                        if let Err(e) = plain(&mut s, &snapshot, &mut pending) {
                            status = e.to_string();
                        }
                    }
                }
            }
            if STOP.load(Ordering::Relaxed) {
                break;
            }
        }
        if STOP.load(Ordering::Relaxed) {
            break;
        }
        let new_screen_size = terminal_dimensions();
        if new_screen_size != screen_size {
            screen_size = new_screen_size;
            CLEAR.store(true, Ordering::Relaxed);
            dirty = true;
        }
        let newsize = dimensions(limits);
        if newsize != size {
            size = newsize;
            call(
                &mut s,
                "client.resize",
                json!({"rows":size.0,"cols":size.1}),
            )?;
            dirty = true;
        }
        let render_interval = if input_at.elapsed() < Duration::from_millis(150) {
            8
        } else {
            25
        };
        if rendered.elapsed() > Duration::from_millis(render_interval) {
            if matches!(mode, Mode::Live) {
                // Navigation can already have replaced the frame with an idle
                // live shell. Observe it before a revision-only poll skips it.
                observe_running_window(&snapshot, &mut running_windows);
                let fresh = match call(
                    &mut s,
                    "client.render",
                    json!({"revision":snapshot["revision"]}),
                ) {
                    Ok(frame) => frame,
                    Err(error) => {
                        // Another attached client may have retired the final
                        // ended pane. Confirm session removal before treating a
                        // missing attachment as a clean exit.
                        let removed = serde_json::from_str::<Value>(&error.to_string())
                            .is_ok_and(|e| e["code"] == "NOT_ATTACHED" || e["code"] == "STALE_ID")
                            && call(&mut s, "session.list", json!({}))?
                                .as_array()
                                .is_some_and(|rows| !rows.iter().any(|row| row["id"] == session));
                        if removed {
                            break;
                        }
                        return Err(error);
                    }
                };
                // The revision counts changes in every session of the daemon;
                // redraw only when this view's content changed, or a client
                // nested in a RUWAC terminal redraws itself in a loop.
                if accept_render_frame(&mut snapshot, fresh) {
                    let ended = snapshot["panes"]
                        .as_array()
                        .is_some_and(|p| !p.is_empty() && p.iter().all(|p| p["ended"] == true));
                    let completed = ended
                        && snapshot["window"]
                            .as_str()
                            .is_some_and(|id| running_windows.remove(id));
                    if completed {
                        let pinned = window
                            .is_some_and(|window| snapshot["window"].as_str() == Some(window));
                        let panes = match call(&mut s, "pane.list", json!({"session":session})) {
                            Ok(panes) => panes,
                            Err(error) => {
                                // A second view can remove the same completed
                                // session between rendering and reading panes.
                                let removed = serde_json::from_str::<Value>(&error.to_string())
                                    .is_ok_and(|e| e["code"] == "STALE_ID")
                                    && call(&mut s, "session.list", json!({}))?
                                        .as_array()
                                        .is_some_and(|rows| {
                                            !rows.iter().any(|row| row["id"] == session)
                                        });
                                if removed {
                                    break;
                                }
                                return Err(error);
                            }
                        };
                        let live = panes
                            .as_array()
                            .and_then(|p| p.iter().find(|p| p["ended"] != true))
                            .cloned();
                        if !pinned {
                            if let Some(live) = &live {
                                call(&mut s, "window.select", json!({"window":live["window"]}))?;
                                call(&mut s, "pane.select", json!({"pane":live["id"]}))?;
                            }
                        }
                        // Normal shell exit removes this completed window, even
                        // when it is the last one. History that was already ended
                        // when attached remains explicit history until -wipe.
                        for pane in snapshot["panes"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter(|p| p["ended"] == true && p["restored"] != true)
                            .filter_map(|p| p["id"].as_str())
                        {
                            if let Err(error) =
                                call(&mut s, "pane.close", json!({"pane":pane,"if_ended":true}))
                            {
                                let stale = serde_json::from_str::<Value>(&error.to_string())
                                    .is_ok_and(|e| e["code"] == "STALE_ID");
                                if !stale {
                                    return Err(error);
                                }
                            }
                        }
                        if pinned || live.is_none() {
                            break;
                        }
                        snapshot = call(&mut s, "client.render", json!({}))?;
                    }
                    observe_running_window(&snapshot, &mut running_windows);
                    dirty = true;
                }
            }
            rendered = Instant::now();
        }
        if matches!(mode, Mode::Live) && commands_refreshed.elapsed() >= Duration::from_millis(500)
        {
            dirty |= status_commands::refresh(&mut snapshot, |m, p| call(&mut s, m, p));
            commands_refreshed = Instant::now();
        }
        if dirty {
            draw_mode(&mut mode, &snapshot, size, &status, &config)?;
        }
    }
    let _ = call(&mut s, "session.detach", json!({}));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn desktop_selection_answers_decode_and_base64_round_trips() {
        for text in ["", "a", "ab", "abc", "Že界\n tail"] {
            let encoded = base64_encode(text.as_bytes());
            assert_eq!(base64_decode(encoded.as_bytes()).unwrap(), text.as_bytes());
        }
        assert_eq!(base64_encode(b"hi"), "aGk=");
        let mut d = Decoder::default();
        assert_eq!(
            d.feed(b"\x1b]52;p;aGk=\x07x"),
            vec![
                Input::Selection("hi".into()),
                Input::Keyboard(b"x".to_vec())
            ]
        );
        // Large answers arrive in pieces and exceed the ordinary OSC bound.
        let text = "y".repeat(100_000);
        let answer = format!("\x1b]52;p;{}\x1b\\", base64_encode(text.as_bytes()));
        let (head, tail) = answer.as_bytes().split_at(30_000);
        assert!(d.feed(head).is_empty());
        assert_eq!(d.feed(tail), vec![Input::Selection(text)]);
        assert!(d.feed(b"\x1b]52;p;!!\x07").is_empty());
    }
    #[test]
    fn unrelated_revision_is_acknowledged_without_repeated_full_frames() {
        let mut snapshot = json!({"revision":1,"panes":[{"id":"p","grid":{"cells":[["x"]]}}]});
        let mut server = snapshot.clone();
        server["revision"] = json!(2);
        snapshot["_commands"] = json!({"window_generated":"sleep"});
        let mut full_frames = 0;
        let mut redraws = 0;
        for _ in 0..120 {
            let fresh = if snapshot["revision"] == server["revision"] {
                json!({"revision":server["revision"],"unchanged":true})
            } else {
                full_frames += 1;
                server.clone()
            };
            redraws += usize::from(accept_render_frame(&mut snapshot, fresh));
        }
        assert_eq!(
            full_frames, 1,
            "unrelated output must not force full snapshots forever"
        );
        assert_eq!(redraws, 0, "an identical visible screen must not repaint");
        assert_eq!(snapshot["revision"], 2);
        assert_eq!(snapshot["_commands"]["window_generated"], "sleep");
        assert_eq!(snapshot["panes"][0]["grid"]["cells"][0][0], "x");

        server["revision"] = json!(3);
        server["panes"][0]["grid"]["cells"][0][0] = json!("y");
        assert!(accept_render_frame(&mut snapshot, server));
        assert_eq!(snapshot["panes"][0]["grid"]["cells"][0][0], "y");
    }
    #[test]
    fn alt_digit_selects_windows_by_status_bar_number() {
        assert_eq!(alt_digit(&copy::Key::Sequence("\x1b1".into())), Some(1));
        assert_eq!(alt_digit(&copy::Key::Sequence("\x1b0".into())), Some(0));
        assert_eq!(alt_digit(&copy::Key::Sequence("\x1ba".into())), None);
        assert_eq!(alt_digit(&copy::Key::Sequence("\x1b[A".into())), None);
        assert_eq!(alt_digit(&copy::Key::Char('1')), None);
    }
    #[test]
    fn frames_differing_only_in_revision_are_the_same() {
        let a = json!({"revision":1,"panes":[{"id":"p","grid":{"cells":[["x"]]}}]});
        let mut b = a.clone();
        b["revision"] = json!(9);
        assert!(same_frame(&a, &b));
        b["panes"][0]["grid"]["cells"][0][0] = json!("y");
        assert!(!same_frame(&a, &b));
        assert!(!same_frame(&a, &json!({"unchanged":true})));
    }
    #[test]
    fn mouse_reports_follow_the_active_program() {
        let frame = |active: &str| {
            json!({"active_pane":active,"panes":[
                {"id":"pane_shell","grid":{"mouse":false}},
                {"id":"pane_htop","grid":{"mouse":true}}
            ]})
        };
        assert!(
            mouse_wanted(&frame("pane_shell")),
            "client handles wheel and drag for shells"
        );
        assert!(mouse_wanted(&frame("pane_htop")));
        assert!(!mouse_wanted(&json!({})));
    }
    #[test]
    fn mouse_reports_route_to_the_pane_under_the_pointer() {
        let frame = json!({"active_pane":"pane_top","panes":[
            {"id":"pane_top","rect":{"x":0,"y":0,"cols":80,"rows":10},"grid":{"mouse":false}},
            {"id":"pane_btm","rect":{"x":0,"y":11,"cols":80,"rows":12},
             "grid":{"mouse":true,"mouse_motion":true}}
        ]});
        // Screen row 13 is row 2 of the lower pane.
        let (pane, event, wants) = route_mouse(&frame, b"\x1b[<0;5;13M").unwrap();
        assert_eq!(pane, "pane_btm");
        assert_eq!(event, json!({"button":0,"x":5,"y":2,"release":false}));
        assert!(wants);
        let (pane, event, wants) = route_mouse(&frame, b"\x1b[<64;1;1M").unwrap();
        assert_eq!((pane.as_str(), wants), ("pane_top", false));
        assert_eq!(event["button"], 64);
        assert!(
            route_mouse(&frame, b"\x1b[<0;5;11M").is_none(),
            "divider row"
        );
        let mut active = frame.clone();
        active["active_pane"] = json!("pane_btm");
        assert_eq!(mouse_request(&active), Some(1003));
        assert_eq!(
            mouse_request(&frame),
            Some(1002),
            "shells: client scrollback and selection"
        );
    }
    #[test]
    fn fragmented_input_and_replies() {
        let mut d = Decoder::default();
        assert_eq!(
            d.feed(b"EARLY\x1b[?"),
            vec![Input::Keyboard(b"EARLY".to_vec())]
        );
        assert_eq!(d.feed(b"1;2cLATE"), vec![Input::Keyboard(b"LATE".to_vec())]);
        assert!(d.feed(&[0xc5]).is_empty());
        assert_eq!(
            d.feed(&[0xbe]),
            vec![Input::Keyboard("ž".as_bytes().to_vec())]
        );
        assert!(d.feed(b"\x1b[<0;").is_empty());
        assert_eq!(
            d.feed(b"1;2M"),
            vec![Input::Mouse(b"\x1b[<0;1;2M".to_vec())]
        );
    }
    #[test]
    fn oversized_paste_drained() {
        let mut d = Decoder::default();
        let mut b = b"\x1b[200~".to_vec();
        b.extend(vec![b'x'; MAX_TEXT + 6]);
        assert_eq!(d.feed(&b), vec![Input::Rejected("paste exceeds 256 KiB")]);
        assert!(d.feed(b"\x01d").is_empty());
        assert_eq!(
            d.feed(b"\x1b[201~OK"),
            vec![Input::Keyboard(b"OK".to_vec())]
        );
    }
    #[test]
    fn paste_not_prefix() {
        let mut d = Decoder::default();
        assert!(d.feed(b"\x1b[200~a\x01d").is_empty());
        assert_eq!(d.feed(b"\x1b[201~"), vec![Input::Paste(b"a\x01d".to_vec())]);
        assert_eq!(
            d.feed(b"\x1b]11;rgb:ffff/0000/0000\x07x"),
            vec![Input::Keyboard(b"x".to_vec())]
        );
    }
}

#[cfg(test)]
mod display_identity_tests {
    use super::*;
    #[test]
    fn split_lines_use_only_unoccupied_reserved_cells() {
        let mut frame = json!({"panes":[{"rect":{"x":0,"y":0,"cols":39,"rows":23}},{"rect":{"x":40,"y":0,"cols":40,"rows":23}}]});
        let lines = divider_cells(&frame);
        assert_eq!(lines.len(), 23);
        assert!(
            lines
                .iter()
                .all(|(&(row, col), &direction)| row < 23 && col == 39 && direction == 1)
        );
        // An older engine has no spare cell; never overwrite application text.
        frame["panes"][0]["rect"]["cols"] = json!(40);
        assert!(divider_cells(&frame).is_empty());
        frame = json!({"panes":[{"rect":{"x":0,"y":0,"cols":80,"rows":11}},{"rect":{"x":0,"y":12,"cols":80,"rows":11}}]});
        assert_eq!(divider_cells(&frame).len(), 80);
    }
    #[test]
    fn long_unicode_window_labels_have_exact_visible_click_ranges() {
        let rows:Vec<_>=(0..18).map(|n|json!({"index":n,"id":format!("window-{n}"),"name":"界é👩‍💻-long-window-label".repeat(8)})).collect();
        for active in [0, 7, 17] {
            let v = json!({"session_name":"long session title".repeat(8),"name":"active title".repeat(8),"window_index":active,"window":format!("window-{active}"),"windows":rows,"rows":40,"cols":120});
            for columns in [20, 37, 80, 240] {
                let layout = status_layout(
                    &v,
                    (40, columns),
                    "prefix · an arbitrarily long diagnostic "
                        .repeat(12)
                        .as_str(),
                );
                assert!(copy::width(&layout.text) <= columns as usize);
                assert!(
                    layout
                        .hits
                        .iter()
                        .any(|(_, _, id)| id == &format!("window-{active}")),
                    "active window must stay visible: {}",
                    layout.text
                );
                for (left, right, id) in &layout.hits {
                    assert!(left < right && *right <= columns as usize);
                    for col in *left..*right {
                        assert_eq!(
                            status_window_at(
                                &v,
                                (40, columns),
                                "prefix · an arbitrarily long diagnostic "
                                    .repeat(12)
                                    .as_str(),
                                col
                            )
                            .as_deref(),
                            Some(id.as_str())
                        );
                    }
                }
                assert!(status_window_at(&v, (40, columns), "", columns as usize).is_none());
            }
        }
    }
    #[test]
    fn colors_preserve_native_ansi_bright_indexed_and_rgb_semantics() {
        assert_eq!(sgr(&json!(1), true), "31");
        assert_eq!(sgr(&json!(9), true), "91");
        assert_eq!(sgr(&json!(1), false), "41");
        assert_eq!(sgr(&json!(9), false), "101");
        assert_eq!(sgr(&json!(123), true), "38;5;123");
        assert_eq!(sgr(&json!([1, 2, 3]), true), "38;2;1;2;3");
        assert_eq!(sgr(&json!(256), true), "39");
        assert_eq!(sgr(&json!(257), false), "49");
        assert_eq!(sgr(&json!(260), true), "38;5;1");
    }
    #[test]
    fn selected_window_identity_and_shared_size_remain_visible_and_safe() {
        let v = json!({"session_name":"d\u{1b}\u{7}","window_index":1,"window":"b","name":"editor","rows":23,"cols":80,"windows":[{"id":"a","name":"shell","index":0},{"id":"b","name":"editor","index":1}]});
        let line = status_line(&v, (30, 160), "");
        assert!(line.starts_with("RUWAC d · 1:editor"));
        assert!(line.contains("1:editor*"));
        assert!(line.contains("shared PTY 80×23"));
        assert!(!window_identity(&v).contains('\u{1b}'));
        assert!(!window_identity(&v).contains('\u{7}'));
        assert!(
            status_line(&v, (23, 80), "Window choice cancelled")
                .contains("Window choice cancelled")
        );
        let mut renamed = v.clone();
        renamed["name"] = json!("Unicode ž ; $HOME");
        assert!(
            status_line(&renamed, (23, 80), "^A · Window choice cancelled")
                .contains("Window choice cancelled")
        );
    }
}

#[cfg(test)]
mod modified_navigation_tests {
    use super::*;

    #[test]
    fn fragmented_shift_left_survives_tui_decode_and_key_split() {
        let sequence = b"\x1b[1;2D";
        for split in 1..sequence.len() {
            let mut decoder = Decoder::default();
            assert!(decoder.feed(&sequence[..split]).is_empty());
            let events = decoder.feed(&sequence[split..]);
            assert_eq!(events, vec![Input::Keyboard(sequence.to_vec())]);
            let keys = copy::keys(std::str::from_utf8(sequence).unwrap());
            assert_eq!(keys.len(), 1);
            assert_eq!(keys[0].bytes().as_bytes(), sequence);
        }
    }
}

#[cfg(test)]
mod legacy_history_tests {
    use super::*;
    #[test]
    fn legacy_single_row_keeps_unicode_wrap_and_rejects_controls() {
        let row = history_rows(&json!({"text":"界e\u{301} ","wrapped":[true]}), true).unwrap();
        assert_eq!(row[0].text, "界e\u{301} ");
        assert!(row[0].wrapped);
        assert!(history_rows(&json!({"text":"a\nb\n","wrapped":[false]}), true).is_err());
        assert!(history_rows(&json!({"text":"a","wrapped":[false]}), true).is_err());
        assert!(history_rows(&json!({"text":"a\n","wrapped":[false,false]}), true).is_err());
    }
    fn legacy_copy(changed: bool, denied: bool) -> io::Result<copy::Copy> {
        let (mut client, mut server) = UnixStream::pair().unwrap();
        let worker = std::thread::spawn(move || {
            let mut zeros = 0;
            while let Ok(q) = ruwac_protocol::read_frame(&mut server) {
                let result = match q["method"].as_str().unwrap() {
                    "pane.history.freeze" => {
                        write_frame(&mut server,&json!({"version":0,"id":q["id"],"error":{"code":if denied {"DENIED"}else{"METHOD_NOT_FOUND"},"message":"test"}})).unwrap();
                        continue;
                    }
                    "client.render" => json!({"session":"session_test","session_name":"test"}),
                    "pane.list" => json!([{"id":"pane_test","cols":80}]),
                    "pane.history" => {
                        assert_eq!(q["params"]["lines"], 1);
                        let offset = q["params"]["offset"].as_u64().unwrap();
                        if offset == 0 {
                            zeros += 1;
                        }
                        json!({"epoch":if changed && zeros==3 {2}else{1},"total_lines":3,"next_offset":offset+1,
                            "text":match offset {0=>"first\n",1=>"界e\u{301}",_=>"last\n"},"wrapped":[offset==1]})
                    }
                    other => panic!("Unexpected legacy call: {other}"),
                };
                write_frame(
                    &mut server,
                    &json!({"version":0,"id":q["id"],"result":result}),
                )
                .unwrap();
            }
        });
        let result = history(&mut client, "pane_test");
        drop(client);
        worker.join().unwrap();
        result
    }
    #[test]
    fn old_daemon_copy_works_without_freeze_or_canonical_rows() {
        let copied = legacy_copy(false, false).unwrap();
        assert_eq!(copied.lines.len(), 3);
        assert_eq!(copied.lines[1].text, "界e\u{301}");
        assert!(copied.lines[1].wrapped);
        assert!(copied.message.contains("Legacy daemon"));
    }
    #[test]
    fn legacy_copy_rejects_output_race_and_never_falls_back_after_denial() {
        assert!(legacy_copy(true, false).is_err());
        assert!(
            legacy_copy(false, true)
                .err()
                .unwrap()
                .to_string()
                .contains("DENIED")
        );
    }
}
