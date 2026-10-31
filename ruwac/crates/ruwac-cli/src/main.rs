mod cleanup;
mod doctor;
mod flags;
mod image;
mod image_transfer;
mod remote;
mod startup;
mod workspace_api;
use ruwac_protocol::{Request, read_frame, request, write_frame};
use serde_json::{Value, json};
use std::{
    io::{self, IsTerminal, Read},
    path::PathBuf,
    time::Duration,
};
fn rpc(socket: &std::path::Path, r: &Request) -> io::Result<Value> {
    let mut s = startup::connect(socket)?;
    s.set_read_timeout(Some(Duration::from_secs(65)))?;
    s.set_write_timeout(Some(Duration::from_secs(10)))?;
    write_frame(&mut s, r)?;
    let v = read_frame(&mut s)?;
    if v["version"] != 0 || v["id"] != r.id {
        return Err(io::Error::other("daemon protocol/correlation rejected"));
    }
    if v.get("error").is_some() {
        println!("{}", v);
        return Err(io::Error::other("request failed"));
    }
    Ok(v)
}
// Older live daemons are preserved: discover their schema before invoking a new method.
fn describe(call: impl Fn(&Request) -> io::Result<Value>) -> io::Result<Value> {
    let schema = call(&request("schema", json!({})))?;
    let methods = schema["result"]["methods"]
        .as_array()
        .ok_or_else(|| io::Error::other("daemon discovery schema rejected"))?;
    if methods.iter().any(|m| m == "describe") {
        return call(&request("describe", json!({})));
    }
    let capabilities = call(&request("capabilities", json!({})))?;
    let version = call(&request("version", json!({})))?;
    Ok(
        json!({"version":0,"id":ruwac_protocol::id("request"),"result":{"protocol":0,"native_describe":false,"schema":schema["result"],"capabilities":capabilities["result"],"version":version["result"],"transport":{"local":"same-UID authenticated Unix socket","remote":"authenticated OpenSSH CLI --host","gui":"independent optional service"}}}),
    )
}
fn session_listing(reply: &Value) -> io::Result<String> {
    let sessions = reply["result"]
        .as_array()
        .ok_or_else(|| io::Error::other("invalid session list"))?;
    if sessions.is_empty() {
        return Ok("No RUWAC sessions. Create one with: ruwac -S NAME".into());
    }
    let mut rows: Vec<_> = sessions.iter().collect();
    rows.sort_by_key(|row| row["name"].as_str().unwrap_or(""));
    let mut out = String::from("NAME                 WINDOWS  STATE     SESSION ID\n");
    for row in rows {
        let name: String = row["name"]
            .as_str()
            .unwrap_or("?")
            .chars()
            .filter(|c| !c.is_control())
            .take(40)
            .collect();
        let id: String = row["id"]
            .as_str()
            .unwrap_or("?")
            .chars()
            .filter(|c| !c.is_control())
            .collect();
        let windows = row["windows"].as_array().map_or(0, Vec::len);
        let state = if row["ended"] == true {
            "dead"
        } else if row["restored"] == true {
            "restored"
        } else {
            "running"
        };
        out.push_str(&format!("{name:<20} {windows:>7}  {state:<8}  {id}\n"));
    }
    Ok(out)
}
/// Conventional short forms for existing commands.
fn alias(command: &str) -> Option<&'static str> {
    Some(match command {
        "-v" | "-V" => "--version",
        "-h" | "-?" => "--help",
        "ls" | "list" | "-l" | "--list" => "-ls",
        "kill" | "-k" => "kill-session",
        "new" => "-S",
        _ => return None,
    })
}

#[cfg(test)]
mod viewer_option_tests {
    use super::*;
    #[test]
    fn viewer_options_are_explicit_and_duplicate_or_wrong_targets_fail() {
        let sid = "session_0123456789abcdef0123456789abcdef";
        let args = |values: &[&str]| values.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(
            attach_options(&args(&["--viewer", "--session", sid])).unwrap(),
            (sid, None, true)
        );
        assert_eq!(
            attach_options(&args(&["--session", sid])).unwrap(),
            (sid, None, false)
        );
        for bad in [
            args(&["--viewer"]),
            args(&["--session", sid, "--viewer", "--viewer"]),
            args(&["--session", "window_0123456789abcdef0123456789abcdef"]),
            args(&["--session", sid, "--window"]),
        ] {
            assert!(attach_options(&bad).is_err());
        }
        let mut short = args(&["attach", "-s", sid, "-O"]);
        flags::normalize(&mut short);
        assert_eq!(attach_options(&short[1..]).unwrap(), (sid, None, true));
    }
}

