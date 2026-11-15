//! Generated forwarding fixture driver; not an installed user command.
#[path = "../src/ssh_transport.rs"]
mod transport;
use std::io::{Read, Write};
fn main() {
    if let Some(status) = transport::run_forward_worker_if_requested() {
        std::process::exit(status);
    }
    let args = std::env::args().collect::<Vec<_>>();
    match args.get(1).map(String::as_str) {
        Some("prepare") => {
            let mut text = String::new();
            std::io::stdin().read_to_string(&mut text).unwrap();
            let args: Vec<String> = serde_json::from_str(&text).unwrap();
            let args = transport::prepare(&args).unwrap();
            println!(
                "{}",
                serde_json::json!({"token":transport::identity(&args),"argv":args})
            );
        }
        Some("open") => {
            let result = transport::LocalForward::open(
                &args[2],
                args[3].parse().unwrap(),
                &args[4],
                args[5].parse().unwrap(),
            );
            match result {
                Ok(forward) => {
                    println!(
                        "{}",
                        serde_json::json!({"ok":true,"port":forward.local_port,"valid":forward.valid()})
                    );
                    std::io::stdout().flush().unwrap();
                    let mut close = [0];
                    let _ = std::io::stdin().read(&mut close);
                    drop(forward);
                }
                Err(_) => {
                    println!("{}", serde_json::json!({"ok":false}));
                    std::process::exit(1);
                }
            }
        }
        _ => std::process::exit(2),
    }
}
