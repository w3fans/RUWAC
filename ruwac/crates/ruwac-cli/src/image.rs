use ruwac_protocol::{Request, request};
use serde_json::{Value, json};
use std::{
    io,
    path::Path,
    time::{Duration, Instant},
};

#[path = "../../ruwac-pty/src/attachment.rs"]
mod guard;
use guard::highest_image;
pub fn attach(
    pane: &str,
    input: &str,
    directory: &str,
    call: &impl Fn(&Request) -> io::Result<Value>,
) -> io::Result<Value> {
    let initial = call(&request("job.query", json!({"pane":pane})))?;
    let expected = initial["result"]["foreground"]["pid"]
        .as_u64()
        .ok_or_else(|| io::Error::other("live explicit foreground required"))?;
    let staged = ruwac_clipboard::stage_png(Path::new(input), Path::new(directory))
        .map_err(io::Error::other)?;
    attach_staged(pane, staged, Some(expected), call)
}
pub fn attach_staged(
    pane: &str,
    staged: ruwac_clipboard::Receipt,
    expected_pid: Option<u64>,
    call: &impl Fn(&Request) -> io::Result<Value>,
) -> io::Result<Value> {
    let preflight = (|| -> io::Result<(u64, u64)> {
        let job = call(&request("job.query", json!({"pane":pane})))?;
        let pid = job["result"]["foreground"]["pid"]
            .as_u64()
            .ok_or_else(|| io::Error::other("live explicit foreground required"))?;
        if expected_pid.is_some_and(|expected| expected != pid) {
            return Err(io::Error::other("Image target changed during SSH transfer"));
        }
        let screen = call(&request("pane.capture", json!({"pane":pane,"lines":100})))?;
        let before = highest_image(screen["result"]["text"].as_str().unwrap_or(""));
        Ok((pid, before))
    })();
    let (pid, before) = match preflight {
        Ok(v) => v,
        Err(error) => {
            let _ = std::fs::remove_file(&staged.path);
            return Err(error);
        }
    };
    let queued = call(&request(
        "pane.attachImage",
        json!({"pane":pane,"path":staged.path,"foreground_pid":pid}),
    ));
    let adapter = match queued {
        // Older daemons attach only to Codex and do not name the adapter.
        Ok(reply) => reply["result"]["adapter"]
            .as_str()
            .unwrap_or("codex")
            .to_owned(),
        Err(error) => {
            return Err(io::Error::other(format!(
                "{error}; staged PNG retained at {}, inspect the draft before retrying",
                staged.path.display()
            )));
        }
    };
    let end = Instant::now() + Duration::from_secs(3);
    let mut observed = false;
    while Instant::now() < end {
        let current = call(&request("job.query", json!({"pane":pane})))?;
        if current["result"]["foreground"]["pid"].as_u64() != Some(pid) {
            break;
        }
        let screen = call(&request("pane.capture", json!({"pane":pane,"lines":100})))?;
        if highest_image(screen["result"]["text"].as_str().unwrap_or("")) > before {
            observed = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(40));
    }
    // Keep the staged file: the agent reads it when the user explicitly submits their draft.
    Ok(
        json!({"pane":pane,"path":staged.path,"sha256":staged.sha256,"bytes":staged.bytes,"width":staged.width,"height":staged.height,"file_mode":384,"directory_mode":448,"queued":true,"draft_attachment_observed":observed,"consumed":false,"submitted":false,"adapter":adapter}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn image_markers_require_complete_numeric_tokens() {
        assert_eq!(highest_image("› [Image #3] [Image #1]"), 3);
        assert_eq!(highest_image("› [Image #4 [Image #x]"), 0);
    }
}
