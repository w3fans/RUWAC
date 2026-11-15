//! Verified PNG stdin transport over the existing authenticated OpenSSH connection.
use crate::image;
use ruwac_protocol::{Request, request};
use serde_json::{Value, json};
use std::{
    fs,
    io::{self, Read, Write},
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
};

struct Temp(PathBuf);
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}
struct Directory(PathBuf);
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn directory(path: &Path) -> io::Result<()> {
    let m = fs::symlink_metadata(path)?;
    if !path.is_absolute()
        || !m.is_dir()
        || m.uid() != unsafe { libc::geteuid() }
        || m.mode() & 0o777 != 0o700
    {
        return Err(io::Error::other(
            "Remote staging must be an absolute owned 0700 directory",
        ));
    }
    Ok(())
}
pub fn send(
    host: &str,
    rpc: &impl Fn(&Request) -> io::Result<Value>,
    exchange: &impl Fn(Vec<u8>) -> io::Result<Value>,
    pane: &str,
    input: &str,
    destination: &str,
) -> io::Result<Value> {
    if !ruwac_protocol::valid_id(pane, "pane")
        || !Path::new(destination).is_absolute()
        || destination.len() > 512
        || destination.chars().any(char::is_control)
    {
        return Err(io::Error::other(
            "Explicit pane and absolute remote staging directory required",
        ));
    }
    let schema = rpc(&request("schema", json!({})))?;
    if !schema["result"]["methods"]
        .as_array()
        .is_some_and(|m| m.iter().any(|m| m == "pane.attachImage"))
    {
        return Err(io::Error::other(
            "Remote daemon does not support image attachment; preserve it and use an explicitly selected current daemon",
        ));
    }
    let job = rpc(&request("job.query", json!({"pane":pane})))?;
    let pid = job["result"]["foreground"]["pid"]
        .as_u64()
        .ok_or_else(|| io::Error::other("Remote foreground unavailable"))?;
    let root = std::env::temp_dir().join(ruwac_protocol::id("ruwac-image-upload"));
    fs::DirBuilder::new().mode(0o700).create(&root)?;
    let owned = Directory(root);
    let staged =
        ruwac_clipboard::stage_png(Path::new(input), &owned.0).map_err(io::Error::other)?;
    let id = ruwac_protocol::id("image-transfer");
    let header = json!({"id":id,"pane":pane,"directory":destination,"foreground_pid":pid,"sha256":staged.sha256,"bytes":staged.bytes,"width":staged.width,"height":staged.height});
    let encoded = serde_json::to_vec(&header)?;
    if encoded.len() > 8192 {
        return Err(io::Error::other("Image metadata exceeds cap"));
    }
    let mut input = (encoded.len() as u32).to_be_bytes().to_vec();
    input.extend(encoded);
    input.extend(fs::read(&staged.path)?);
    let response = exchange(input)?;
    if response["version"] != 0 || response["id"] != id {
        return Err(io::Error::other("Image transfer correlation rejected"));
    }
    let result = &response["result"];
    let path = result["path"]
        .as_str()
        .ok_or_else(|| io::Error::other("Remote image path missing"))?;
    let filename = Path::new(path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");
    if Path::new(path).parent() != Some(Path::new(destination))
        || filename.len() != 36
        || !filename.ends_with(".png")
        || !filename.as_bytes()[..32].iter().all(u8::is_ascii_hexdigit)
        || result["pane"] != pane
        || result["sha256"] != staged.sha256
        || result["bytes"] != staged.bytes
        || result["width"] != staged.width
        || result["height"] != staged.height
        || result["file_mode"] != 384
        || result["directory_mode"] != 448
        || result["submitted"] != false
        || result["consumed"] != false
        || result["queued"] != true
        || !result["draft_attachment_observed"].is_boolean()
        || !matches!(result["adapter"].as_str(), Some("codex" | "claude"))
    {
        return Err(io::Error::other(
            "Remote PNG hash, dimensions, permissions or destination receipt rejected; inspect the remote draft before retrying",
        ));
    }
    let mut response = response;
    response["result"]["remote_host"] = json!(host);
    response["result"]["transfer_verified"] = json!(true);
    Ok(response)
}
pub fn receive(call: &impl Fn(&Request) -> io::Result<Value>) -> io::Result<Value> {
    let mut input = io::stdin().lock();
    let mut length = [0; 4];
    input.read_exact(&mut length)?;
    let length = u32::from_be_bytes(length) as usize;
    if length == 0 || length > 8192 {
        return Err(io::Error::other("Image metadata exceeds cap"));
    }
    let mut bytes = vec![0; length];
    input.read_exact(&mut bytes)?;
    let metadata: Value = serde_json::from_slice(&bytes)?;
    let keys = [
        "id",
        "pane",
        "directory",
        "foreground_pid",
        "sha256",
        "bytes",
        "width",
        "height",
    ];
    if !metadata
        .as_object()
        .is_some_and(|o| o.len() == keys.len() && o.keys().all(|k| keys.contains(&k.as_str())))
    {
        return Err(io::Error::other("Image metadata fields rejected"));
    }
    let pane = metadata["pane"]
        .as_str()
        .filter(|s| ruwac_protocol::valid_id(s, "pane"))
        .ok_or_else(|| io::Error::other("Explicit image pane required"))?;
    let id = metadata["id"]
        .as_str()
        .filter(|s| ruwac_protocol::valid_id(s, "image-transfer"))
        .ok_or_else(|| io::Error::other("Image correlation required"))?;
    let path = metadata["directory"]
        .as_str()
        .filter(|s| s.len() <= 512 && !s.chars().any(char::is_control))
        .ok_or_else(|| io::Error::other("Image directory rejected"))?;
    let pid = metadata["foreground_pid"]
        .as_u64()
        .filter(|p| *p > 0 && *p <= i32::MAX as u64)
        .ok_or_else(|| io::Error::other("Image foreground PID required"))?;
    let count = metadata["bytes"]
        .as_u64()
        .filter(|n| *n > 0 && *n <= ruwac_clipboard::MAX_IMAGE_BYTES as u64)
        .ok_or_else(|| io::Error::other("Image payload exceeds cap"))?;
    let hash = metadata["sha256"]
        .as_str()
        .filter(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()))
        .ok_or_else(|| io::Error::other("Image hash required"))?;
    let dir = Path::new(path);
    directory(dir)?;
    let temp = Temp(dir.join(format!("upload-{}.png", ruwac_protocol::id("image"))));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(&temp.0)?;
    let copied = io::copy(&mut input.take(count + 1), &mut file)?;
    if copied != count {
        return Err(io::Error::other(
            "Image transfer truncated or exceeded declared size",
        ));
    }
    file.flush()?;
    drop(file);
    let staged = ruwac_clipboard::stage_png(&temp.0, dir).map_err(io::Error::other)?;
    if staged.sha256 != hash
        || staged.bytes as u64 != count
        || metadata["width"] != staged.width
        || metadata["height"] != staged.height
    {
        let _ = fs::remove_file(&staged.path);
        return Err(io::Error::other(
            "Transferred PNG hash or dimensions differ",
        ));
    }
    let result = image::attach_staged(pane, staged, Some(pid), call)?;
    Ok(json!({"version":0,"id":id,"result":result}))
}
