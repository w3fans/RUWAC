//! User shell selection shared by the daemon and graphical client.
use std::{ffi::CStr, path::Path};
/// Apply terminal defaults even when an older daemon supplies the PTY.
/// Keep login-shell discovery literal; this wrapper executes on its endpoint.
pub fn terminal_argv(argv: Vec<String>) -> Vec<String> {
    let mut command = vec![
        "/usr/bin/env".into(),
        "-u".into(),
        "NO_COLOR".into(),
        "--".into(),
    ];
    command.extend(argv);
    command
}
pub fn default_terminal_argv() -> Result<Vec<String>, String> {
    local_terminal_argv(default_argv()?)
}
/// New local terminals use the creating frontend's desktop, not the daemon's.
/// An absent variable deliberately removes a stale inherited value. Remote
/// launches must use terminal_argv: local display credentials cannot cross SSH.
pub fn local_terminal_argv(argv: Vec<String>) -> Result<Vec<String>, String> {
    desktop_argv(argv, |key| match std::env::var(key) {
        Ok(value) => Ok(Some(value)),
        Err(std::env::VarError::NotPresent) => Ok(None),
        Err(_) => Err(format!("Desktop environment variable {key} is not UTF-8")),
    })
}
const DESKTOP_ENV: &[&str] = &[
    "DISPLAY",
    "WAYLAND_DISPLAY",
    "XAUTHORITY",
    "XDG_RUNTIME_DIR",
    "DBUS_SESSION_BUS_ADDRESS",
    "XDG_SESSION_TYPE",
];
fn desktop_argv(
    argv: Vec<String>,
    mut read: impl FnMut(&str) -> Result<Option<String>, String>,
) -> Result<Vec<String>, String> {
    let mut command = vec!["/usr/bin/env".into(), "-u".into(), "NO_COLOR".into()];
    let mut assignments = Vec::new();
    for key in DESKTOP_ENV {
        command.extend(["-u".into(), (*key).into()]);
        if let Some(value) = read(key)?.filter(|value| !value.is_empty()) {
            if value.len() > 4096 || value.chars().any(char::is_control) {
                return Err(format!("Invalid desktop environment variable {key}"));
            }
            assignments.push(format!("{key}={value}"));
        }
    }
    command.push("--".into());
    command.extend(assignments);
    command.extend(argv);
    Ok(command)
}
pub fn default_argv() -> Result<Vec<String>, String> {
    let configured = std::env::var("SHELL").ok().filter(|s| !s.is_empty());
    let shell = if let Some(shell) = configured {
        shell
    } else {
        let mut passwd: libc::passwd = unsafe { std::mem::zeroed() };
        let mut result = std::ptr::null_mut();
        let mut bytes = vec![0u8; 65536];
        let code = unsafe {
            libc::getpwuid_r(
                libc::geteuid(),
                &mut passwd,
                bytes.as_mut_ptr().cast(),
                bytes.len(),
                &mut result,
            )
        };
        if code != 0 || result.is_null() || passwd.pw_shell.is_null() {
            return Err("User login shell unavailable; set SHELL to an absolute executable".into());
        }
        unsafe { CStr::from_ptr(passwd.pw_shell) }
            .to_str()
            .map_err(|_| "User shell path is not UTF-8")?
            .to_owned()
    };
    login_argv(&shell)
}
fn login_argv(shell: &str) -> Result<Vec<String>, String> {
    if !Path::new(shell).is_absolute()
        || shell.len() > 4096
        || shell.chars().any(char::is_control)
        || !std::fs::metadata(shell).is_ok_and(|m| m.is_file())
    {
        return Err("User shell must be an absolute executable path".into());
    }
    Ok(vec![shell.into(), "-l".into()])
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn user_shell_is_literal_and_loads_login_config() {
        assert_eq!(login_argv("/bin/bash").unwrap(), ["/bin/bash", "-l"]);
        assert!(login_argv("bash").is_err());
        assert!(login_argv("/bin/bash -c command").is_err());
        assert!(login_argv("/bin/bash\n").is_err());
    }
    #[test]
    fn desktop_snapshot_overrides_and_removes_daemon_values_literally() {
        let command = desktop_argv(vec!["/usr/bin/env".into()], |key| {
            Ok(match key {
                "DISPLAY" => Some(":fresh".into()),
                "XAUTHORITY" => Some("/generated/auth $(literal) file".into()),
                _ => None,
            })
        })
        .unwrap();
        let output = std::process::Command::new(&command[0])
            .args(&command[1..])
            .env_clear()
            .env("DISPLAY", ":stale")
            .env("WAYLAND_DISPLAY", "wayland-stale")
            .env("DBUS_SESSION_BUS_ADDRESS", "unix:path=/generated/stale")
            .env("NO_COLOR", "1")
            .env("UNRELATED", "preserved")
            .output()
            .unwrap();
        assert!(output.status.success());
        let result = String::from_utf8(output.stdout).unwrap();
        assert!(result.lines().any(|line| line == "DISPLAY=:fresh"));
        assert!(
            result
                .lines()
                .any(|line| line == "XAUTHORITY=/generated/auth $(literal) file")
        );
        assert!(result.lines().any(|line| line == "UNRELATED=preserved"));
        assert!(
            !result
                .lines()
                .any(|line| line.starts_with("WAYLAND_DISPLAY=")
                    || line.starts_with("DBUS_SESSION_BUS_ADDRESS=")
                    || line.starts_with("NO_COLOR="))
        );
    }
    #[test]
    fn invalid_desktop_values_fail_before_a_command_is_created() {
        for invalid in ["injected\nvalue".to_owned(), "x".repeat(4097)] {
            assert!(desktop_argv(vec!["/bin/bash".into()], |_| Ok(Some(invalid.clone()))).is_err());
        }
    }
}
