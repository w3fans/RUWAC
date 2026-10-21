//! Explicit agent launch in a persistent remote RUWAC session.
use crate::{profiles, sessions, worker::Rpc};
use gtk::prelude::*;
use serde_json::json;
use std::{cell::RefCell, path::PathBuf, rc::Rc, time::Duration};

fn quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}
fn agent_command(agent: u32) -> Result<(&'static str, &'static str), String> {
    match agent {
        0 => Ok(("codex", "codex resume --all")),
        1 => Ok(("claude", "claude --resume")),
        5 => Ok(("kimi", "kimi --session")),
        _ => Err("Choose Codex, Claude or Kimi".into()),
    }
}
fn ssh_command(profile: &profiles::Profile, command: String) -> Result<Vec<String>, String> {
    let mut argv = profiles::ssh_argv(profile)?;
    let position = argv
        .iter()
        .position(|s| s == "--")
        .ok_or("Invalid SSH command")?;
    argv.splice(
        position..position,
        [
            "-tt".into(),
            "-o".into(),
            "ForwardAgent=no".into(),
            "-o".into(),
            "ClearAllForwardings=yes".into(),
        ],
    );
    argv.push(command);
    Ok(argv)
}
pub(crate) fn argv(
    profile: &profiles::Profile,
    agent: u32,
    binary: &str,
    socket: &str,
) -> Result<Vec<String>, String> {
    if profile.protocol != "SSH"
        || (!profile.preferences.gateway.is_empty()
            && profile.preferences.ssh_jump_alias.is_empty())
    {
        return Err("Choose an SSH endpoint with a direct route or an explicit jump alias for the Linux account".into());
    }
    if binary.is_empty()
        || [binary, socket]
            .iter()
            .any(|v| v.len() > 512 || v.chars().any(char::is_control))
    {
        return Err("Invalid remote RUWAC executable or socket".into());
    }
    if agent == 4 {
        // One explicitly chosen key: the confirmation names exactly what is
        // installed (ssh-copy-id would otherwise add every agent identity).
        let key = &profile.preferences.identity_file;
        if key.is_empty() || key.starts_with('-') || key.chars().any(char::is_control) {
            return Err(
                "Choose and save an SSH identity file before authorizing its public key".into(),
            );
        }
        let base = profiles::ssh_argv(profile)?;
        let mut command = vec![
            "ssh-copy-id".into(),
            "-o".into(),
            "ForwardAgent=no".into(),
            "-o".into(),
            "ClearAllForwardings=yes".into(),
        ];
        let mut i = 1;
        while i < base.len() {
            if base[i] == "--" {
                command.push("--".into());
                command.extend_from_slice(&base[i + 1..]);
                break;
            }
            let option = &base[i];
            let value = &base[i + 1];
            match option.as_str() {
                "-i" => command.extend([
                    "-i".into(),
                    if key.ends_with(".pub") {
                        key.clone()
                    } else {
                        format!("{key}.pub")
                    },
                ]),
                "-l" => command.extend(["-o".into(), format!("User={value}")]),
                _ => command.extend([option.clone(), value.clone()]),
            }
            i += 2;
        }
        return Ok(command);
    }
    if agent == 2 {
        return ssh_command(
            profile,
            format!(
                "python3 -c {} --bootstrap --headless --prebuilt",
                quote(include_str!("../../../scripts/update-user.py"))
            ),
        );
    }
    let (name, default) = agent_command(agent)?;
    let configured = if agent == 0 {
        &profile.preferences.codex_command
    } else if agent == 1 {
        &profile.preferences.claude_command
    } else {
        &profile.preferences.kimi_command
    };
    let command = if configured.is_empty() {
        default
    } else {
        configured.as_str()
    };
    if command.len() > 1024 || command.chars().any(char::is_control) {
        return Err(
            "Invalid agent command; keep credentials in the remote agent's own login store".into(),
        );
    }
    let installed;
    let command = if profile.preferences.agent_install_missing {
        installed = format!(
            "python3 -c {} {} {}",
            quote(include_str!("../../../scripts/agent-bootstrap.py")),
            quote(name),
            quote(command)
        );
        installed.as_str()
    } else {
        command
    };
    match profile.preferences.agent_backend.as_str() {
        "" | "ruwac" => (),
        "tmux" => {
            return ssh_command(
                profile,
                format!(
                    "exec tmux new-session -A -s {} bash -lic {}",
                    quote(name),
                    quote(command)
                ),
            );
        }
        "screen" => {
            return ssh_command(
                profile,
                format!(
                    "exec screen -xRR -S {} bash -lic {}",
                    quote(name),
                    quote(command)
                ),
            );
        }
        "direct" => return ssh_command(profile, format!("exec bash -lic {}", quote(command))),
        _ => return Err("Unknown remote session backend".into()),
    }
    let script = format!(
        "python3 -c {} {} {} {} {} {}",
        quote(include_str!("../../../scripts/remote-agent.py")),
        quote(binary),
        quote(socket),
        quote(name),
        quote(command),
        quote(&profile.preferences.agent_session)
    );
    // Optional: install the RUWAC CLI first (prebuilt, per user) when missing.
    let script = if profile.preferences.ruwac_install_missing {
        format!(
            "{{ command -v {} >/dev/null 2>&1 || test -x \"$HOME/.local/bin/ruwac\" || {{ echo 'Installing RUWAC…'; python3 -c {} --bootstrap --headless --prebuilt; }}; }} && {}",
            quote(binary),
            quote(include_str!("../../../scripts/update-user.py")),
            script
        )
    } else {
        script
    };
    ssh_command(profile, format!("bash -lc {}", quote(&script)))
}