fn attach_options(args: &[String]) -> io::Result<(&str, Option<&str>, bool)> {
    let mut session = None;
    let mut window = None;
    let mut viewer = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--viewer" if !viewer => {
                viewer = true;
                i += 1;
            }
            "--session" if session.is_none() && i + 1 < args.len() => {
                session = Some(args[i + 1].as_str());
                i += 2;
            }
            "--window" if window.is_none() && i + 1 < args.len() => {
                window = Some(args[i + 1].as_str());
                i += 2;
            }
            _ => {
                return Err(io::Error::other(
                    "attach --session session_ID [--window window_ID] [--viewer (-O)]",
                ));
            }
        }
    }
    let session = session
        .filter(|s| ruwac_protocol::valid_id(s, "session"))
        .ok_or_else(|| io::Error::other("attach requires a valid --session ID"))?;
    if window.is_some_and(|w| !ruwac_protocol::valid_id(w, "window")) {
        return Err(io::Error::other("attach requires a valid --window ID"));
    }
    Ok((session, window, viewer))
}

fn main() {
    if let Err(e) = run() {
        eprintln!("ruwac: {e}");
        std::process::exit(1)
    }
}

fn named_request(a: &[String]) -> io::Result<Request> {
    let failure = || {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid or missing explicit target/options; use --help",
        )
    };
    if a.len() < 2 {
        return Err(failure());
    }
    let method = match (a[0].as_str(), a[1].as_str()) {
        ("session", "create") => "session.create",
        ("session", "list") => "session.list",
        ("window", "create") => "window.create",
        ("window", "list") => "window.list",
        ("pane", "list") => "pane.list",
        ("pane", "send-text") => "pane.sendText",
        ("pane", "send-key") => "pane.sendKey",
        ("pane", "capture") => "pane.capture",
        ("pane", "wait") => "pane.wait",
        ("pane", "split") => "pane.create",
        _ => return Err(failure()),
    };
    let allowed: &[&str] = match method {
        "session.create" => &["--name"],
        "session.list" => &[],
        "window.create" | "window.list" | "pane.list" => &["--session"],
        "pane.sendText" => &["--id", "--text", "--file"],
        "pane.sendKey" => &["--id", "--key"],
        "pane.capture" => &["--id", "--lines", "--buffer"],
        "pane.wait" => &["--id", "--contains", "--timeout-ms"],
        "pane.create" => &["--window"],
        _ => &[],
    };
    let mut options = std::collections::HashMap::new();
    let mut i = 2;
    while i < a.len() {
        if a[i] == "--json" || (a[i] == "--detach" && method == "session.create") {
            i += 1;
            continue;
        }
        if !allowed.contains(&a[i].as_str())
            || i + 1 >= a.len()
            || options.insert(a[i].as_str(), a[i + 1].as_str()).is_some()
        {
            return Err(failure());
        }
        i += 2;
    }
    let required = |k: &str| options.get(k).copied().ok_or_else(failure);
    let mut params = json!({});
    for (flag, key) in [
        ("--session", "session"),
        ("--id", "pane"),
        ("--window", "window"),
        ("--name", "name"),
        ("--key", "key"),
        ("--contains", "contains"),
    ] {
        if allowed.contains(&flag) {
            params[key] = json!(required(flag)?);
        }
    }
    for (flag, key) in [("--lines", "lines"), ("--timeout-ms", "timeout_ms")] {
        if let Some(value) = options.get(flag) {
            params[key] = json!(value.parse::<u64>().map_err(|_| failure())?);
        }
    }
    if let Some(value) = options.get("--buffer") {
        if !matches!(*value, "active" | "primary") {
            return Err(failure());
        }
        params["buffer"] = json!(value);
    }
    if method == "pane.sendText" {
        params["text"] = match (options.get("--text"), options.get("--file")) {
            (Some(text), None) => json!(text),
            (None, Some(path)) => {
                let mut bytes = Vec::new();
                std::fs::File::open(path)?
                    .take((ruwac_protocol::MAX_TEXT + 1) as u64)
                    .read_to_end(&mut bytes)?;
                if bytes.len() > ruwac_protocol::MAX_TEXT {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "text exceeds 256 KiB",
                    ));
                }
                json!(String::from_utf8(bytes).map_err(|_| failure())?)
            }
            _ => return Err(failure()),
        };
    }
    Ok(request(method, params))
}

