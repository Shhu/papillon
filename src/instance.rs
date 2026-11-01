//! Only one Papillon at a time: two would overwrite each other's notes. The
//! first one holds a lock on a file; a later one leaves a request file, which
//! the first one notices, and quits. Files rather than a socket because
//! Flatpak gives each run of an app its own network.

use std::fs::{self, File, OpenOptions};
use std::path::PathBuf;
use std::time::Duration;

/// Held for as long as the app runs: the system frees the lock when it ends,
/// even after a crash.
pub struct Instance {
    _lock: File,
}

fn lock_file() -> PathBuf {
    crate::config::dir().join("lock")
}

fn request_file() -> PathBuf {
    crate::config::dir().join("show-request")
}

/// The lock if no other Papillon runs; otherwise tells that one to show itself.
pub fn acquire() -> Option<Instance> {
    let _ = fs::create_dir_all(crate::config::dir());
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(lock_file());
    match lock {
        Ok(lock) => {
            if lock.try_lock().is_ok() {
                // A request left when nobody was running is out of date.
                let _ = fs::remove_file(request_file());
                Some(Instance { _lock: lock })
            } else {
                let _ = fs::write(request_file(), "");
                None
            }
        }
        // An unusable settings folder must not stop the notes from opening.
        Err(_) => Some(Instance {
            _lock: File::open(std::env::current_exe().unwrap_or_default()).ok()?,
        }),
    }
}

/// Calls `on_request` from a background thread when another launch asks.
pub fn watch(on_request: impl Fn() + Send + 'static) {
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(Duration::from_millis(500));
            if fs::remove_file(request_file()).is_ok() {
                on_request();
            }
        }
    });
}