/// Called only by explicit machine actions. No remote probe or agent starts on list refresh.
pub fn launch(
    parent: &impl IsA<gtk::Window>,
    profile: profiles::Profile,
    kind: u32,
    tabs: &adw::TabView,
    views: &Rc<RefCell<Vec<Rc<crate::view::View>>>>,
    socket: &PathBuf,
    broker: &crate::clipboard::Broker,
) {
    if kind == 2 || kind == 4 {
        let d = gtk::Dialog::builder()
            .title(if kind == 4 {
                "Authorize SSH public key"
            } else {
                "Install remote RUWAC CLI"
            })
            .transient_for(parent)
            .modal(true)
            .build();
        let text = if kind == 4 {
            format!(
                "Install your selected public key for {} at {} using SSH authentication. Verify the host key when prompted. The private key stays on this computer. This grants access for that key to the remote account.",
                profile.username, profile.server
            )
        } else {
            format!(
                "Install/update the headless CLI for {} at {} using its saved SSH account. The existing verified source installer runs in a terminal, needs Python 3 and Rust build tools, and preserves sessions. No agent installation or login is performed.",
                profile.name, profile.server
            )
        };
        let info = gtk::Label::new(Some(&text));
        info.set_wrap(true);
        info.set_max_width_chars(64);
        d.content_area().append(&info);
        d.add_button("Cancel", gtk::ResponseType::Cancel);
        d.add_button(
            if kind == 4 {
                "Authorize public key"
            } else {
                "Install"
            },
            gtk::ResponseType::Accept,
        );
        let (t, v, s, b) = (tabs.clone(), views.clone(), socket.clone(), broker.clone());
        d.connect_response(move |d, r| {
            if r == gtk::ResponseType::Accept {
                open_terminal(profile.clone(), kind, &t, &v, &s, &b);
            }
            d.close();
        });
        d.present();
    } else {
        open_terminal(profile, kind, tabs, views, socket, broker);
    }
}
fn open_terminal(
    profile: profiles::Profile,
    kind: u32,
    tabs: &adw::TabView,
    views: &Rc<RefCell<Vec<Rc<crate::view::View>>>>,
    socket: &PathBuf,
    broker: &crate::clipboard::Broker,
) {
    let title = format!(
        "{} · {}",
        profile.name,
        if kind == 0 {
            "Codex"
        } else if kind == 1 {
            "Claude"
        } else if kind == 5 {
            "Kimi"
        } else if kind == 4 {
            "SSH authorization"
        } else {
            "RUWAC setup"
        }
    );
    let planned = argv(&profile, kind, "ruwac", "").map(|a| profiles::session_argv(&profile, a));
    let launch_argv = planned.clone().unwrap_or_default();
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    let sock = socket.clone();
    let name = title.clone();
    std::thread::spawn(move || {
        let result = (|| -> Result<serde_json::Value, String> {
            let argv = planned?;
            let mut rpc = Rpc::connect(&sock).map_err(|_| "Local daemon unavailable")?;
            rpc.call("session.create", json!({"name":name,"argv":argv}))
                .map_err(|_| "Unable to create SSH session".into())
        })();
        let _ = tx.send(result);
    });
    let (t, v, s, b) = (tabs.clone(), views.clone(), socket.clone(), broker.clone());
    let target = profile.clone();
    gtk::glib::timeout_add_local(Duration::from_millis(20), move || match rx.try_recv() {
        Ok(Ok(r)) => {
            if let Some(id) = r["session"].as_str() {
                if let Ok(recipe) = crate::launch_context::Recipe::agent(&target, kind, "ruwac", "")
                {
                    crate::launches::remember_remote(&r, &target, recipe, &title, &launch_argv);
                } else {
                    crate::launches::remember(id, &title, &launch_argv);
                }
                sessions::append(&t, &v, &s, &b, id, &title);
            }
            gtk::glib::ControlFlow::Break
        }
        Ok(Err(error)) => {
            let d = gtk::Window::builder()
                .title("Remote terminal unavailable")
                .default_width(420)
                .child(&gtk::Label::new(Some(&error)))
                .build();
            d.present();
            gtk::glib::ControlFlow::Break
        }
        Err(std::sync::mpsc::TryRecvError::Empty) => gtk::glib::ControlFlow::Continue,
        Err(_) => gtk::glib::ControlFlow::Break,
    });
}

