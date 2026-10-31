/// GNU Screen names for operations supported by both RUWAC clients.
pub fn screen_command(name: &str) -> &str {
    match name {
        "screen" => "new",
        "title" => "rename",
        "previous" => "prev",
        "prev" => "prev",
        "windowlist" => "windows",
        "focus" => "focus",
        _ => name,
    }
}
/// Screen's alternative default keys; double prefix remains a nested literal.
pub fn screen_key(key: char) -> char {
    match key {
        ' ' => 'n',
        '\x08' | '\x7f' => 'p',
        'w' => '"',
        '\x1b' => '[',
        _ => key,
    }
}
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::io::{self, Read, Write};
use uuid::Uuid;
pub const MAX_FRAME: usize = 1024 * 1024;
pub const MAX_TEXT: usize = 256 * 1024;
/// Requests and ordinary replies keep their original 1 MiB bound. Only an
/// explicitly opted-in terminal render response may use this larger ceiling.
pub const MAX_RENDER_FRAME: usize = 64 * 1024 * 1024;
#[derive(Clone, Copy, Debug, Serialize)]
pub struct TerminalLimits {
    pub rows: u16,
    pub cols: u16,
    pub cells: usize,
}
impl TerminalLimits {
    pub const LEGACY: Self = Self {
        rows: 100,
        cols: 200,
        cells: 20000,
    };
    pub const CURRENT: Self = Self {
        rows: 512,
        cols: 1024,
        cells: 262144,
    };
    pub fn valid(self, rows: u16, cols: u16) -> bool {
        rows > 0
            && cols > 0
            && rows <= self.rows
            && cols <= self.cols
            && rows as usize * cols as usize <= self.cells
    }
    pub fn clamp(self, rows: u16, cols: u16) -> (u16, u16) {
        let cols = cols.clamp(10, (self.cols as usize).min(self.cells / 4) as u16);
        let rows = rows.clamp(
            4,
            (self.rows as usize).min(self.cells / cols as usize) as u16,
        );
        (rows, cols)
    }
    pub fn advertised(capabilities: &Value) -> Self {
        let value = &capabilities["terminal_size_limits"];
        let Some(rows) = value["rows"].as_u64().and_then(|n| u16::try_from(n).ok()) else {
            return Self::LEGACY;
        };
        let Some(cols) = value["cols"].as_u64().and_then(|n| u16::try_from(n).ok()) else {
            return Self::LEGACY;
        };
        let Some(cells) = value["cells"]
            .as_u64()
            .and_then(|n| usize::try_from(n).ok())
        else {
            return Self::LEGACY;
        };
        if !(4..=Self::CURRENT.rows).contains(&rows)
            || !(10..=Self::CURRENT.cols).contains(&cols)
            || !(40..=Self::CURRENT.cells).contains(&cells)
            || cells < cols as usize * 4
        {
            return Self::LEGACY;
        }
        Self { rows, cols, cells }
    }
}
pub fn id(kind: &str) -> String {
    format!("{kind}_{}", Uuid::new_v4().simple())
}
pub fn valid_id(s: &str, kind: &str) -> bool {
    s.strip_prefix(&format!("{kind}_"))
        .is_some_and(|x| x.len() == 32 && x.bytes().all(|b| b.is_ascii_hexdigit()))
}
/// Live clients close completed split PTYs when a live sibling remains. Use the
/// established pane.close operation so old daemons and all attached clients
/// share the same layout/focus repair. Final-session and restored history stay.
/// Closing a split has the same history semantics as an explicit pane close.
pub fn client_call(
    method: &str,
    mut params: Value,
    mut call: impl FnMut(&str, Value) -> io::Result<Value>,
) -> io::Result<Value> {
    if method == "client.render" {
        if !params.is_object() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "render params must be an object",
            ));
        }
        params["max_response_bytes"] = json!(MAX_RENDER_FRAME);
    }
    let mut frame = call(method, params)?;
    if method != "client.render" || frame["unchanged"] == true || frame["size_role"] == "viewer" {
        return Ok(frame);
    }
    let Some(panes) = frame["panes"].as_array() else {
        return Ok(frame);
    };
    if !panes
        .iter()
        .any(|p| p["ended"] == false && p["restored"] != true)
    {
        return Ok(frame);
    }
    let ended: Vec<String> = panes
        .iter()
        .filter(|p| p["ended"] == true && p["restored"] != true)
        .filter_map(|p| p["id"].as_str())
        .filter(|id| valid_id(id, "pane"))
        .take(128)
        .map(str::to_owned)
        .collect();
    for id in &ended {
        if let Err(error) = call("pane.close", json!({"pane":id})) {
            // Another attached client may already have closed this exact ID.
            // Transport, authentication and unrelated errors must propagate.
            let stale = serde_json::from_str::<Value>(&error.to_string())
                .is_ok_and(|e| e["code"] == "STALE_ID");
            if !stale {
                return Err(error);
            }
        }
    }
    if !ended.is_empty() {
        frame = call(
            "client.render",
            json!({"max_response_bytes":MAX_RENDER_FRAME}),
        )?;
    }
    Ok(frame)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub version: u8,
    pub id: Value,
    pub method: String,
    #[serde(default)]
    pub params: Value,
    #[serde(default)]
    pub idempotency_key: Option<String>,
}
pub fn request(method: &str, params: Value) -> Request {
    Request {
        version: 0,
        id: json!(id("request")),
        method: method.into(),
        params,
        idempotency_key: None,
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Error {
    pub code: String,
    pub message: String,
}
impl Error {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
}
pub type Result<T> = std::result::Result<T, Error>;
pub fn response(id: &Value, result: Result<Value>) -> Value {
    let mut envelope = json!({"version":0,"id":id});
    match result {
        Ok(value) => envelope["result"] = value,
        Err(error) => envelope["error"] = json!({"code":error.code,"message":error.message}),
    }
    envelope
}
pub fn write_frame(w: &mut impl Write, v: &impl Serialize) -> io::Result<()> {
    let b = encode_frame(v, MAX_FRAME)?;
    w.write_all(&(b.len() as u32).to_be_bytes())?;
    w.write_all(&b)?;
    w.flush()
}
pub fn read_frame(r: &mut impl Read) -> io::Result<Value> {
    read_frame_with_limit(r, MAX_FRAME)
}
pub fn read_frame_with_limit(r: &mut impl Read, limit: usize) -> io::Result<Value> {
    if !(1..=MAX_RENDER_FRAME).contains(&limit) {
        return Err(io::Error::other("invalid frame limit"));
    }
    let mut h = [0; 4];
    r.read_exact(&mut h)?;
    let n = u32::from_be_bytes(h) as usize;
    if n == 0 || n > limit {
        return Err(io::Error::other("invalid frame size"));
    }
    let mut b = vec![0; n];
    r.read_exact(&mut b)?;
    Ok(serde_json::from_slice(&b)?)
}
/// Stop serialization at the negotiated bound, before allocating an unbounded
/// response or writing a partial frame onto the authenticated connection.
pub fn encode_frame(v: &impl Serialize, limit: usize) -> io::Result<Vec<u8>> {
    if !(1..=MAX_RENDER_FRAME).contains(&limit) {
        return Err(io::Error::other("invalid frame limit"));
    }
    struct Bounded {
        bytes: Vec<u8>,
        limit: usize,
    }
    impl Write for Bounded {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if bytes.len() > self.limit.saturating_sub(self.bytes.len()) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "oversized frame",
                ));
            }
            self.bytes.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let mut writer = Bounded {
        bytes: Vec::new(),
        limit,
    };
    serde_json::to_writer(&mut writer, v).map_err(io::Error::other)?;
    Ok(writer.bytes)
}
pub async fn read_async(r: &mut (impl tokio::io::AsyncRead + Unpin)) -> io::Result<Vec<u8>> {
    use tokio::io::AsyncReadExt;
    let n = r.read_u32().await? as usize;
    if n == 0 || n > MAX_FRAME {
        return Err(io::Error::other("invalid frame size"));
    }
    let mut b = vec![0; n];
    r.read_exact(&mut b).await?;
    Ok(b)
}
pub async fn write_async(
    w: &mut (impl tokio::io::AsyncWrite + Unpin),
    v: &Value,
) -> io::Result<()> {
    use tokio::io::AsyncWriteExt;
    let b = encode_frame(v, MAX_FRAME)?;
    w.write_u32(b.len() as u32).await?;
    w.write_all(&b).await?;
    w.flush().await
}
pub fn text(v: &Value, k: &str) -> Result<String> {
    v.get(k)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| Error::new("INVALID_PARAMS", format!("{k} must be a string")))
}
pub fn checked_text(s: &str) -> Result<()> {
    if s.contains('\0') {
        return Err(Error::new("INVALID_TEXT", "NUL is not accepted"));
    }
    if s.len() > MAX_TEXT {
        return Err(Error::new("LIMIT", "text exceeds 256 KiB"));
    }
    Ok(())
}
pub fn target(v: &Value, k: &str, kind: &str) -> Result<String> {
    let s = text(v, k)?;
    if !valid_id(&s, kind) {
        return Err(Error::new(
            "INVALID_ID",
            format!("expected {kind}_ followed by 32 hex digits"),
        ));
    }
    Ok(s)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn large_replies_require_an_explicit_bounded_reader() {
        let value = json!({"render":"x".repeat(MAX_FRAME+200)});
        let mut ordinary = Vec::new();
        assert!(write_frame(&mut ordinary, &value).is_err());
        assert!(
            ordinary.is_empty(),
            "failed encoding must not write a partial header"
        );
        let encoded = encode_frame(&value, MAX_RENDER_FRAME).unwrap();
        let mut wire = (encoded.len() as u32).to_be_bytes().to_vec();
        wire.extend(encoded);
        assert!(read_frame(&mut wire.as_slice()).is_err());
        assert_eq!(
            read_frame_with_limit(&mut wire.as_slice(), MAX_RENDER_FRAME).unwrap(),
            value
        );
        assert!(
            read_frame_with_limit(
                &mut ((MAX_RENDER_FRAME + 1) as u32).to_be_bytes().as_slice(),
                MAX_RENDER_FRAME
            )
            .is_err()
        );
        assert!(encode_frame(&value, 0).is_err());
    }
    #[test]
    fn advertised_limits_fail_closed_and_bound_area() {
        let limits = TerminalLimits::CURRENT;
        assert!(limits.valid(180, 480));
        assert!(!limits.valid(512, 1024));
        assert_eq!(limits.clamp(512, 1024), (256, 1024));
        assert_eq!(
            TerminalLimits::advertised(&json!({})).clamp(180, 480),
            (100, 200)
        );
        assert_eq!(
            TerminalLimits::advertised(
                &json!({"terminal_size_limits":{"rows":65535,"cols":65535,"cells":1}})
            )
            .clamp(180, 480),
            (100, 200)
        );
        assert_eq!(
            TerminalLimits::advertised(&json!({"terminal_size_limits":limits})).clamp(180, 480),
            (180, 480)
        );
    }
    #[test]
    fn framing_bounds() {
        let mut b = Vec::new();
        write_frame(&mut b, &json!({"a":"ž"})).unwrap();
        assert_eq!(read_frame(&mut b.as_slice()).unwrap()["a"], "ž");
        assert!(read_frame(&mut (MAX_FRAME as u32 + 1).to_be_bytes().as_slice()).is_err());
    }
    #[test]
    fn target_and_text() {
        assert!(valid_id(&id("pane"), "pane"));
        assert!(!valid_id(&id("window"), "pane"));
        assert!(checked_text("\0").is_err());
        assert!(checked_text(&"x".repeat(MAX_TEXT + 1)).is_err());
    }
}

