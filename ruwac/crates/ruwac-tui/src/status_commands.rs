//! Bounded foreground-command metadata for status labels; never reads argv.
use serde_json::{Value, json};
use std::collections::HashMap;
use std::io;
use std::time::{Duration, Instant};

struct Probe {
    pid: u64,
    checked: Instant,
    codex: bool,
}
#[derive(Default)]
pub struct Labels {
    probes: HashMap<String, Probe>,
}

fn node_runtime(name: &str) -> bool {
    matches!(name, "node" | "nodejs")
        || name.strip_prefix("node-").is_some_and(|version| {
            !version.is_empty()
                && version.bytes().all(|b| b.is_ascii_digit() || b == b'.')
                && version.as_bytes()[0].is_ascii_digit()
        })
}

impl Labels {
    pub fn refresh(
        &mut self,
        frame: &mut Value,
        mut call: impl FnMut(&str, Value) -> io::Result<Value>,
    ) -> bool {
        let collect = (|| -> Option<Value> {
            let session = frame["session"].as_str()?;
            let windows = call("window.list", json!({"session":session})).ok()?;
            if windows.as_array()?.len() > 128 {
                return None;
            }
            let mut jobs = Vec::new();
            let mut offset = 0;
            let mut membership = None;
            for page in 0..128 {
                let result = call(
                    "job.list",
                    json!({"session":session,"offset":offset,"limit":100}),
                )
                .ok()?;
                if page != 0 && membership != result.get("membership").cloned() {
                    return None;
                }
                membership = result.get("membership").cloned();
                let rows = result["jobs"].as_array()?;
                if rows.len() > 100 || jobs.len() + rows.len() > 128 {
                    return None;
                }
                jobs.extend(rows.iter().cloned());
                if result["next_offset"].is_null() {
                    break;
                }
                let next = result["next_offset"].as_u64()?;
                if page == 127 || next != offset + rows.len() as u64 || next <= offset || next > 128
                {
                    return None;
                }
                offset = next;
            }
            let mut commands = serde_json::Map::new();
            let mut nodes = Vec::new();
            for window in windows.as_array()? {
                let id = window["id"].as_str()?;
                let pane = if frame["window"] == id {
                    &frame["active_pane"]
                } else {
                    &window["active_pane"]
                };
                let Some(job) = jobs
                    .iter()
                    .find(|job| job["pane"] == *pane && job["pane_ended"] != true)
                else {
                    continue;
                };
                let command = job["foreground"]["command"]
                    .as_str()
                    .or_else(|| job["command"].as_str());
                if let Some(command) = command.filter(|s| !s.is_empty() && s.len() <= 256) {
                    let name = std::path::Path::new(command)
                        .file_name()
                        .and_then(|s| s.to_str())
                        .unwrap_or(command);
                    let name: String = name.chars().filter(|c| !c.is_control()).collect();
                    if node_runtime(&name) {
                        if let (Some(pane), Some(pid)) = (
                            pane.as_str(),
                            job["foreground"]["pid"].as_u64().filter(|pid| *pid > 0),
                        ) {
                            nodes.push((id.to_owned(), pane.to_owned(), pid));
                        }
                        continue;
                    }
                    if !name.is_empty() {
                        commands.insert(id.into(), json!(name));
                    }
                }
            }
            self.probes.retain(|pane, probe| {
                nodes
                    .iter()
                    .any(|(_, p, pid)| p == pane && *pid == probe.pid)
            });
            // Process metadata is optional display information. Never add one SSH
            // round trip per window to the input loop: probe at most one candidate
            // per refresh, preferring unprobed panes (active first), then the oldest.
            let next = nodes
                .iter()
                .filter(|(_, pane, _)| {
                    self.probes
                        .get(pane)
                        .is_none_or(|probe| probe.checked.elapsed() >= Duration::from_secs(2))
                })
                .min_by_key(|(window, pane, _)| {
                    (
                        self.probes.get(pane).map(|probe| probe.checked),
                        frame["window"] != window.as_str(),
                    )
                })
                .cloned();
            if let Some((_, pane, pid)) = next {
                let probe = call("agent.probe", json!({"pane":pane})).ok();
                let codex = probe.is_some_and(|p| {
                    p["adapter"] == "codex"
                        && p["process_detected"] == true
                        && p["foreground_pid"].as_u64() == Some(pid)
                });
                self.probes.insert(
                    pane,
                    Probe {
                        pid,
                        checked: Instant::now(),
                        codex,
                    },
                );
            }
            for (window, pane, pid) in nodes {
                if self
                    .probes
                    .get(&pane)
                    .is_some_and(|probe| probe.pid == pid && probe.codex)
                {
                    commands.insert(window, json!("codex"));
                }
            }
            Some(Value::Object(commands))
        })();
        let Some(commands) = collect else {
            // Older daemons can lack job metadata. Keep ordinary window labels and
            // never make terminal input depend on optional process information.
            return false;
        };
        let changed = frame["_commands"] != commands;
        frame["_commands"] = commands;
        changed
    }
}

#[cfg(test)]
fn refresh(frame: &mut Value, call: impl FnMut(&str, Value) -> io::Result<Value>) -> bool {
    Labels::default().refresh(frame, call)
}