pub fn button(
    parent: &impl IsA<gtk::Window>,
    inventory: profiles::InventoryReader,
    reader: profiles::ImageProfileReader,
    tabs: adw::TabView,
    views: Rc<RefCell<Vec<Rc<crate::view::View>>>>,
    local_socket: PathBuf,
    broker: crate::clipboard::Broker,
) -> gtk::Button {
    let button = gtk::Button::with_label("Open remote agent…");
    let parent = parent.as_ref().downgrade();
    button.connect_clicked(move |_| {
        let Some(parent) = parent.upgrade() else { return; };
        let dialog = gtk::Dialog::builder().title("Open remote agent").transient_for(&parent).modal(true).default_width(480).build();
        dialog.add_button("Cancel", gtk::ResponseType::Cancel);
        let open = dialog.add_button("Open", gtk::ResponseType::Accept);
        let body = gtk::Box::builder().orientation(gtk::Orientation::Vertical).spacing(8).margin_start(16).margin_end(16).margin_top(16).margin_bottom(16).build();
        let rows = inventory().unwrap_or_default().into_iter().filter(|r| r["protocol"] == "SSH" && r["configuration_valid"] == true).collect::<Vec<_>>();
        let names = rows.iter().map(|r| format!("{} / {} · {}",r["group"].as_str().unwrap_or(""),r["name"].as_str().unwrap_or(""),r["server"].as_str().unwrap_or(""))).collect::<Vec<_>>();
        let endpoint = gtk::DropDown::from_strings(&names.iter().map(String::as_str).collect::<Vec<_>>());
        endpoint.set_widget_name("agent-launch-profile");
        let agent = gtk::DropDown::from_strings(&["Codex (cx)", "Claude Code (cc)", "Kimi"]);
        agent.set_widget_name("agent-launch-agent");
        let backend=gtk::DropDown::from_strings(&["RUWAC","GNU Screen","tmux","Direct"]);backend.set_widget_name("agent-launch-backend");
        let command=gtk::Entry::builder().text("codex resume --all").build();command.set_widget_name("agent-launch-command");let cmd=command.clone();agent.connect_selected_notify(move|a|cmd.set_text(match a.selected(){0=>"codex resume --all",1=>"claude --resume",_=>"kimi --session"}));
        body.append(&gtk::Label::new(Some("Remote session backend")));body.append(&backend);body.append(&gtk::Label::new(Some("Command (no passwords)")));body.append(&command);
        let binary = gtk::Entry::builder().text("ruwac").build();
        binary.set_widget_name("agent-launch-binary");
        let socket = gtk::Entry::builder().placeholder_text("Leave empty for the remote user's default").build();
        socket.set_widget_name("agent-launch-socket");
        for (label, widget) in [("SSH machine / Linux account",endpoint.upcast_ref::<gtk::Widget>()),("Agent",agent.upcast_ref()),("Remote RUWAC executable",binary.upcast_ref()),("Remote daemon socket",socket.upcast_ref())] {
            let l=gtk::Label::new(Some(label));l.set_xalign(0.0);body.append(&l);body.append(widget);
        }
        let status=gtk::Label::new(Some(if rows.is_empty(){"Add an SSH endpoint first. RDP service credentials are separate from the Linux SSH account."}else{"Starts or reuses the remote RUWAC daemon and opens a persistent agent session. Requires RUWAC, Python 3 and the agent on the remote host. SSH uses your key or prompts in the terminal; no password is embedded in the command."}));status.set_wrap(true);body.append(&status);
        open.set_sensitive(!rows.is_empty());dialog.content_area().append(&body);
        let (tabs,views,local_socket,broker,reader)=(tabs.clone(),views.clone(),local_socket.clone(),broker.clone(),reader.clone());
        dialog.connect_response(move |d,response| {
            if response!=gtk::ResponseType::Accept { d.close();return; }
            let plan=rows.get(endpoint.selected() as usize).and_then(|r|r["id"].as_str()).ok_or("Choose an SSH endpoint".to_string()).and_then(|id|reader(id)).and_then(|mut p|{p.preferences.agent_backend=["ruwac","screen","tmux","direct"][backend.selected()as usize].into();if agent.selected()==0{p.preferences.codex_command=command.text().into();}else if agent.selected()==1{p.preferences.claude_command=command.text().into();}else{p.preferences.kimi_command=command.text().into();}{let kind=[0,1,5][agent.selected()as usize];let recipe=crate::launch_context::Recipe::agent(&p,kind,&binary.text(),&socket.text()).map_err(|e|e.to_string())?;let argv=argv(&p,kind,&binary.text(),&socket.text())?;Ok((p.clone(),profiles::session_argv(&p,argv),recipe))}});
            let (profile,argv,recipe)=match plan {Ok(a)=>a,Err(e)=>{status.set_text(&e);return;}};
            open.set_sensitive(false);status.set_text("Opening remote agent terminal…");
            let (tx,rx)=std::sync::mpsc::sync_channel(1);let socket=local_socket.clone();
            let launch_argv=argv.clone();
            std::thread::spawn(move || {let result=(||{let mut rpc=Rpc::connect(&socket)?;rpc.call("session.create",json!({"name":"Remote agent","argv":argv}))})();let _=tx.send(result);});
            let (tabs,views,socket,broker,dialog,status,open)=(tabs.clone(),views.clone(),local_socket.clone(),broker.clone(),d.downgrade(),status.clone(),open.clone());
            gtk::glib::timeout_add_local(Duration::from_millis(20),move ||match rx.try_recv(){
                Ok(Ok(r))=>{if let Some(id)=r["session"].as_str(){crate::launches::remember_remote(&r,&profile,recipe.clone(),"Remote agent",&launch_argv);sessions::append(&tabs,&views,&socket,&broker,id,"Remote agent");}if let Some(d)=dialog.upgrade(){d.close();}gtk::glib::ControlFlow::Break},
                Ok(Err(_))|Err(std::sync::mpsc::TryRecvError::Disconnected)=>{status.set_text("Could not create the local SSH terminal");open.set_sensitive(true);gtk::glib::ControlFlow::Break},
                Err(std::sync::mpsc::TryRecvError::Empty)=>gtk::glib::ControlFlow::Continue,
            });
        });
        dialog.present();
    });
    button
}