fn job_request(args: &[String]) -> io::Result<Request> {
    let bad = || {
        io::Error::other(
            "job list [--session ID] [--query TEXT] [--offset N] [--limit N] | job query --id pane_ID [--json]",
        )
    };
    let verb = args.first().ok_or_else(bad)?;
    let mut params = json!({});
    let mut i = 1;
    while i < args.len() {
        if args[i] == "--json" && i + 1 == args.len() {
            i += 1;
            continue;
        }
        if i + 1 >= args.len() {
            return Err(bad());
        }
        let key = match (verb.as_str(), args[i].as_str()) {
            ("list", "--session") => "session",
            ("list", "--query") => "query",
            ("list", "--offset") => "offset",
            ("list", "--limit") => "limit",
            ("query", "--id") => "pane",
            _ => return Err(bad()),
        };
        if params.get(key).is_some() {
            return Err(bad());
        }
        params[key] = if matches!(key, "offset" | "limit") {
            json!(args[i + 1].parse::<u64>().map_err(|_| bad())?)
        } else {
            json!(args[i + 1])
        };
        i += 2;
    }
    if verb == "query" && params.get("pane").is_none() {
        return Err(bad());
    }
    if !matches!(verb.as_str(), "list" | "query") {
        return Err(bad());
    }
    Ok(request(&format!("job.{verb}"), params))
}

