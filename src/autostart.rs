//! Starting with the session, so that notes are there when the computer is:
//! the `Run` key of the registry on Windows; on Linux, the Background portal
//! inside the Flatpak (the sandbox has no access to the session's autostart
//! folder), an XDG autostart entry otherwise.

#[cfg(windows)]
mod platform {
    use std::os::windows::process::CommandExt;
    use std::path::PathBuf;
    use std::process::{Command, Stdio};

    const KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    /// By its full path: a plain `reg` would first be looked for beside the
    /// executable, which is often the Downloads folder.
    fn reg_exe() -> PathBuf {
        let root = std::env::var_os("SystemRoot").unwrap_or_else(|| r"C:\Windows".into());
        PathBuf::from(root).join("System32").join("reg.exe")
    }

    fn reg(args: &[&str]) -> bool {
        Command::new(reg_exe())
            .args(args)
            .creation_flags(CREATE_NO_WINDOW)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    }

    pub fn is_enabled() -> bool {
        reg(&["query", KEY, "/v", "Papillon"])
    }

    pub fn set(enabled: bool) -> bool {
        if !enabled {
            return reg(&["delete", KEY, "/v", "Papillon", "/f"]);
        }
        let Ok(exe) = std::env::current_exe() else {
            return false;
        };
        let command = format!("\"{}\"", exe.display());
        reg(&["add", KEY, "/v", "Papillon", "/d", &command, "/f"])
    }

    /// The portable .exe may have moved since the entry was written.
    pub fn refresh() {
        if is_enabled() {
            set(true);
        }
    }
}

#[cfg(not(windows))]
mod platform {
    use std::path::PathBuf;
    use std::process::{Command, Stdio};

    fn in_flatpak() -> bool {
        std::env::var_os("FLATPAK_ID").is_some()
    }

    /// The portal doesn't say what it did: the wish is remembered here.
    fn wish_file() -> PathBuf {
        crate::config::dir().join("autostart")
    }

    fn entry() -> PathBuf {
        let config = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".config")
            });
        config
            .join("autostart")
            .join(format!("{}.desktop", crate::app::APP_ID))
    }

    /// The program's path as the Exec key of a desktop entry wants it.
    pub(super) fn exec_value(path: &str) -> Option<String> {
        if path.contains(['\n', '\r']) {
            return None;
        }
        let mut quoted = String::from("\"");
        for c in path.chars() {
            match c {
                // Escaped once for Exec, and backslashes once more for the
                // file format.
                '"' | '`' | '$' => {
                    quoted.push_str("\\\\");
                    quoted.push(c);
                }
                '\\' => quoted.push_str("\\\\\\\\"),
                '%' => quoted.push_str("%%"),
                c => quoted.push(c),
            }
        }
        quoted.push('"');
        Some(quoted)
    }

    fn set_with_portal(enabled: bool) -> bool {
        let options = format!(
            "{{'reason': <'Papillon'>, 'autostart': <{enabled}>, 'background': <{enabled}>, 'commandline': <['papillon']>}}"
        );
        Command::new("gdbus")
            .args([
                "call",
                "--session",
                "--dest",
                "org.freedesktop.portal.Desktop",
                "--object-path",
                "/org/freedesktop/portal/desktop",
                "--method",
                "org.freedesktop.portal.Background.RequestBackground",
                "",
                &options,
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    }

    pub fn is_enabled() -> bool {
        if in_flatpak() {
            wish_file().exists()
        } else {
            entry().exists()
        }
    }

    pub fn set(enabled: bool) -> bool {
        if in_flatpak() {
            if !set_with_portal(enabled) {
                return false;
            }
            return if enabled {
                std::fs::write(wish_file(), "").is_ok()
            } else {
                std::fs::remove_file(wish_file()).is_ok()
            };
        }
        let path = entry();
        if !enabled {
            return std::fs::remove_file(path).is_ok();
        }
        let exe = std::env::current_exe().ok();
        let Some(exec) = exe.and_then(|e| exec_value(&e.display().to_string())) else {
            return false;
        };
        let content = format!(
            "[Desktop Entry]\nType=Application\nName=Papillon\nExec={exec}\nIcon={}\nTerminal=false\nX-GNOME-Autostart-enabled=true\n",
            crate::app::APP_ID
        );
        path.parent()
            .is_some_and(|dir| std::fs::create_dir_all(dir).is_ok())
            && std::fs::write(path, content).is_ok()
    }

    pub fn refresh() {}
}

pub use platform::{is_enabled, refresh, set};

#[cfg(all(test, not(windows)))]
mod tests {
    use super::platform::exec_value;

    #[test]
    fn exec_values_survive_odd_paths() {
        assert_eq!(
            exec_value("/home/me/my apps/papillon").unwrap(),
            "\"/home/me/my apps/papillon\""
        );
        assert_eq!(exec_value("/a%b").unwrap(), "\"/a%%b\"");
        assert_eq!(exec_value("/a$b").unwrap(), "\"/a\\\\$b\"");
        assert_eq!(exec_value("/a\nb"), None);
    }
}