pub fn fixture(q: &serde_json::Value) -> serde_json::Value {
    let mut visible = false;
    let mut profiles = 0;
    for w in gtk::Window::list_toplevels() {
        if let Some(d) = w.downcast_ref::<gtk::Dialog>() {
            if d.title().as_deref() != Some("Open remote agent") {
                continue;
            }
            visible = d.is_visible();
            let mut child = d.content_area().first_child().and_then(|w| w.first_child());
            while let Some(w) = child {
                child = w.next_sibling();
                if w.widget_name() == "agent-launch-profile" {
                    profiles = w
                        .downcast_ref::<gtk::DropDown>()
                        .and_then(|d| d.model())
                        .map(|m| m.n_items())
                        .unwrap_or(0);
                }
            }
            if q["operation"] == "cancel" {
                d.response(gtk::ResponseType::Cancel);
            }
        }
    }
    json!({"dialog_visible":visible,"SSH_profiles":profiles,"agents":["Codex (cx)","Claude Code (cc)","Kimi"],"starts_agent_on_cancel":false})
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, os::unix::fs::PermissionsExt, process::Command};
    #[test]
    fn public_key_authorization_uses_saved_target_without_password_arguments() {
        let profile: profiles::Profile = serde_json::from_value(json!({"id":"endpoint_demo","machine_id":"machine_demo","name":"Demo","group":"Test","protocol":"SSH","server":"127.0.0.1:2222","username":"generated-user","secret_reference":"GENERATED-SECRET-REFERENCE","favorite":false,"last_used":0,"tags":[],"preferences":{"identity_file":"/tmp/generated key'file","known_hosts":"/tmp/generated hosts"}})).unwrap();
        let args = argv(&profile, 4, "ruwac", "").unwrap();
        assert_eq!(args[0], "ssh-copy-id");
        assert!(
            args.windows(2)
                .any(|pair| pair == ["-i", "/tmp/generated key'file.pub"])
        );
        assert!(args.windows(2).any(|pair| pair == ["-p", "2222"]));
        assert!(
            args.windows(2)
                .any(|pair| pair == ["-o", "User=generated-user"])
        );
        assert!(
            args.windows(2)
                .any(|pair| pair == ["-o", "StrictHostKeyChecking=ask"])
        );
        assert_eq!(&args[args.len() - 2..], &["--", "127.0.0.1"]);
        assert!(!args.join(" ").contains("GENERATED-SECRET-REFERENCE"));
        let mut missing = profile.clone();
        missing.preferences.identity_file.clear();
        assert!(argv(&missing, 4, "ruwac", "").is_err());
    }

    #[test]
    fn missing_ruwac_is_installed_first_only_when_enabled() {
        let mut profile: profiles::Profile = serde_json::from_value(json!({"id":"endpoint_demo","machine_id":"machine_demo","name":"Demo","group":"Test","protocol":"SSH","server":"127.0.0.1:2222","username":"generated-user","secret_reference":"GENERATED-SECRET-REF","favorite":false,"last_used":0,"tags":[]})).unwrap();
        let plain = argv(&profile, 0, "ruwac", "").unwrap();
        assert!(!plain.last().unwrap().contains("--prebuilt"));
        profile.preferences.ruwac_install_missing = true;
        let install = argv(&profile, 0, "ruwac", "").unwrap();
        let command = install.last().unwrap();
        assert!(command.contains("--bootstrap --headless --prebuilt"));
        assert!(command.contains("command -v"));
    }
    #[test]
    fn defaults_open_resume_pickers_without_a_prompt() {
        assert_eq!(agent_command(0).unwrap(), ("codex", "codex resume --all"));
        assert_eq!(agent_command(1).unwrap(), ("claude", "claude --resume"));
        assert_eq!(agent_command(5).unwrap(), ("kimi", "kimi --session"));
    }

    #[test]
    fn bootstrap_survives_shell_quoting_and_detaches_without_agent_input() {
        let root = std::env::temp_dir().join(ruwac_protocol::id("launch"));
        fs::create_dir(&root).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        let binary = root.join("remote ' executable");
        let receipt = root.join("receipt.json");
        // A disposable remote CLI process verifies the real shell/Python
        // boundary. It never starts Codex, Claude or an owner daemon.
        fs::write(&binary,format!("#!/usr/bin/python3\nimport json,sys\nfrom pathlib import Path\na=sys.argv\nif 'session.list' in a:\n print(json.dumps({{'result':[]}}))\nif 'session.create' in a:\n Path({:?}).write_text(a[a.index('--params')+1])\n print(json.dumps({{'result':{{'session':'session_generated'}}}}))\n",receipt.to_str().unwrap())).unwrap();
        fs::set_permissions(&binary, fs::Permissions::from_mode(0o700)).unwrap();
        let p = profiles::Profile {
            id: "endpoint_generated".into(),
            machine_id: "machine_generated".into(),
            name: "Generated".into(),
            group: String::new(),
            protocol: "SSH".into(),
            server: "127.0.0.1:22".into(),
            username: "fixture".into(),
            secret_reference: "private-reference-never-in-argv".into(),
            favorite: false,
            last_used: 0,
            tags: vec![],
            preferences: Default::default(),
            inherit: false,
            inherit_credential: false,
            inherit_connection: false,
        };
        for agent in [0, 1] {
            let args = argv(
                &p,
                agent,
                binary.to_str().unwrap(),
                root.join("socket ' quote").to_str().unwrap(),
            )
            .unwrap();
            assert!(!args.join(" ").contains(&p.secret_reference));
            let output = Command::new("bash")
                .arg("-c")
                .arg(args.last().unwrap())
                .output()
                .unwrap();
            assert!(output.status.success(), "bootstrap process failed");
            let params: serde_json::Value =
                serde_json::from_slice(&fs::read(&receipt).unwrap()).unwrap();
            assert_eq!(params["argv"][0], "bash");
            assert_eq!(params["argv"][1], "-lic");
            assert_eq!(params["name"], if agent == 0 { "codex" } else { "claude" });
            assert!(String::from_utf8_lossy(&output.stdout).contains("Closing this view detaches"));
        }
        let mut custom = p.clone();
        custom.preferences.codex_command = "printf '%s' 'generated command with spaces'".into();
        for backend in ["screen", "tmux", "direct"] {
            custom.preferences.agent_backend = backend.into();
            let args = argv(&custom, 0, "ruwac", "").unwrap();
            assert!(!args.join(" ").contains(&custom.secret_reference));
            if backend == "direct" {
                let output = Command::new("bash")
                    .args(["-c", args.last().unwrap()])
                    .output()
                    .unwrap();
                assert!(output.status.success());
                assert_eq!(
                    String::from_utf8_lossy(&output.stdout),
                    "generated command with spaces"
                );
            } else {
                let script = &args[args.len() - 1];
                assert!(script.starts_with(&format!("exec {backend} ")));
                assert!(script.contains("codex"));
                assert!(script.contains("bash -lic"));
            }
        }
        custom.preferences.agent_backend = "unknown".into();
        assert!(argv(&custom, 0, "ruwac", "").is_err());
        let install = argv(&p, 2, "ruwac", "").unwrap();
        assert!(
            install
                .last()
                .unwrap()
                .ends_with("--bootstrap --headless --prebuilt")
        );
        assert!(!install.join(" ").contains(&p.secret_reference));
        let mut rdp = p.clone();
        rdp.protocol = "RDP".into();
        assert!(argv(&rdp, 0, "ruwac", "").is_err());
        fs::remove_dir_all(root).unwrap();
    }
}