pub fn name(frame: &Value, id: &Value, stored: &Value) -> String {
    let stored = stored.as_str().unwrap_or("shell");
    match frame["_commands"]
        .get(id.as_str().unwrap_or(""))
        .and_then(Value::as_str)
    {
        Some(command) if matches!(stored, "shell" | "Terminal") || node_runtime(stored) => {
            command.into()
        }
        Some(command) if stored.eq_ignore_ascii_case(command) => stored.into(),
        Some(command) => format!("{stored} ({command})"),
        None if node_runtime(stored) => "Terminal".into(),
        None => stored.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn many_node_windows_share_one_probe_budget_and_expire_by_identity() {
        let mut labels = Labels::default();
        let mut frame =
            json!({"session":"session_test","window":"window_5","active_pane":"pane_5"});
        let windows: Vec<_> = (0..12)
            .map(|i| json!({"id":format!("window_{i}"),"active_pane":format!("pane_{i}")}))
            .collect();
        let mut jobs: Vec<_> = (0..12).map(|i| json!({"pane":format!("pane_{i}"),"foreground":{"command":"node-22","pid":100+i}})).collect();
        let mut probed = Vec::new();
        for pass in 0..13 {
            let before = probed.len();
            labels.refresh(&mut frame, |method, params| match method {
                "window.list" => Ok(json!(windows)),
                "job.list" => Ok(json!({"jobs":jobs})),
                "agent.probe" => {
                    let pane = params["pane"].as_str().unwrap().to_owned();
                    let i: u64 = pane.strip_prefix("pane_").unwrap().parse().unwrap();
                    probed.push(pane);
                    Ok(json!({"adapter":"codex","process_detected":true,"foreground_pid":100+i}))
                }
                _ => panic!("Unexpected RPC"),
            });
            assert!(probed.len() - before <= 1);
            if pass == 0 {
                assert_eq!(probed, ["pane_5"]);
                assert_eq!(frame["_commands"]["window_5"], "codex");
            }
        }
        assert_eq!(probed.len(), 12);
        assert_eq!(
            probed
                .iter()
                .collect::<std::collections::HashSet<_>>()
                .len(),
            12
        );
        assert_eq!(labels.probes.len(), 12);

        jobs[5]["foreground"]["pid"] = json!(999);
        labels.refresh(&mut frame, |method, _| match method {
            "window.list" => Ok(json!(windows)),
            "job.list" => Ok(json!({"jobs":jobs})),
            "agent.probe" => Err(io::Error::other("unsupported")),
            _ => panic!("Unexpected RPC"),
        });
        assert!(
            frame["_commands"]["window_5"].is_null(),
            "A new PID inherited another process's label"
        );
        labels.probes.get_mut("pane_0").unwrap().checked = Instant::now() - Duration::from_secs(3);
        labels.refresh(&mut frame, |method, _| match method {
            "window.list" => Ok(json!(windows)),
            "job.list" => Ok(json!({"jobs":jobs})),
            "agent.probe" => Ok(json!({"process_detected":false})),
            _ => panic!("Unexpected RPC"),
        });
        assert!(
            frame["_commands"]["window_0"].is_null(),
            "Exited child retained its label"
        );
        labels.refresh(&mut frame, |method, _| match method {
            "window.list" => Ok(json!(windows)),
            "job.list" => Ok(json!({"jobs":[]})),
            _ => panic!("Ended panes must not be probed"),
        });
        assert!(labels.probes.is_empty());
    }
    fn frame() -> Value {
        json!({"session":"session_test","window":"window_test","active_pane":"pane_test"})
    }
    fn refresh_node(probe: io::Result<Value>) -> Value {
        let mut frame = frame();
        let mut probe = Some(probe);
        refresh(&mut frame, |method, _| match method {
            "window.list" => Ok(json!([{"id":"window_test","active_pane":"pane_test"}])),
            "job.list" => Ok(
                json!({"jobs":[{"pane":"pane_test","foreground":{"command":"node-22","pid":123}}]}),
            ),
            "agent.probe" => probe.take().unwrap(),
            _ => panic!("Unexpected RPC"),
        });
        frame
    }
    #[test]
    fn node_launcher_uses_only_a_matching_positive_process_probe() {
        let frame = refresh_node(Ok(
            json!({"adapter":"codex","process_detected":true,"foreground_pid":123}),
        ));
        assert_eq!(
            name(&frame, &json!("window_test"), &json!("shell")),
            "codex"
        );
        assert_eq!(
            name(&frame, &json!("window_test"), &json!("Codex")),
            "Codex"
        );
        assert_eq!(
            name(&frame, &json!("window_test"), &json!("node-22")),
            "codex"
        );
    }
    #[test]
    fn unknown_node_and_changed_foreground_keep_window_name() {
        for probe in [
            Err(io::Error::other("METHOD_NOT_FOUND")),
            Ok(json!({"adapter":"codex","process_detected":false,"foreground_pid":123})),
            Ok(json!({"adapter":"codex","process_detected":true,"foreground_pid":124})),
            Ok(json!({"adapter":"codex","process_detected":true})),
        ] {
            let frame = refresh_node(probe);
            assert_eq!(
                name(&frame, &json!("window_test"), &json!("My work")),
                "My work"
            );
            assert_eq!(
                name(&frame, &json!("window_test"), &json!("node-22")),
                "Terminal"
            );
        }
    }
    #[test]
    fn ordinary_command_labels_do_not_probe_agents() {
        let mut frame = frame();
        refresh(&mut frame, |method, _| match method {
            "window.list" => Ok(json!([{"id":"window_test","active_pane":"pane_test"}])),
            "job.list" => {
                Ok(json!({"jobs":[{"pane":"pane_test","foreground":{"command":"/usr/bin/btm"}}]}))
            }
            _ => panic!("Ordinary command must not probe an agent"),
        });
        assert_eq!(name(&frame, &json!("window_test"), &json!("shell")), "btm");
        assert_eq!(
            name(&frame, &json!("window_test"), &json!("Monitor")),
            "Monitor (btm)"
        );
        assert!(!node_runtime("node-project"));
    }
}
