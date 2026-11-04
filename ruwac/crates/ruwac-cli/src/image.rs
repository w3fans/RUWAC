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
fn legacy_claude_paste(
    pane: &str,
    path: &Path,
    pid: u64,
    call: &impl Fn(&Request) -> io::Result<Value>,
) -> io::Result<Value> {
    let text = path
        .to_str()
        .ok_or_else(|| io::Error::other("Image path is not UTF-8"))?;
    let agent = guard::validate(
        text,
        u32::try_from(pid).map_err(|_| io::Error::other("Invalid foreground PID"))?,
    )?;
    let job = call(&request("job.query", json!({"pane":pane})))?;
    let screen = call(&request("pane.capture", json!({"pane":pane,"lines":100})))?;
    if agent != guard::Agent::Claude
        || job["result"]["selected"] != true
        || job["result"]["foreground"]["pid"].as_u64() != Some(pid)
        || !guard::ready(agent, screen["result"]["text"].as_str().unwrap_or(""))
    {
        return Err(io::Error::other(
            "Select the Claude Code pane and wait for its input box before attaching an image",
        ));
    }
    call(&request(
        "pane.sendText",
        json!({"pane":pane,"text":format!("\x1b[200~{text}\x1b[201~")}),
    ))?;
    Ok(json!({"result":{"adapter":"claude","legacy_daemon":true}}))
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
    // Daemons before v0.4.54 attach drafts only to Codex, and their agent
    // probe never reports Claude Code. Running daemons are preserved, so for a
    // Claude Code foreground on such a daemon this client (on the daemon's own
    // machine) checks the same conditions and sends the PNG path as a
    // bracketed paste, which Claude Code turns into [Image #N].
    let legacy_claude = u32::try_from(pid).ok().and_then(guard::foreground_agent)
        == Some(guard::Agent::Claude)
        && call(&request("agent.probe", json!({"pane":pane})))
            .map_or(true, |probe| probe["result"]["adapter"] != "claude");
    let queued = if legacy_claude {
        legacy_claude_paste(pane, &staged.path, pid, call)
    } else {
        call(&request(
            "pane.attachImage",
            json!({"pane":pane,"path":staged.path,"foreground_pid":pid}),
        ))
    };
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
    // A real 1x1 PNG, staged through the production stage_png.
    const PNG: &[u8] = &[
        0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 0x0d, 0x49, 0x48, 0x44, 0x52, 0,
        0, 0, 1, 0, 0, 0, 1, 8, 6, 0, 0, 0, 0x1f, 0x15, 0xc4, 0x89, 0, 0, 0, 0x0a, 0x49, 0x44,
        0x41, 0x54, 0x78, 0x9c, 0x63, 0, 1, 0, 0, 5, 0, 1, 0x0d, 0x0a, 0x2d, 0xb4, 0, 0, 0, 0,
        0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
    ];
    fn claude_paste_through(daemon_adapter: &'static str) -> (Value, Vec<(String, Value)>) {
        use std::os::unix::{fs::PermissionsExt, process::CommandExt};
        let root = std::env::temp_dir().join(format!(
            "ruwac-cli-claude-{}-{daemon_adapter}",
            std::process::id()
        ));
        let stage = root.join("stage");
        std::fs::create_dir_all(&stage).unwrap();
        std::fs::set_permissions(&stage, std::fs::Permissions::from_mode(0o700)).unwrap();
        let input = root.join("input.png");
        std::fs::write(&input, PNG).unwrap();
        let staged = ruwac_clipboard::stage_png(&input, &stage).unwrap();
        // A foreground process laid out like Claude Code's native binary.
        let exe = root.join("claude/versions/9.9.9");
        std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
        std::fs::copy("/usr/bin/sleep", &exe).unwrap();
        let mut child = std::process::Command::new(&exe)
            .arg("10")
            .process_group(0)
            .spawn()
            .unwrap();
        std::thread::sleep(Duration::from_millis(50));
        let pid = u64::from(child.id());
        let calls = std::cell::RefCell::new(Vec::new());
        let call = |r: &Request| -> io::Result<Value> {
            calls
                .borrow_mut()
                .push((r.method.clone(), r.params.clone()));
            Ok(match r.method.as_str() {
                "agent.probe" => json!({"result":{"adapter":daemon_adapter}}),
                "job.query" => json!({"result":{"selected":true,"foreground":{"pid":pid}}}),
                "pane.capture" => json!({"result":{"text":"❯ \n  ? for shortcuts"}}),
                "pane.attachImage" => json!({"result":{"adapter":"claude","queued":true}}),
                _ => json!({"result":{}}),
            })
        };
        let result = attach_staged(
            "pane_00000000000000000000000000000000",
            staged,
            Some(pid),
            &call,
        );
        child.kill().unwrap();
        child.wait().unwrap();
        std::fs::remove_dir_all(&root).unwrap();
        (result.unwrap(), calls.into_inner())
    }
    #[test]
    fn claude_drafts_on_daemons_before_v0_4_54_get_a_guarded_bracketed_path() {
        let (result, calls) = claude_paste_through("codex");
        assert_eq!(result["adapter"], "claude");
        assert!(
            !calls.iter().any(|(m, _)| m == "pane.attachImage"),
            "{calls:?}"
        );
        let (_, text) = calls.iter().find(|(m, _)| m == "pane.sendText").unwrap();
        let text = text["text"].as_str().unwrap();
        assert!(
            text.starts_with("\x1b[200~/") && text.ends_with(".png\x1b[201~"),
            "{text}"
        );
    }
    #[test]
    fn current_daemons_attach_claude_drafts_themselves() {
        let (result, calls) = claude_paste_through("claude");
        assert_eq!(result["adapter"], "claude");
        assert!(calls.iter().any(|(m, _)| m == "pane.attachImage"));
        assert!(!calls.iter().any(|(m, _)| m == "pane.sendText"));
    }
    #[test]
    fn image_markers_require_complete_numeric_tokens() {
        assert_eq!(highest_image("› [Image #3] [Image #1]"), 3);
        assert_eq!(highest_image("› [Image #4 [Image #x]"), 0);
    }
}
