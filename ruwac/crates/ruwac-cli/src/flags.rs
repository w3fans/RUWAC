//! Short option spellings. Values remain opaque, including strings beginning with '-'.
pub const GLOBAL: &[(&str, &str, usize)] = &[
    ("-s", "--socket", 1),
    ("-H", "--host", 1),
    ("-B", "--remote-binary", 1),
];
pub const CONTROL: &[(&str, &str, usize)] = &[
    ("-s", "--session", 1),
    ("-n", "--name", 1),
    ("-i", "--id", 1),
    ("-j", "--json", 0),
    ("-d", "--detach", 0),
    ("-t", "--text", 1),
    ("-f", "--file", 1),
    ("-k", "--key", 1),
    ("-l", "--lines", 1),
    ("-b", "--buffer", 1),
    ("-c", "--contains", 1),
    ("-T", "--timeout-ms", 1),
    ("-w", "--window", 1),
    ("-q", "--query", 1),
    ("-o", "--offset", 1),
    ("-L", "--limit", 1),
    ("-p", "--params", 1),
    ("-I", "--idempotency", 1),
    ("-D", "--desktop", 0),
    ("-e", "--terminal", 0),
    ("-R", "--repair", 0),
    ("-S", "--subscription", 1),
    ("-a", "--after", 1),
    ("-y", "--types", 1),
    ("-P", "--profile", 1),
    ("-A", "--activate", 0),
    ("-g", "--group", 1),
    ("-r", "--protocol", 1),
    ("-F", "--favorites", 0),
    ("-N", "--snapshot", 1),
    ("-O", "--viewer", 0),
];
pub const DESKTOP: &[(&str, &str, usize)] = &[
    ("-G", "--primary-companion", 1),
    ("-s", "--socket", 1),
    ("-S", "--session", 1),
    ("-f", "--fixture-dir", 1),
    ("-d", "--staging-dir", 1),
    ("-w", "--window-id", 1),
    ("-n", "--new-instance", 0),
    ("-m", "--show-machines", 0),
    ("-c", "--clipboard-disabled", 0),
    ("-W", "--allow-workspace-control", 0),
    ("-R", "--allow-rdp-control", 0),
    ("-t", "--theme", 1),
    ("-v", "--version", 0),
    ("-h", "--help", 0),
    ("-E", "--enable-primary-receive", 0),
    ("-D", "--disable-primary-receive", 0),
    ("-Y", "--dynamic-rdp", 0),
    ("-p", "--paste-primary", 1),
    ("-r", "--receive-primary", 4),
    ("-x", "--disable-primary", 1),
];
pub fn options(args: &mut [String], table: &[(&str, &str, usize)]) {
    let mut value = 0;
    for arg in args {
        if value > 0 {
            value -= 1;
            continue;
        }
        if arg == "--" {
            break;
        }
        if let Some((_, long, takes_value)) = table
            .iter()
            .find(|(short, long, _)| arg == short || arg == long)
        {
            *arg = (*long).into();
            value = *takes_value;
        }
    }
}
pub fn normalize(args: &mut [String]) {
    let mut command = 0;
    while command < args.len() {
        let Some((_, long, _)) = GLOBAL
            .iter()
            .find(|(short, long, _)| args[command] == *short || args[command] == *long)
        else {
            break;
        };
        args[command] = (*long).into();
        command += 2;
    }
    let Some(name) = args.get(command).map(String::as_str) else {
        return;
    };
    if matches!(name, "-r" | "-x") && args.len() == command + 3 && args[command + 1] == "-s" {
        args[command + 1] = "--session".into();
        return;
    }
    if name == "gui" {
        options(&mut args[command + 1..], DESKTOP);
    } else if matches!(
        name,
        "session"
            | "window"
            | "pane"
            | "job"
            | "workspace"
            | "host"
            | "rdp"
            | "doctor"
            | "attach"
            | "rpc"
            | "describe"
            | "health"
            | "version"
            | "capabilities"
            | "schema"
            | "-ls"
            | "ls"
            | "list"
            | "-l"
            | "--list"
            | "-wipe"
    ) {
        options(&mut args[command + 1..], CONTROL);
    }
}
pub fn help(table: &[(&str, &str, usize)]) -> String {
    table
        .iter()
        .map(|(short, long, _)| format!("{short} {long}"))
        .collect::<Vec<_>>()
        .join(" · ")
}
#[cfg(test)]
mod tests {
    use super::*;
    fn args(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| (*s).into()).collect()
    }
    #[test]
    fn all_four_primary_route_values_are_opaque() {
        let mut a = args(&["gui", "-r", "endpoint_fixture", "-x", "-p", "-m"]);
        normalize(&mut a);
        assert_eq!(
            a,
            args(&[
                "gui",
                "--receive-primary",
                "endpoint_fixture",
                "-x",
                "-p",
                "-m"
            ])
        );
    }
    #[test]
    fn values_are_never_rewritten_and_screen_commands_keep_their_meaning() {
        let mut a = args(&[
            "-H",
            "-i",
            "pane",
            "send-text",
            "-i",
            "pane_1",
            "-t",
            "-j",
            "-j",
        ]);
        normalize(&mut a);
        assert_eq!(
            a,
            args(&[
                "--host",
                "-i",
                "pane",
                "send-text",
                "--id",
                "pane_1",
                "--text",
                "-j",
                "--json"
            ])
        );
        let mut a = args(&["-S", "-i", "-X", "title", "-j"]);
        let original = a.clone();
        normalize(&mut a);
        assert_eq!(a, original);
        let mut a = args(&["image-stage", "-i", "-j"]);
        let original = a.clone();
        normalize(&mut a);
        assert_eq!(a, original);
    }
    #[test]
    fn parsed_requests_and_desktop_options_use_the_same_targets() {
        let mut a = args(&["pane", "send-text", "-i", "pane_demo", "-t", "--json", "-j"]);
        normalize(&mut a);
        assert_eq!(
            a,
            args(&[
                "pane",
                "send-text",
                "--id",
                "pane_demo",
                "--text",
                "--json",
                "--json"
            ])
        );
        let mut a = args(&["gui", "-s", "/tmp/private", "-S", "-m", "-m"]);
        normalize(&mut a);
        assert_eq!(
            a,
            args(&[
                "gui",
                "--socket",
                "/tmp/private",
                "--session",
                "-m",
                "--show-machines"
            ])
        );
    }
    #[test]
    fn option_terminator_and_help_inventory_are_unambiguous() {
        for table in [GLOBAL, CONTROL, DESKTOP] {
            let mut seen = std::collections::HashSet::new();
            for (short, long, _) in table {
                assert!(seen.insert(short));
                assert!(short.starts_with('-') && !short.starts_with("--"));
                assert!(long.starts_with("--"));
            }
        }
        let mut a = args(&["-j", "--", "-i"]);
        options(&mut a, CONTROL);
        assert_eq!(a, args(&["--json", "--", "-i"]));
    }
}
