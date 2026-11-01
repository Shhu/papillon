//! Starting with the session, so that notes are there when the computer is:
//! the `Run` key of the registry on Windows, an XDG autostart entry on Linux.

#[cfg(windows)]
mod platform {
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};

    const KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    fn reg(args: &[&str]) -> bool {
        Command::new("reg")
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

    /// In the real `~/.config`: inside the Flatpak, `XDG_CONFIG_HOME` is the
    /// app's own folder, which the session doesn't read.
    fn entry() -> PathBuf {
        let home = std::env::var_os("HOME").unwrap_or_default();
        PathBuf::from(home)
            .join(".config/autostart")
            .join(format!("{}.desktop", crate::app::APP_ID))
    }

    fn command() -> Option<String> {
        if std::env::var_os("FLATPAK_ID").is_some() {
            return Some(format!(
                "flatpak run --command=papillon {}",
                crate::app::APP_ID
            ));
        }
        Some(std::env::current_exe().ok()?.display().to_string())
    }

    pub fn is_enabled() -> bool {
        entry().exists()
    }

    pub fn set(enabled: bool) -> bool {
        let path = entry();
        if !enabled {
            return std::fs::remove_file(path).is_ok();
        }
        let Some(command) = command() else {
            return false;
        };
        let content = format!(
            "[Desktop Entry]\nType=Application\nName=Papillon\nExec={command}\nIcon={}\nTerminal=false\nX-GNOME-Autostart-enabled=true\n",
            crate::app::APP_ID
        );
        path.parent()
            .is_some_and(|dir| std::fs::create_dir_all(dir).is_ok())
            && std::fs::write(path, content).is_ok()
    }

    pub fn refresh() {}
}

pub use platform::{is_enabled, refresh, set};
