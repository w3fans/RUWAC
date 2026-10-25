//! Explicit fixture driver for the production SFTP and Secret Service adapters.
#[path = "../src/askpass.rs"]
mod askpass;
#[path = "../src/credentials.rs"]
mod credentials;
#[path = "../src/files.rs"]
mod files;
#[path = "../src/files_resume.rs"]
mod files_resume;
#[path = "../src/files_rsync.rs"]
mod files_rsync;
#[path = "../src/primary.rs"]
mod primary;
#[path = "../src/profiles.rs"]
mod profiles;
#[path = "../src/rdp_trust.rs"]
mod rdp_trust;
#[path = "../src/remote_paste_upload.rs"]
mod remote_paste_upload;
#[path = "../src/sidebar_shell.rs"]
mod sidebar_shell;
#[path = "../src/ssh_jump.rs"]
mod ssh_jump;
#[path = "../src/ssh_keys.rs"]
mod ssh_keys;
#[path = "../src/ssh_probe.rs"]
mod ssh_probe;
#[path = "../src/ssh_transport.rs"]
mod ssh_transport;
#[path = "../src/terminal_primary.rs"]
mod terminal_primary;
use std::{
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64},
    },
    time::Duration,
};
fn main() {
    if let Some(status) = askpass::run_if_requested() {
        std::process::exit(status);
    }
    let args: Vec<_> = std::env::args().collect();
    let result = (|| -> Result<serde_json::Value, String> {
        if args.get(1).map(String::as_str) == Some("remmina-credentials") {
            let path = std::path::Path::new(args.get(2).ok_or("Remmina fixture required")?);
            let reference = ruwac_protocol::id("fixture-remmina");
            let secret = credentials::lookup_remmina(path)?;
            let matched = secret == "generated-remmina-password";
            credentials::set(&reference, secret)?;
            let copied = credentials::lookup(&reference)? == "generated-remmina-password";
            credentials::forget(&reference)?;
            return Ok(
                serde_json::json!({"matched":matched,"copied":copied,"forgotten":credentials::lookup(&reference).is_err()}),
            );
        }
        if args.get(1).map(String::as_str) == Some("credentials") {
            let reference = ruwac_protocol::id("fixture-secret");
            credentials::set(&reference, "synthetic-keyring-fixture-only".into())?;
            let matched = credentials::lookup(&reference)? == "synthetic-keyring-fixture-only";
            credentials::set(&reference, "synthetic-replacement-only".into())?;
            let replaced = credentials::lookup(&reference)? == "synthetic-replacement-only";
            credentials::forget(&reference)?;
            let forgotten = credentials::lookup(&reference).is_err();
            return Ok(
                serde_json::json!({"matched":matched,"replaced":replaced,"forgotten":forgotten,"provider":"real Secret Service via secret-tool"}),
            );
        }
        let p: profiles::Profile = serde_json::from_slice(
            &std::fs::read(args.get(2).ok_or("profile required")?)
                .map_err(|_| "cannot read fixture profile")?,
        )
        .map_err(|_| "invalid fixture profile")?;
        p.validate()?;
        if args[1] == "primary-flow" {
            // Exercise the actual saved-route Bridge without substituting a
            // second transport or dispatching any desktop input.
            let root =
                std::path::PathBuf::from(args.get(3).ok_or("Private flow fixture required")?);
            let mut bridge = primary::Bridge::start(&p)?;
            let mut sequence = 0u64;
            let mut active = true;
            let mut preparing = None;
            let mut committing = None;
            let mut prepared = serde_json::Value::Null;
            let mut committed = serde_json::Value::Null;
            let mut response = serde_json::Value::Null;
            let started = std::time::Instant::now();
            while started.elapsed() < Duration::from_secs(120) {
                bridge.tick(active, false);
                let command = std::fs::read(root.join("flow-command.json"))
                    .ok()
                    .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
                    .unwrap_or_default();
                if let Some(next) = command["seq"].as_u64().filter(|next| *next > sequence) {
                    sequence = next;
                    let mut ok = false;
                    let mut id = None;
                    match command["action"].as_str() {
                        Some("close") => return Ok(serde_json::json!({"closed":true})),
                        Some("mode") => {
                            if let Some(enabled) = command["active"].as_bool() {
                                active = enabled;
                                bridge.tick(active, false);
                                preparing = None;
                                committing = None;
                                prepared = serde_json::Value::Null;
                                committed = serde_json::Value::Null;
                                ok = true;
                            }
                        }
                        Some("authorize") => ok = bridge.authorize().is_ok(),
                        Some("prepare") => {
                            if let Some(text) = command["text"].as_str() {
                                if let Ok(wanted) = bridge.offer_primary(text.to_owned()) {
                                    preparing = Some(wanted);
                                    id = Some(wanted);
                                    prepared = serde_json::Value::Null;
                                    ok = true;
                                }
                            }
                        }
                        Some("commit") => {
                            if let Some(wanted) = command["id"].as_u64() {
                                ok = bridge.commit_primary(wanted).is_ok();
                                if ok {
                                    committing = Some(wanted);
                                    committed = serde_json::Value::Null;
                                }
                            }
                        }
                        Some("cancel") => {
                            if let Some(wanted) = command["id"].as_u64() {
                                bridge.cancel_primary(wanted);
                                preparing = None;
                                committing = None;
                                ok = true;
                            }
                        }
                        _ => {}
                    }
                    response = serde_json::json!({"seq":sequence,"ok":ok,"id":id});
                }
                if let Some(id) = preparing {
                    if let Some(ok) = bridge.take_prepare_result(id) {
                        prepared = serde_json::json!({"id":id,"ok":ok});
                        preparing = None;
                    }
                }
                if let Some(id) = committing {
                    if let Some(ok) = bridge.take_offer_result(id) {
                        committed = serde_json::json!({"id":id,"ok":ok});
                        committing = None;
                    }
                }
                let value = serde_json::json!({"ready":bridge.ready,"failed":bridge.failed,
                    "native":bridge.native(),"observes":bridge.observes(),"active":active,
                    "authorized":bridge.authorized(),"authorizing":bridge.authorizing(),
                    "response":response,"prepared":prepared,"committed":committed});
                let temporary = root.join("flow-state.tmp");
                std::fs::write(&temporary, serde_json::to_vec(&value).unwrap())
                    .map_err(|_| "Cannot write flow state")?;
                std::fs::rename(temporary, root.join("flow-state.json"))
                    .map_err(|_| "Cannot publish flow state")?;
                std::thread::sleep(Duration::from_millis(10));
            }
            return Err("Private flow fixture timed out".into());
        }
        if args[1] == "primary-receive" {
            use gtk::prelude::*;
            let root =
                std::path::PathBuf::from(args.get(3).ok_or("Private receiver fixture required")?);
            let app = gtk::Application::builder()
                .application_id("si.z4.ruwac.PrimaryFixture")
                .flags(gtk::gio::ApplicationFlags::NON_UNIQUE)
                .build();
            app.connect_activate(move|app| {
                let entry=gtk::Entry::new();entry.set_placeholder_text(Some("Focused receiver fixture"));
                let window=gtk::ApplicationWindow::builder().application(app).title("RUWAC PRIMARY receiver fixture").default_width(650).default_height(260).child(&entry).build();
                window.maximize();window.present();entry.grab_focus();
                let bridge=std::rc::Rc::new(std::cell::RefCell::new(primary::Bridge::start(&p).unwrap()));
                let root=root.clone();let app=app.clone();let profile=p.clone();let mut generation=0u64;let mut ticks=0u64;let mut received=0u64;let mut received_bytes=0usize;
                gtk::glib::timeout_add_local(Duration::from_millis(25),move|| {
                    let command=std::fs::read(root.join("receiver-command.json")).ok().and_then(|b|serde_json::from_slice::<serde_json::Value>(&b).ok()).unwrap_or_default();
                    if command["close"]==true {app.quit();return gtk::glib::ControlFlow::Break;}
                    if command["present"]==true {window.present();entry.grab_focus();}
                    let active=window.is_active()&&command["enabled"]!=false;
                    let mut b=bridge.borrow_mut();
                    if let Some(wanted)=command["restart"].as_u64() {if wanted>generation {*b=primary::Bridge::start(&profile).unwrap();generation=wanted;}}
                    b.tick(false,active);ticks+=1;
                    if active {if let Ok(Some(bytes))=b.publish_primary(&entry.display().primary_clipboard()) {received+=1;received_bytes=bytes;}}
                    let value=serde_json::json!({"ready":b.ready,"failed":b.failed,"active":active,"received":received,"received_bytes":received_bytes,"generation":generation,"ticks":ticks});
                    let temp=root.join("receiver-state.tmp");std::fs::write(&temp,serde_json::to_vec(&value).unwrap()).unwrap();std::fs::rename(temp,root.join("receiver-state.json")).unwrap();
                    gtk::glib::ControlFlow::Continue
                });
            });
            app.run_with_args::<&str>(&[]);
            return Ok(serde_json::json!({"closed":true}));
        }

        if args[1] == "ssh-image-paste" || args[1] == "ssh-image-paste-original" {
            let bytes = std::fs::read(args.get(3).ok_or("PNG input required")?)
                .map_err(|_| "PNG input unavailable")?;
            let allowed = Arc::new(AtomicBool::new(true));
            let original = args[1] == "ssh-image-paste-original";
            if let Some(ms) = args
                .get(if original { 6 } else { 4 })
                .and_then(|v| v.parse::<u64>().ok())
            {
                let ticket = allowed.clone();
                std::thread::spawn(move || {
                    std::thread::sleep(Duration::from_millis(ms));
                    ticket.store(false, std::sync::atomic::Ordering::Release);
                });
            }
            let result = if original {
                let transport = args.get(4).ok_or("Transport identity required")?;
                let pid = args
                    .get(5)
                    .ok_or("Original SSH process required")?
                    .parse::<u32>()
                    .map_err(|_| "Invalid original SSH process")?;
                remote_paste_upload::upload_original(
                    &p,
                    &bytes,
                    &allowed,
                    Duration::from_secs(10),
                    transport,
                    pid,
                )
            } else {
                remote_paste_upload::upload(&p, &bytes, &allowed, Duration::from_secs(10))
            };
            return result.map(|path| serde_json::json!({"path":path}));
        }
        if args[1] == "ssh-protected-channel-argv" {
            let token = args.get(3).ok_or("Transport identity required")?;
            let pid = args
                .get(4)
                .ok_or("Original SSH process required")?
                .parse::<u32>()
                .map_err(|_| "Invalid original SSH process")?;
            let argv = ssh_transport::client(token, pid).map_err(|e| e.to_string())?;
            return Ok(serde_json::json!({"argv":argv}));
        }
        if args[1] == "ssh-protected-argv" {
            let argv = profiles::session_argv(&p, profiles::ssh_argv(&p)?);
            return Ok(serde_json::json!({"transport":ssh_transport::identity(&argv),"argv":argv}));
        }
        if args[1] == "ssh-argv" {
            return Ok(serde_json::json!({"argv": profiles::ssh_argv(&p)?}));
        }
        if args[1] == "ssh-access-plan" {
            let plan = ssh_probe::plan(&p, None, &AtomicBool::new(true))?;
            return Ok(serde_json::json!({"argv": ssh_probe::probe_argv(&plan.argv, &[])}));
        }
        if args[1] == "ssh-access" {
            let keys = ssh_probe::identities();
            let outcome = ssh_probe::probe(&p, &keys);
            let snapshot = ssh_probe::inspect(&p, &keys, &AtomicBool::new(true));
            return Ok(serde_json::json!({
                "access": outcome.access,
                "accepted_key": outcome.accepted_key,
                "description": outcome.describe(),
                "inputs": snapshot.as_ref().map(|(hash,_)| hash.clone()).unwrap_or_default(),
                "target": snapshot.map(|(_,target)| target),
                "identities": keys.iter().map(|k| k.fingerprint.clone()).collect::<Vec<_>>(),
            }));
        }
        if args[1] == "rsync-pending" {
            return serde_json::to_value(files_rsync::pending(&p)?)
                .map_err(|_| "Cannot encode recovery jobs".into());
        }
        if args[1] == "rsync-recover" {
            return files_rsync::recover(&p, &args[3], &AtomicBool::new(false))
                .map(|s| serde_json::json!({"status":s}));
        }
        if matches!(args[1].as_str(), "rsync-upload" | "rsync-download") {
            let cancel = Arc::new(AtomicBool::new(false));
            let progress = Arc::new(AtomicU64::new(0));
            if let Some(threshold) = args.get(6).and_then(|s| s.parse::<u64>().ok()) {
                let (c, b) = (cancel.clone(), progress.clone());
                std::thread::spawn(move || {
                    while b.load(std::sync::atomic::Ordering::Relaxed) < threshold {
                        std::thread::sleep(Duration::from_millis(1));
                    }
                    c.store(true, std::sync::atomic::Ordering::Relaxed);
                });
            }
            let observe_phase = args.get(7).cloned();
            return serde_json::to_value(files_rsync::transfer_observed(
                &p,
                Path::new(&args[3]),
                Path::new(&args[4]),
                args[1] == "rsync-upload",
                args.get(5).map(String::as_str) == Some("overwrite"),
                &cancel,
                progress,
                move |id, phase| {
                    if observe_phase.as_deref() == Some(phase) {
                        use std::io::Write;
                        println!("{}", serde_json::json!({"checkpoint":phase,"id":id}));
                        std::io::stdout()
                            .flush()
                            .map_err(|_| "Fixture output unavailable")?;
                        unsafe {
                            libc::raise(libc::SIGSTOP);
                        }
                    }
                    Ok(())
                },
            )?)
            .map_err(|_| "Cannot encode fixture receipt".into());
        }
        if args[1] == "list" {
            let entries = files::list(&p, Path::new(&args[3]))?;
            return Ok(
                serde_json::json!({"entries":entries.iter().map(|e|&e.name).collect::<Vec<_>>()}),
            );
        }
        if args[1] == "pending" {
            return serde_json::to_value(files_resume::list(&p)?).map_err(|e| e.to_string());
        }
        if args[1] == "discard" {
            files_resume::discard(&p, &args[3], &AtomicBool::new(false))?;
            return Ok(serde_json::json!({"discarded":true}));
        }
        if matches!(
            args[1].as_str(),
            "resume-upload" | "resume-download" | "resume"
        ) {
            let cancel = Arc::new(AtomicBool::new(false));
            let progress = Arc::new(AtomicU64::new(0));
            if let Some(threshold) = args
                .get(if args[1] == "resume" { 4 } else { 6 })
                .and_then(|s| s.parse::<u64>().ok())
            {
                let (c, b) = (cancel.clone(), progress.clone());
                std::thread::spawn(move || {
                    while b.load(std::sync::atomic::Ordering::Relaxed) < threshold {
                        std::thread::sleep(Duration::from_millis(1));
                    }
                    c.store(true, std::sync::atomic::Ordering::Relaxed);
                });
            }
            let receipt = if args[1] == "resume" {
                files_resume::resume(&p, &args[3], &cancel, &progress)
            } else {
                files_resume::start(
                    &p,
                    Path::new(&args[3]),
                    Path::new(&args[4]),
                    args[1] == "resume-upload",
                    args.get(5).map(String::as_str) == Some("overwrite"),
                    &cancel,
                    &progress,
                )
            }?;
            return serde_json::to_value(receipt).map_err(|e| e.to_string());
        }
        let cancel = Arc::new(AtomicBool::new(false));
        if let Some(ms) = args.get(6).and_then(|s| s.parse::<u64>().ok()) {
            let c = cancel.clone();
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(ms));
                c.store(true, std::sync::atomic::Ordering::Relaxed);
            });
        }
        let progress = AtomicU64::new(0);
        let start = std::time::Instant::now();
        let hash = files::transfer(
            &p,
            Path::new(&args[3]),
            Path::new(&args[4]),
            args[1] == "upload",
            args.get(5).map(String::as_str) == Some("overwrite"),
            &cancel,
            &progress,
        )?;
        Ok(
            serde_json::json!({"sha256":hash,"bytes":progress.load(std::sync::atomic::Ordering::Relaxed),"elapsed_ms":start.elapsed().as_millis()}),
        )
    })();
    println!(
        "{}",
        match result {
            Ok(v) => serde_json::json!({"ok":true,"result":v}),
            Err(e) => serde_json::json!({"ok":false,"error":e}),
        }
    );
}
