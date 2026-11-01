//! Where Papillon keeps its settings and notes.

use std::path::PathBuf;

/// `%APPDATA%\Papillon` on Windows, else `papillon` in `$XDG_CONFIG_HOME` or
/// `~/.config`.
pub fn dir() -> PathBuf {
    // Tests never touch the user's settings.
    if cfg!(test) {
        return std::env::temp_dir().join("papillon-test-config");
    }
    if let Some(roaming) = std::env::var_os("APPDATA").filter(|_| cfg!(windows)) {
        PathBuf::from(roaming).join("Papillon")
    } else if let Some(config) = std::env::var_os("XDG_CONFIG_HOME") {
        PathBuf::from(config).join("papillon")
    } else {
        let home = std::env::var_os("HOME").unwrap_or_default();
        PathBuf::from(home).join(".config").join("papillon")
    }
}
