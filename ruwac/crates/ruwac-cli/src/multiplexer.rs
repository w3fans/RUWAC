//! A deliberately bounded command dialect, not a tmux configuration interpreter.
//! All destructive/input commands require explicit, stable resource targets.
use ruwac_protocol::{Request, request, valid_id};
use serde_json::{Value, json};
use std::{collections::HashMap, io};

fn bad(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

fn options<'a>(
    args: &'a [String],
    flags: &[&str],
    switches: &[&str],
) -> io::Result<(HashMap<&'a str, &'a str>, Vec<&'a str>)> {
    let mut result = HashMap::new();
    let mut positional = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let arg = args[i].as_str();
        if arg == "--" {
            positional.extend(args[i + 1..].iter().map(String::as_str));
            break;
        }
        if switches.contains(&arg) {
            if result.insert(arg, "").is_some() {
                return Err(bad("Duplicate option"));
            }
        } else if flags.contains(&arg) {
            i += 1;
            let value = args.get(i).ok_or_else(|| bad("Missing option value"))?;
            if result.insert(arg, value.as_str()).is_some() {
                return Err(bad("Duplicate option"));
            }
        } else if arg.starts_with('-') {
            return Err(bad(
                "Unsupported option; this is RUWAC's bounded command dialect (see --help)",
            ));
        } else {
            positional.push(arg);
        }
        i += 1;
    }
    Ok((result, positional))
}

