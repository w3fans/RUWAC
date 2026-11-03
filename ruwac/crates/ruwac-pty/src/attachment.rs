//! Shared local agent (Codex, Claude Code) file/process guards; no graphical or
//! credential dependencies.
use std::{
    fs,
    io::{self, Read},
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::Path,
};
fn invalid(message: &str) -> io::Error {
    io::Error::other(message)
}
/// A coding agent whose composer accepts a bracketed-pasted PNG path as an
/// image attachment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Agent {
    Codex,
    Claude,
}
impl Agent {
    pub fn name(self) -> &'static str {
        match self {
            Agent::Codex => "codex",
            Agent::Claude => "claude",
        }
    }
}
/// Codex is its own `codex` executable. Claude Code is either the native
/// binary (`claude`, or `.../claude/versions/<version>`) or the npm package run
/// by Node/Bun.
fn agent_of(root: &str) -> Option<Agent> {
    let exe = fs::read_link(format!("{root}/exe")).ok()?;
    let name = exe.file_name()?.to_str()?;
    if name == "codex" {
        return Some(Agent::Codex);
    }
    let parent = exe.parent();
    if name == "claude"
        || (parent
            .and_then(Path::file_name)
            .is_some_and(|n| n == "versions")
            && parent
                .and_then(Path::parent)
                .and_then(Path::file_name)
                .is_some_and(|n| n == "claude"))
    {
        return Some(Agent::Claude);
    }
    if matches!(name, "node" | "bun") {
        let mut cmdline = Vec::new();
        fs::File::open(format!("{root}/cmdline"))
            .ok()?
            .take(4096)
            .read_to_end(&mut cmdline)
            .ok()?;
        if cmdline
            .split(|b| *b == 0)
            .any(|arg| arg.windows(26).any(|w| w == b"@anthropic-ai/claude-code/"))
        {
            return Some(Agent::Claude);
        }
    }
    None
}
fn agent_in_group(pid: u32, group: i32, depth: usize, budget: &mut usize) -> Option<Agent> {
    if depth > 4 || *budget == 0 {
        return None;
    }
    *budget -= 1;
    let root = format!("/proc/{pid}");
    if fs::metadata(&root).map_or(true, |m| m.uid() != unsafe { libc::geteuid() }) {
        return None;
    }
    let stat = fs::read_to_string(format!("{root}/stat")).ok()?;
    let (_, tail) = stat.rsplit_once(')')?;
    let fields: Vec<_> = tail.split_whitespace().collect();
    if fields.get(2).and_then(|v| v.parse::<i32>().ok()) != Some(group) {
        return None;
    }
    if let Some(agent) = agent_of(&root) {
        return Some(agent);
    }
    fs::read_to_string(format!("{root}/task/{pid}/children"))
        .unwrap_or_default()
        .split_whitespace()
        .take(32)
        .filter_map(|s| s.parse().ok())
        .find_map(|child| agent_in_group(child, group, depth + 1, budget))
}
/// The coding agent in this foreground process group, if any.
pub fn foreground_agent(foreground: u32) -> Option<Agent> {
    (foreground != 0)
        .then(|| agent_in_group(foreground, foreground as i32, 0, &mut 64))
        .flatten()
}