// Exact identities take precedence; duplicate names never select an arbitrary session.
fn named_session_id(reply: &Value, name: &str) -> io::Result<Option<String>> {
    let sessions = reply["result"]
        .as_array()
        .ok_or_else(|| io::Error::other("invalid session list response"))?;
    if ruwac_protocol::valid_id(name, "session") {
        return Ok(sessions
            .iter()
            .find(|s| s["id"] == name)
            .map(|_| name.to_owned()));
    }
    let matches: Vec<_> = sessions.iter().filter(|s| s["name"] == name).collect();
    match matches.as_slice() {
        [] => Ok(None),
        [session] => session["id"]
            .as_str()
            .map(|id| Some(id.to_owned()))
            .ok_or_else(|| io::Error::other("session has no ID")),
        _ => Err(io::Error::other(
            "multiple sessions have this name; use ruwac -ls, then ruwac -r SESSION_ID",
        )),
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut a: Vec<_> = std::env::args().skip(1).collect();
    flags::normalize(&mut a);
    let mut socket = PathBuf::from(std::env::var("RUWAC_SOCKET").unwrap_or_else(|_| {
        format!(
            "{}/control.sock",
            std::env::var("XDG_RUNTIME_DIR")
                .map(|r| format!("{r}/ruwac"))
                .unwrap_or_else(|_| format!("/tmp/ruwac-{}", unsafe { libc::geteuid() }))
        )
    }));
    let mut host = None;
    let mut remote_binary = None;
    let mut explicit_socket = None;
    while a
        .first()
        .is_some_and(|v| matches!(v.as_str(), "--socket" | "--host" | "--remote-binary"))
    {
        if a.len() < 3 {
            return Err("global option requires a value and command".into());
        }
        let option = a.remove(0);
        let value = a.remove(0);
        match option.as_str() {
            "--socket" => {
                if explicit_socket.replace(PathBuf::from(&value)).is_some() {
                    return Err("duplicate --socket".into());
                }
                socket = PathBuf::from(value);
            }
            "--host" => {
                if host.replace(value).is_some() {
                    return Err("duplicate --host".into());
                }
            }
            "--remote-binary" => {
                if remote_binary.replace(value).is_some() {
                    return Err("duplicate --remote-binary".into());
                }
            }
            _ => unreachable!(),
        }
    }
    if remote_binary.is_some() && host.is_none() {
        return Err("--remote-binary requires --host".into());
    }
    if let Some(first) = a.first_mut() {
        if let Some(canonical) = alias(first) {
            *first = canonical.into();
        }
    }
    if a.first().is_some_and(|command| command == "update") {
        if host.is_some() || explicit_socket.is_some() {
            return Err("run updates locally on each machine, without --host or --socket".into());
        }
        let managed =
            std::env::current_exe().is_ok_and(|exe| ruwac_protocol::managed_rpm_installation(&exe));
        let mut update = std::process::Command::new("python3");
        update
            .arg("-c")
            .arg(include_str!("../../../../scripts/update-user.py"));
        if managed {
            update.arg("--managed-rpm");
        }
        let status = update
            .arg("--current")
            .arg(env!("CARGO_PKG_VERSION"))
            .args(&a[1..])
            .status()?;
        if !status.success() {
            return Err("update failed; see the diagnostics above".into());
        }
        return Ok(());
    }
    let remote = host.map(|host| remote::Remote {
        host,
        binary: remote_binary.unwrap_or_else(|| "ruwac".into()),
        socket: explicit_socket,
    });
    if let Some(remote) = &remote {
        remote.validate()?;
        match a.first().map(String::as_str) {
            Some("doctor") => {
                doctor::options(&a[1..])?;
                if a[1..].iter().any(|s| s == "--repair") {
                    return Err("remote terminal repair requires running doctor directly in its foreground TTY".into());
                }
                let v = remote.doctor(&a[1..])?;
                println!("{v}");
                if v["result"]["ok"] != true {
                    return Err("remote diagnostics failed".into());
                }
                return Ok(());
            }
            Some("start") => {
                if a.len() != 1 {
                    return Err("start takes no command options".into());
                }
                println!("{}", remote.start()?);
                return Ok(());
            }
            Some("attach") => {
                let (session, window, viewer) = attach_options(&a[1..])?;
                remote.attach_with_options(session, window, viewer)?;
                return Ok(());
            }
            Some("workspace" | "rdp" | "host") => {
                let req = workspace_api::parse(&a[0], &a[1..])?;
                let v = remote.gui(&req)?;
                println!("{v}");
                if v.get("error").is_some() {
                    return Err("remote GUI operation failed".into());
                }
                return Ok(());
            }
            Some("image-attach") => {
                if a.len() != 4 {
                    return Err(
                        "--host HOST image-attach pane_ID LOCAL_PNG REMOTE_PRIVATE_DIRECTORY"
                            .into(),
                    );
                }
                println!(
                    "{}",
                    image_transfer::send(
                        &remote.host,
                        &|q| remote.rpc(q),
                        &|input| remote.image_transfer(input),
                        &a[1],
                        &a[2],
                        &a[3]
                    )?
                );
                return Ok(());
            }
            Some("gui" | "serve" | "image-stage" | "image-receive" | "gui-request") => return Err(
                "--host supports authenticated control; launch the GUI directly on its endpoint"
                    .into(),
            ),
            _ => (),
        }
    }
    let call = |request: &Request| match &remote {
        Some(remote) => remote.rpc(request),
        None => rpc(&socket, request),
    };
    // Resolve defaults where the new shell will run. Raw RPC/request remains literal.
    // Supplying argv also fixes shell startup against an already-running older daemon.
    let spawn_call = |request: &Request| {
        let mut request = request.clone();
        if matches!(
            request.method.as_str(),
            "session.create" | "window.create" | "pane.create"
        ) && request.params.get("argv").is_none()
        {
            request.params["argv"] = json!(match &remote {
                Some(remote) => ruwac_pty::shell::terminal_argv(remote.shell_argv()?),
                None => ruwac_pty::shell::default_terminal_argv().map_err(io::Error::other)?,
            });
        }
        call(&request)
    };
    if a.len() >= 4 && a[0] == "-S" && a[2] == "-X" {
        let sessions = call(&request("session.list", json!({})))?;
        let id = named_session_id(&sessions, &a[1])?.ok_or("No exact named session")?;
        let session = sessions["result"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["id"] == id)
            .unwrap();
        let window = session["active_window"].clone();
        let name = ruwac_protocol::screen_command(&a[3]);
        let args = &a[4..];
        let reply = match (name, args) {
            ("new", [] | [_]) => spawn_call(&request("window.create", json!({"session":id,"name":args.first().map(String::as_str).unwrap_or("shell")})))?,
            ("rename", [_, ..]) => call(&request("window.rename", json!({"window":window,"name":args.join(" ")})))?,
            ("windows", []) => call(&request("window.list", json!({"session":id})))?,
            ("next" | "prev", []) => call(&request("window.cycle", json!({"session":id,"previous":name=="prev"})))?,
            ("other", []) => call(&request("window.previous", json!({"session":id})))?,
            ("select", [index]) => {
                let windows = call(&request("window.list", json!({"session":id})))?;
                let row = windows["result"].as_array().and_then(|rows| index.parse::<usize>().ok().and_then(|i| rows.get(i))).ok_or("select requires an available window index")?;
                call(&request("window.select", json!({"window":row["id"]})))?
            }
            ("kill" | "quit", []) => {
                let panes = call(&request("pane.list", json!({"session":id})))?;
                for pane in panes["result"].as_array().ok_or("Pane list unavailable")?.iter().filter(|p| name == "quit" || p["window"] == window) {
                    call(&request("pane.close", json!({"pane":pane["id"]})))?;
                }
                json!({"result":{"closed":true}})
            }
            _ => return Err("Supported -S NAME -X commands: screen [NAME], title NAME, windows, next, prev, other, select N, kill, quit".into()),
        };
        println!("{reply}");
        return Ok(());
    }
    if a.len() == 3 && matches!(a[0].as_str(), "-r" | "-x") && a[1] == "--session" {
        a.remove(1);
    }
    match a.first().map(String::as_str) {
        Some("-wipe" | "wipe") => {
            let args: Vec<_> = a[1..].iter().filter(|v| v.as_str() != "--json").collect();
            if args.len() > 1 || a.len() > 3 {
                return Err("usage: ruwac -wipe [NAME_OR_SESSION_ID] [--json]".into());
            }
            let selected = args
                .first()
                .map(|name| {
                    named_session_id(&call(&request("session.list", json!({})))?, name)?
                        .ok_or_else(|| io::Error::other("No session with this exact name or ID"))
                })
                .transpose()?;
            let result = cleanup::wipe(selected.as_deref(), |m, p| {
                call(&request(m, p)).map(|v| v["result"].clone())
            })?;
            if a.iter().any(|v| v == "--json") {
                println!("{result}");
            } else {
                println!(
                    "Removed {} dead sessions and {} dead windows; live processes preserved.",
                    result["removed_sessions"], result["removed_windows"]
                );
            }
        }
        Some("kill-session") => {
            if a.len() != 2 {
                return Err(
                    "usage: ruwac kill-session NAME_OR_SESSION_ID (closes its programs)".into(),
                );
            }
            let id = named_session_id(&call(&request("session.list", json!({})))?, &a[1])?
                .ok_or("No session with this exact name or ID")?;
            let panes = call(&request("pane.list", json!({"session":id})))?;
            for pane in panes["result"].as_array().ok_or("Pane list unavailable")? {
                call(&request("pane.close", json!({"pane":pane["id"]})))?;
            }
            if named_session_id(&call(&request("session.list", json!({})))?, &id)?.is_some() {
                return Err("Session changed while closing; inspect ruwac -ls and retry".into());
            }
            println!("Closed session {id}");
        }
        Some("-ls" | "-list") => {
            if a.len() > 2 || (a.len() == 2 && a[1] != "--json") {
                return Err("usage: ruwac -ls [--json]".into());
            }
            let mut reply = call(&request("session.list", json!({})))?;
            if a.len() == 2 {
                println!("{reply}");
            } else {
                if let Some(rows) = reply["result"].as_array_mut() {
                    for row in rows {
                        let panes = call(&request("pane.list", json!({"session":row["id"]})))?;
                        row["ended"] = json!(panes["result"].as_array().is_some_and(|ps| {
                            ps.iter()
                                .all(|p| p["ended"] == true || p["restored"] == true)
                        }));
                    }
                }
                println!("{}", session_listing(&reply)?);
            }
        }
        Some("-S" | "-r" | "-x" | "-R" | "-dmS") => {
            if a.len() != 2 || a[1].trim().is_empty() || a[1].chars().any(char::is_control) {
                return Err(
                    "usage: ruwac -S NAME (create/attach) or ruwac -r NAME_OR_SESSION_ID (reattach only)".into(),
                );
            }
            if !matches!(a[0].as_str(), "-dmS")
                && (!io::stdin().is_terminal() || !io::stdout().is_terminal())
            {
                return Err("-S/-r requires an interactive terminal; use session create --name NAME --detach --json for automation".into());
            }
            if matches!(a[0].as_str(), "-S" | "-R" | "-dmS") {
                match &remote {
                    Some(remote) => {
                        remote.start()?;
                    }
                    None => {
                        startup::ensure(&socket)?;
                    }
                }
            }
            let mut id = match named_session_id(&call(&request("session.list", json!({})))?, &a[1])? {
                Some(id) => id,
                None if matches!(a[0].as_str(), "-r" | "-x") || ruwac_protocol::valid_id(&a[1], "session") => {
                    return Err("No session with this exact name or ID; use -S NAME to create a named session".into());
                }
                None => {
                    spawn_call(&request("session.create", json!({"name": a[1]})))?["result"]["session"]
                        .as_str()
                        .ok_or("session creation returned no ID")?
                        .to_owned()
                }
            };
            let panes = call(&request("pane.list", json!({"session":id})))?;
            let dead = panes["result"].as_array().is_some_and(|rows| {
                !rows.is_empty()
                    && rows
                        .iter()
                        .all(|p| p["ended"] == true || p["restored"] == true)
            });
            if dead {
                if matches!(a[0].as_str(), "-r" | "-x") {
                    return Err("This session has ended. Use -S NAME to start a new one, or -wipe to remove dead sessions. Explicit history remains available through attach --session ID.".into());
                }
                let listed = call(&request("session.list", json!({})))?;
                let name = listed["result"]
                    .as_array()
                    .and_then(|rows| rows.iter().find(|row| row["id"] == id))
                    .and_then(|row| row["name"].as_str())
                    .ok_or("Session changed; retry")?
                    .to_owned();
                cleanup::wipe(Some(&id), |m, p| {
                    call(&request(m, p)).map(|v| v["result"].clone())
                })?;
                id = spawn_call(&request("session.create", json!({"name":name})))?["result"]["session"].as_str().ok_or("Session creation returned no ID")?.to_owned();
            }
            if matches!(a[0].as_str(), "-dmS") {
                println!("Detached session {} ({id})", a[1]);
                return Ok(());
            }
            match &remote {
                Some(remote) => remote.attach(&id)?,
                None => ruwac_tui::attach(&socket, &id)?,
            }
        }
        Some("doctor") => {
            let v = doctor::run(&socket, &a[1..])?;
            println!("{v}");
            if v["result"]["ok"] != true {
                return Err("requested diagnostics failed".into());
            }
        }
        Some("--version") => println!("RUWAC {}", env!("CARGO_PKG_VERSION")),
        Some("session" | "window" | "pane") => println!("{}", spawn_call(&named_request(&a)?)?),
        Some("shell-argv") => {
            if remote.is_some() || a.len() != 1 {
                return Err("shell-argv is endpoint-local and takes no options".into());
            }
            println!(
                "{}",
                json!({"version":0,"result":{"argv":ruwac_pty::shell::default_argv().map_err(io::Error::other)?}})
            );
        }
        Some("job") => println!("{}", call(&job_request(&a[1..])?)?),
        Some("gui") => {
            use std::os::unix::process::CommandExt;
            let binary = std::env::current_exe()?;
            let installed = binary
                .parent()
                .unwrap_or(std::path::Path::new("."))
                .join("ruwac-desktop");
            let entry = std::env::var_os("RUWAC_GUI_ENTRY")
                .map(PathBuf::from)
                .unwrap_or(installed);
            if !entry.is_file() {
                return Err(
                    "GUI unavailable: optional compiled ruwac-desktop is not installed".into(),
                );
            }
            if !a[1..]
                .iter()
                .any(|s| matches!(s.as_str(), "--help" | "--version"))
            {
                startup::ensure(&socket)?;
            }
            return Err(std::process::Command::new(entry)
                .arg("--socket")
                .arg(&socket)
                .args(&a[1..])
                .exec()
                .into());
        }
        Some("start") => {
            if a.len() != 1 {
                return Err("start takes only --socket before command".into());
            }
            let started = startup::ensure(&socket)?;
            println!(
                "{}",
                json!({"version":0,"result":{"ready":true,"start_requested":started}})
            );
        }
        Some("workspace" | "rdp" | "host") => {
            let req = workspace_api::parse(&a[0], &a[1..])?;
            let v = workspace_api::exchange(&socket, &req)?;
            println!("{v}");
            if v.get("error").is_some() {
                return Err("GUI operation failed".into());
            }
        }
        Some("gui-request") => {
            if a.len() != 1 {
                return Err("gui-request takes one JSON stdin request and no options".into());
            }
            let mut bytes = Vec::new();
            io::stdin()
                .take(ruwac_protocol::MAX_FRAME as u64 + 1)
                .read_to_end(&mut bytes)?;
            if bytes.len() > ruwac_protocol::MAX_FRAME {
                return Err("GUI request exceeds frame cap".into());
            }
            let req: Request = serde_json::from_slice(&bytes)?;
            let v = workspace_api::exchange(&socket, &req)?;
            println!("{v}");
            if v.get("error").is_some() {
                return Err("GUI operation failed".into());
            }
        }
        Some("serve") => {
            if a.len() != 1 {
                return Err("serve takes only --socket before command".into());
            }
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()?
                .block_on(ruwac_daemon::serve(&socket))?;
        }
        Some("attach") => {
            let (session, window, viewer) = attach_options(&a[1..])?;
            drop(startup::connect(&socket)?);
            ruwac_tui::attach_with_role(&socket, session, window, viewer)?;
        }
        Some("rpc") => {
            if a.len() < 2 {
                return Err("rpc METHOD [--params JSON] [--idempotency KEY]".into());
            }
            let mut r = request(&a[1], json!({}));
            let mut i = 2;
            while i < a.len() {
                let value = a.get(i + 1).ok_or("option requires value")?;
                match a[i].as_str() {
                    "--params" => r.params = serde_json::from_str(value)?,
                    "--idempotency" => r.idempotency_key = Some(value.clone()),
                    _ => return Err("unknown rpc option".into()),
                }
                i += 2;
            }
            println!("{}", call(&r)?);
        }
        Some("request") => {
            let mut b = String::new();
            io::stdin()
                .take((ruwac_protocol::MAX_FRAME + 1) as u64)
                .read_to_string(&mut b)?;
            if b.len() > ruwac_protocol::MAX_FRAME {
                return Err("request exceeds 1 MiB".into());
            }
            let r: Request = serde_json::from_str(&b)?;
            println!("{}", call(&r)?);
        }
        Some("image-receive") => {
            if a.len() != 1 {
                return Err("image-receive reads a bounded SSH binary stdin transfer".into());
            }
            println!("{}", image_transfer::receive(&call)?);
        }
        Some("image-attach") => {
            if a.len() != 4 {
                return Err(
                    "image-attach pane_ID PNG PRIVATE_DIRECTORY (local Codex draft only)".into(),
                );
            }
            println!("{}", image::attach(&a[1], &a[2], &a[3], &call)?);
        }
        Some("image-stage") => {
            if a.len() != 3 {
                return Err("image-stage PNG PRIVATE_DIRECTORY".into());
            }
            let r = ruwac_clipboard::stage_png(
                std::path::Path::new(&a[1]),
                std::path::Path::new(&a[2]),
            )
            .map_err(io::Error::other)?;
            println!(
                "{}",
                json!({"path":r.path,"mime":"image/png","bytes":r.bytes,"sha256":r.sha256,"width":r.width,"height":r.height,"attached":false})
            );
        }
        Some("describe") => {
            if a.len() > 2 || (a.len() == 2 && a[1] != "--json") {
                return Err("describe takes only optional --json".into());
            }
            println!("{}", describe(call)?);
        }
        Some("health" | "version" | "capabilities" | "schema") => {
            if a.len() > 2 || (a.len() == 2 && a[1] != "--json") {
                return Err("discovery takes only optional --json".into());
            }
            println!("{}", call(&request(&a[0], json!({})))?);
        }
        Some("--help" | "help") | None => {
            println!(
                "Global short options: {}\nCommand short options: {}\nGUI short options: {}\nUse ruwac update -h for update short options (including -i install). Values after short options are literal; combined short options are not supported.",
                flags::help(flags::GLOBAL),
                flags::help(flags::CONTROL),
                flags::help(flags::DESKTOP)
            );
            println!(
                "Updates (Fedora 44): ruwac update --check | --install | --rollback | --auto-check on|off|status | --auto-install on|off|status | --testing-bundles on|off|status.\nTesting bundle delivery is opt-in for Fedora 44 x86_64; source builds require development dependencies; --headless retains a CLI-only installation. No running processes are restarted."
            );
            println!(
                "RUWAC\n  ruwac -ls [--json]  # list persistent sessions\n  ruwac -wipe [NAME_OR_SESSION_ID] [--json]  # remove only dead windows/sessions\n  ruwac -S NAME -X COMMAND  # Screen control aliases; kill/quit close programs\n  ruwac -dmS NAME  # create a detached named session\n  ruwac -x NAME  # share an existing session (also -r)\n  ruwac -R NAME  # attach or create an exact named session\n  ruwac -S NAME  # create or attach a named terminal session\n  ruwac -r NAME_OR_SESSION_ID  # reattach an existing session\n  ruwac kill-session NAME_OR_SESSION_ID  # closes all programs in this session\n  ruwac job list [--session session_ID] [--query TEXT] --json\n  ruwac job query --id pane_ID --json\n  ruwac workspace describe|list|saved [--json]\n  ruwac workspace events subscribe --types tab,focus,layout,rdp --json\n  ruwac workspace events poll --subscription subscription_ID --after CURSOR --limit 50 --json\n  ruwac workspace events unsubscribe --subscription subscription_ID --json\n  ruwac workspace save --name NAME --json\n  ruwac workspace open|restore --id workspace_ID --json\n  ruwac host list [--query TEXT] [--group GROUP] [--protocol SSH|RDP] [--favorites] [--limit 1..50] [--offset N --snapshot TOKEN] --json\n  ruwac host query --id endpoint_ID --json\n  ruwac rdp list --json\n  ruwac rdp query|reconnect|disconnect --id view_ID --json\n  ruwac rdp open --profile endpoint_ID [--activate] --json\n  ruwac workspace activate --id tab_ID --json\n  ruwac workspace focus --id view_ID --json\n  ruwac doctor [--desktop] [--terminal [--repair]] [--json]\n  ruwac --version  # also -v / -V\n  Short forms: -h help · ls/list/-l sessions · new NAME = -S NAME · kill NAME = kill-session NAME\n  ruwac start  # starts/reuses a private persistent daemon\n  ruwac gui [--session session_ID]\n  ruwac session create --name NAME --detach --json\n  ruwac session list --json\n  ruwac pane list --session session_ID --json\n  ruwac pane send-text --id pane_ID --text TEXT --json\n  ruwac pane send-text --id pane_ID --file UTF8_FILE --json\n  ruwac pane send-key --id pane_ID --key Enter --json\n  ruwac pane capture --id pane_ID --lines 250 [--buffer active|primary] --json\n  ruwac pane wait --id pane_ID --contains TEXT --timeout-ms 5000 --json\n  ruwac --socket PATH serve\n  ruwac --socket PATH rpc METHOD --params JSON [--idempotency KEY]\n  ruwac --socket PATH request < request.json\n  ruwac --socket PATH attach --session session_ID [--window window_ID] [--viewer (-O)]\n  ruwac image-stage PNG PRIVATE_DIRECTORY\n  ruwac --socket PATH image-attach pane_ID PNG PRIVATE_DIRECTORY\n  ruwac --socket PATH health|version|capabilities|schema|describe\nRemote: ruwac --host SSH_HOST [--remote-binary PATH] [--socket REMOTE_PATH] COMMAND.\n  ruwac --host SSH_HOST describe\n  ruwac --host SSH_HOST pane send-text --id pane_ID --file LOCAL_UTF8_FILE\n  ruwac --host SSH_HOST attach --session session_ID [--window window_ID] [--viewer (-O)]\n  ruwac --host SSH_HOST image-attach pane_ID LOCAL_PNG REMOTE_PRIVATE_DIRECTORY\nHost/workspace/RDP commands address an already-running remote GUI with its explicit permissions. Uses existing SSH keys/config and verified host keys; no credential or agent forwarding. Without explicit --socket, the remote CLI selects its own private default.\nImage attachment: explicit local or SSH Codex draft; no automatic submission. No desktop clipboard or full Screen compatibility claim."
            );
        }
        _ => return Err("unknown command; use --help".into()),
    }
    Ok(())
}

#[cfg(test)]
mod named_session_tests {
    use super::*;

    #[test]
    fn short_options_create_identical_requests_and_keep_literal_text() {
        let mut short = vec!["pane", "send-text", "-i", "pane_fixture", "-t", "-j", "-j"]
            .into_iter()
            .map(str::to_owned)
            .collect::<Vec<_>>();
        flags::normalize(&mut short);
        let request = named_request(&short).unwrap();
        assert_eq!(request.method, "pane.sendText");
        assert_eq!(request.params["pane"], "pane_fixture");
        assert_eq!(request.params["text"], "-j");
        let mut attach = vec!["-r".into(), "-s".into(), "session_fixture".into()];
        flags::normalize(&mut attach);
        assert_eq!(attach[1], "--session");
    }
    #[test]
    fn conventional_short_forms_map_to_existing_commands() {
        assert_eq!(alias("-v"), Some("--version"));
        assert_eq!(alias("-V"), Some("--version"));
        assert_eq!(alias("-h"), Some("--help"));
        assert_eq!(alias("ls"), Some("-ls"));
        assert_eq!(alias("new"), Some("-S"));
        assert_eq!(alias("kill"), Some("kill-session"));
        // Discovery and Screen flags keep their meaning.
        for kept in ["version", "-ls", "-S", "-r", "-x", "update", "gui"] {
            assert_eq!(alias(kept), None);
        }
    }

    #[test]
    fn readable_list_drops_terminal_controls_and_handles_empty() {
        let text = session_listing(&json!({"result":[{"name":"demo\u{001b}[2J\n", "id":"session_test", "windows":["w"], "restored":false}]})).unwrap();
        assert!(!text.contains('\u{1b}'));
        assert!(text.contains("session_test"));
        assert!(
            session_listing(&json!({"result":[]}))
                .unwrap()
                .contains("No RUWAC sessions")
        );
        assert!(session_listing(&json!({"result":{}})).is_err());
    }

    #[test]
    fn exact_name_or_new_session() {
        let reply = json!({"result": [{"id": "session_a", "name": "d"}, {"id": "session_b", "name": "daily"}]});
        assert_eq!(
            named_session_id(&reply, "d").unwrap(),
            Some("session_a".into())
        );
        assert_eq!(named_session_id(&reply, "D").unwrap(), None);
        assert_eq!(named_session_id(&reply, "new").unwrap(), None);
    }

    #[test]
    fn exact_id_selects_one_of_duplicate_names_and_stale_id_never_becomes_name() {
        let a = ruwac_protocol::id("session");
        let b = ruwac_protocol::id("session");
        let missing = ruwac_protocol::id("session");
        let reply = json!({"result":[{"id":a,"name":"Terminal"},{"id":b,"name":"Terminal"},{"id":"session_test","name":missing}]});
        assert!(named_session_id(&reply, "Terminal").is_err());
        assert_eq!(named_session_id(&reply, &a).unwrap(), Some(a));
        assert_eq!(named_session_id(&reply, &b).unwrap(), Some(b));
        assert_eq!(named_session_id(&reply, &missing).unwrap(), None);
    }

    #[test]
    fn ambiguous_or_malformed_sessions_are_rejected() {
        assert!(
            named_session_id(
                &json!({"result": [{"id":"a","name":"d"},{"id":"b","name":"d"}]}),
                "d"
            )
            .is_err()
        );
        assert!(named_session_id(&json!({"error": "failed"}), "d").is_err());
        assert!(named_session_id(&json!({"result": [{"name":"d"}]}), "d").is_err());
    }
}