/// Detect the distribution marker adjacent to this executable's installation.
/// Version equality is intentionally unnecessary: an older process survives RPM upgrades.
pub fn managed_rpm_installation(executable: &std::path::Path) -> bool {
    let Some(prefix) = executable.parent().and_then(std::path::Path::parent) else {
        return false;
    };
    let marker = prefix.join("share/ruwac/rpm-installation.json");
    let Ok(metadata) = marker.metadata() else {
        return false;
    };
    if !metadata.is_file() || metadata.len() > 8192 {
        return false;
    }
    std::fs::read(marker)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
        .is_some_and(|value| value["kind"] == "fedora-testing-rpm")
}

#[cfg(test)]
mod split_exit_tests {
    use super::*;
    fn mixed(dead: &str, live: &str) -> Value {
        json!({"panes":[{"id":dead,"ended":true,"restored":false},{"id":live,"ended":false,"restored":false}]})
    }
    #[test]
    fn another_client_can_finish_the_same_close() {
        let dead = id("pane");
        let live = id("pane");
        let mut calls = Vec::new();
        let result = client_call("client.render", json!({}), |method, params| {
            calls.push(method.to_owned());
            match calls.len() {
                1 => Ok(mixed(&dead, &live)),
                2 => {
                    assert_eq!(method, "pane.close");
                    assert_eq!(params["pane"], dead);
                    Err(io::Error::other(
                        json!({"code":"STALE_ID","message":"Closed by another client"}).to_string(),
                    ))
                }
                3 => Ok(json!({"active_pane":live,"panes":[{"id":live,"ended":false}]})),
                _ => panic!("Unbounded repair requests"),
            }
        })
        .unwrap();
        assert_eq!(result["active_pane"], live);
        assert_eq!(calls, ["client.render", "pane.close", "client.render"]);
    }
    #[test]
    fn unrelated_errors_cannot_be_disguised_as_a_stale_close() {
        for code in ["ACCESS_DENIED", "NOT_ATTACHED", "VERSION"] {
            let mut count = 0;
            let result = client_call("client.render", json!({}), |_, _| {
                count += 1;
                match count {
                    1 => Ok(mixed(&id("pane"), &id("pane"))),
                    2 => Err(io::Error::other(
                        json!({"code":code,"message":"STALE_ID appears only in text"}).to_string(),
                    )),
                    _ => panic!("Failure must propagate before another request"),
                }
            })
            .unwrap_err();
            assert_eq!(
                serde_json::from_str::<Value>(&result.to_string()).unwrap()["code"],
                code
            );
            assert_eq!(count, 2);
        }
    }
    #[test]
    fn final_session_and_restored_history_are_not_closed() {
        let dead = id("pane");
        let live = id("pane");
        for frame in [
            json!({"panes":[{"id":dead,"ended":true,"restored":false}]}),
            json!({"panes":[{"id":dead,"ended":true,"restored":true},{"id":live,"ended":false}]}),
        ] {
            let mut count = 0;
            let result = client_call("client.render", json!({}), |method, _| {
                count += 1;
                assert_eq!(method, "client.render");
                assert_eq!(count, 1);
                Ok(frame.clone())
            })
            .unwrap();
            assert_eq!(result, frame);
        }
    }
}