pub fn validate_file(path: &str) -> io::Result<()> {
    // A short literal path is essential: never send shell syntax, newlines or large-paste text.
    if !Path::new(&path).is_absolute()
        || path.len() > 512
        || !path
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"/._-".contains(&b))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "attachment requires a short absolute plain PNG path",
        ));
    }
    let parent = Path::new(&path)
        .parent()
        .ok_or_else(|| invalid("staging parent missing"))?;
    let parent_meta =
        fs::symlink_metadata(parent).map_err(|_| invalid("staging parent unavailable"))?;
    if !parent_meta.is_dir()
        || parent_meta.uid() != unsafe { libc::geteuid() }
        || parent_meta.mode() & 0o777 != 0o700
    {
        return Err(invalid("staging parent must be owned and 0700"));
    }
    let file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
        .open(&path)
        .map_err(|_| invalid("staged PNG unavailable"))?;
    let m = file
        .metadata()
        .map_err(|_| invalid("staged PNG unavailable"))?;
    if !m.is_file()
        || m.uid() != unsafe { libc::geteuid() }
        || m.mode() & 0o777 != 0o600
        || m.nlink() != 1
        || m.len() > 64 * 1024 * 1024
    {
        return Err(invalid(
            "PNG must be an owned 0600 regular file within the cap",
        ));
    }
    let mut signature = [0; 8];
    (&file)
        .read_exact(&mut signature)
        .map_err(|_| invalid("PNG signature missing"))?;
    if signature != *b"\x89PNG\r\n\x1a\n" {
        return Err(invalid("PNG signature rejected"));
    }

    Ok(())
}
pub fn validate_process(foreground: u32) -> io::Result<Agent> {
    foreground_agent(foreground)
        .ok_or_else(|| invalid("live same-user Codex or Claude Code foreground required"))
}
pub fn validate(path: &str, foreground: u32) -> io::Result<Agent> {
    validate_file(path)?;
    validate_process(foreground)
}
/// Codex can paint its empty composer before model/image capabilities load.
pub fn model_ready(screen: &str) -> bool {
    screen.contains("GPT-") || screen.contains("context left")
}
/// Claude Code's input footer appears once its composer accepts input.
fn claude_ready(screen: &str) -> bool {
    screen.contains("shift+tab to cycle") || screen.contains("? for shortcuts")
}
pub fn ready(agent: Agent, screen: &str) -> bool {
    match agent {
        Agent::Codex => model_ready(screen),
        Agent::Claude => claude_ready(screen),
    }
}
pub fn highest_image(screen: &str) -> u64 {
    // Only the current composer counts: old turns and model output are not attachment receipts.
    let lines: Vec<_> = screen.lines().collect();
    // Codex marks its composer with '›', Claude Code with '❯'.
    let prompt = |line: &str| line.trim_start().starts_with(['›', '❯']);
    let Some(start) = lines.iter().rposition(|line| prompt(line)) else {
        return 0;
    };
    let composer = lines[start..]
        .iter()
        .take_while(|line| prompt(line) || !(model_ready(line) || claude_ready(line)))
        .copied()
        .collect::<Vec<_>>()
        .join("\n");
    composer
        .split("[Image #")
        .skip(1)
        .filter_map(|s| s.split_once(']').and_then(|(n, _)| n.parse::<u64>().ok()))
        .max()
        .unwrap_or(0)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn history_and_model_output_cannot_acknowledge_a_draft() {
        assert_eq!(
            highest_image(
                "› [Image #9] old turn\n• [Image #40] model output\n› New draft\nGPT-6.1-Sol"
            ),
            0
        );
        assert_eq!(
            highest_image("› [Image #9] old turn\n  › [Image #1] new draft\nGPT-6.1-Sol"),
            1
        );
        assert_eq!(highest_image("› [Image #1] Explain GPT-5\nGPT-6.1-Sol"), 1);
        assert_eq!(
            highest_image("› [Image #1] wrapped\n  [Image #2]\nGPT-6.1-Sol"),
            2
        );
    }
    #[test]
    fn process_scan_has_a_global_budget() {
        assert!(agent_in_group(std::process::id(), 1, 0, &mut 0).is_none());
    }
    #[test]
    fn claude_code_composer_and_footer_are_recognized() {
        let screen = "❯ [Image #1] old turn\n⏺ [Image #7] reply\n❯ [Image #2]\n────\n  ⏵⏵ auto mode on (shift+tab to cycle)";
        assert_eq!(highest_image(screen), 2);
        assert!(ready(Agent::Claude, "  ? for shortcuts"));
        assert!(!ready(Agent::Claude, "Welcome to Claude Code"));
        assert!(!ready(Agent::Codex, "  ? for shortcuts"));
    }
    #[test]
    fn foreground_agents_are_identified_by_their_executable() {
        use std::os::unix::process::CommandExt;
        let dir = std::env::temp_dir().join(format!(
            "ruwac-agent-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let cases = [
            ("codex", Some(Agent::Codex)),
            ("claude", Some(Agent::Claude)),
            ("claude/versions/9.9.9", Some(Agent::Claude)),
            ("other/versions/9.9.9", None),
            ("sleep", None),
        ];
        for (index, (relative, expected)) in cases.into_iter().enumerate() {
            let exe = dir.join(index.to_string()).join(relative);
            fs::create_dir_all(exe.parent().unwrap()).unwrap();
            fs::copy("/usr/bin/sleep", &exe).unwrap();
            let mut child = std::process::Command::new(&exe)
                .arg("5")
                .process_group(0)
                .spawn()
                .unwrap();
            std::thread::sleep(std::time::Duration::from_millis(50));
            let found = foreground_agent(child.id());
            child.kill().unwrap();
            child.wait().unwrap();
            assert_eq!(found, expected, "{relative}");
        }
        fs::remove_dir_all(dir).unwrap();
    }
}