pub fn run(
    args: &[String],
    call: impl Fn(&Request) -> io::Result<Value>,
    spawn: impl Fn(&Request) -> io::Result<Value>,
) -> io::Result<Option<Value>> {
    let Some(command) = args.first().map(String::as_str) else {
        return Ok(None);
    };
    if command == "list-sessions" {
        if args.len() != 1 {
            return Err(bad(
                "list-sessions takes no options; use session list --json",
            ));
        }
        return call(&request("session.list", json!({}))).map(Some);
    }
    let (flags, switches): (&[&str], &[&str]) = match command {
        "new-window" => (&["-t", "-n"], &[]),
        "split-window" => (&["-t"], &["-h", "-v"]),
        "send-keys" => (&["-t"], &["-l"]),
        "has-session" | "list-windows" | "next-window" | "previous-window" | "last-window"
        | "select-window" | "rename-window" | "select-pane" | "kill-pane" => (&["-t"], &[]),
        _ => return Ok(None),
    };
    let (opts, words) = options(&args[1..], flags, switches)?;
    let target = *opts.get("-t").ok_or_else(|| {
        bad("An explicit -t target is required; use stable IDs from session/window/pane list")
    })?;
    if !matches!(command, "rename-window" | "send-keys") && !words.is_empty() {
        return Err(bad("Unexpected positional argument"));
    }
    if command == "rename-window" && words.len() != 1 {
        return Err(bad(
            "rename-window -t window_ID NAME (quote a name containing spaces)",
        ));
    }
    // Resolve exact names only for session commands. Window/pane targets are never
    // fuzzy names, global focus, tmux indices or implicit environment variables.
    let session_command = matches!(
        command,
        "new-window"
            | "has-session"
            | "list-windows"
            | "next-window"
            | "previous-window"
            | "last-window"
    );
    let target = if session_command {
        super::named_session_id(&call(&request("session.list", json!({})))?, target)?
            .ok_or_else(|| bad("No session with this exact name or ID"))?
    } else {
        let kind = if matches!(command, "select-pane" | "kill-pane" | "send-keys") {
            "pane"
        } else {
            "window"
        };
        if !valid_id(target, kind) {
            return Err(bad("Target must be a stable RUWAC window_ID or pane_ID"));
        }
        target.to_owned()
    };
    let (method, params) = match command {
        "has-session" => return Ok(Some(json!({"result":{"exists":true,"session":target}}))),
        "list-windows" => ("window.list", json!({"session":target})),
        "next-window" | "previous-window" => (
            "window.cycle",
            json!({"session":target,"previous":command=="previous-window"}),
        ),
        "last-window" => ("window.previous", json!({"session":target})),
        "new-window" => {
            let name = opts.get("-n").copied().unwrap_or("shell");
            ruwac_protocol::checked_text(name).map_err(|e| bad(&e.message))?;
            return spawn(&request(
                "window.create",
                json!({"session":target,"name":name}),
            ))
            .map(Some);
        }
        "select-window" => ("window.select", json!({"window":target})),
        "rename-window" => {
            ruwac_protocol::checked_text(words[0]).map_err(|e| bad(&e.message))?;
            ("window.rename", json!({"window":target,"name":words[0]}))
        }
        "select-pane" => ("pane.select", json!({"pane":target})),
        "kill-pane" => ("pane.close", json!({"pane":target})),
        "split-window" => {
            if opts.contains_key("-h") && opts.contains_key("-v") {
                return Err(bad("Choose -h or -v, not both"));
            }
            // tmux -h means side-by-side, represented by RUWAC's vertical divider.
            let orientation = if opts.contains_key("-h") {
                "vertical"
            } else {
                "horizontal"
            };
            return spawn(&request(
                "pane.create",
                json!({"window":target,"orientation":orientation}),
            ))
            .map(Some);
        }
        "send-keys" => {
            if !opts.contains_key("-l") || words.is_empty() {
                return Err(bad(
                    "send-keys -t pane_ID -l TEXT... sends literal text; use pane send-key for named keys",
                ));
            }
            // tmux's literal arguments concatenate without inserted spaces. Build
            // and validate the whole payload before a single write (no partial send).
            let text = words.concat();
            ruwac_protocol::checked_text(&text).map_err(|e| bad(&e.message))?;
            ("pane.sendText", json!({"pane":target,"text":text}))
        }
        _ => unreachable!(),
    };
    call(&request(method, params)).map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    fn args(s: &[&str]) -> Vec<String> {
        s.iter().map(|v| v.to_string()).collect()
    }
    const SID: &str = "session_0123456789abcdef0123456789abcdef";
    const WID: &str = "window_0123456789abcdef0123456789abcdef";
    const PID: &str = "pane_0123456789abcdef0123456789abcdef";
    #[test]
    fn explicit_targets_literals_and_orientation() {
        let seen = RefCell::new(Vec::new());
        let call = |q: &Request| {
            seen.borrow_mut().push((q.method.clone(), q.params.clone()));
            Ok(json!({"result":true}))
        };
        run(
            &args(&["send-keys", "-t", PID, "-l", "--", "-n", " ž\n"]),
            call,
            call,
        )
        .unwrap();
        assert_eq!(
            seen.borrow()[0],
            ("pane.sendText".into(), json!({"pane":PID,"text":"-n ž\n"}))
        );
        run(&args(&["split-window", "-t", WID, "-h"]), call, call).unwrap();
        assert_eq!(seen.borrow()[1].1["orientation"], "vertical");
        run(&args(&["split-window", "-t", WID, "-v"]), call, call).unwrap();
        assert_eq!(seen.borrow()[2].1["orientation"], "horizontal");
    }
    #[test]
    fn rejects_implicit_fuzzy_unknown_or_partial_input_before_writes() {
        let call = |_: &Request| -> io::Result<Value> { panic!("invalid command reached daemon") };
        for values in [
            args(&["kill-pane"]),
            args(&["kill-pane", "-t", "0"]),
            args(&["kill-pane", "-t", PID, "-a"]),
            args(&["kill-pane", "-t", PID, "-t", PID]),
            args(&["rename-window", "-t", WID]),
            args(&["split-window", "-t", WID, "-h", "-v"]),
            args(&["send-keys", "-t", PID, "Enter"]),
            args(&["send-keys", "-t", PID, "-l", "safe", "bad\0"]),
        ] {
            assert!(run(&values, call, call).is_err());
        }
        assert!(
            run(&args(&["source-file", "anything"]), call, call)
                .unwrap()
                .is_none()
        );
    }
    #[test]
    fn session_names_are_exact_and_ambiguous_names_do_not_route() {
        let call = |q: &Request| -> io::Result<Value> {
            assert_eq!(q.method, "session.list");
            Ok(
                json!({"result":[{"id":SID,"name":"demo"},{"id":"session_11111111111111111111111111111111","name":"demo"}]}),
            )
        };
        assert!(run(&args(&["has-session", "-t", "demo"]), call, call).is_err());
        assert!(run(&args(&["has-session", "-t", "dem"]), call, call).is_err());
        assert_eq!(
            run(&args(&["has-session", "-t", SID]), call, call)
                .unwrap()
                .unwrap()["result"]["session"],
            SID
        );
    }
}
