//! Only one Papillon at a time: two would overwrite each other's notes. The
//! first one holds a lock on a file; a later one leaves a request file, which
//! the first one notices, and quits. Files rather than a socket because
//! Flatpak gives each run of an app its own network.

use std::fs::{self, File, OpenOptions, TryLockError};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

/// How long a later launch waits for the first one: it may be quitting.
const WAITING: Duration = Duration::from_millis(1500);
/// A request younger than this comes from a launch happening right now.
const RECENT: Duration = Duration::from_secs(2);

/// Held for as long as the app runs: the system frees the lock when it ends,
/// even after a crash. Nothing when no lock is possible at all.
pub struct Instance {
    _lock: Option<File>,
}

fn lock_file() -> PathBuf {
    crate::config::dir().join("lock")
}

fn request_file() -> PathBuf {
    crate::config::dir().join("show-request")
}

enum Attempt {
    Mine(File),
    /// Another Papillon holds it.
    Taken(File),
    /// This place can't lock (read-only folder, network disk…).
    Impossible,
}

fn try_lock(path: &Path) -> Attempt {
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(path);
    let Ok(file) = file else {
        return Attempt::Impossible;
    };
    match file.try_lock() {
        Ok(()) => Attempt::Mine(file),
        Err(TryLockError::WouldBlock) => Attempt::Taken(file),
        Err(TryLockError::Error(_)) => Attempt::Impossible,
    }
}

/// The lock if no other Papillon runs; otherwise tells that one to show itself.
pub fn acquire() -> Option<Instance> {
    let _ = fs::create_dir_all(crate::config::dir());
    let attempt = match try_lock(&lock_file()) {
        // Not in the settings folder, then: still one instance at a time, for
        // this user, even if it can't be told to show itself.
        Attempt::Impossible => {
            let name = format!("papillon-{}.lock", user_name());
            try_lock(&std::env::temp_dir().join(name))
        }
        attempt => attempt,
    };
    match attempt {
        Attempt::Mine(lock) => {
            discard_old_request();
            Some(Instance { _lock: Some(lock) })
        }
        Attempt::Taken(lock) => ask_the_other(lock).map(|lock| Instance { _lock: Some(lock) }),
        // Better notes that can't be protected than no notes at all.
        Attempt::Impossible => Some(Instance { _lock: None }),
    }
}

fn user_name() -> String {
    std::env::var("USERNAME")
        .or_else(|_| std::env::var("USER"))
        .unwrap_or_default()
}

/// A request left when nobody was running is out of date, but a launch
/// happening at the same time as this one is waiting for an answer.
fn discard_old_request() {
    let age = fs::metadata(request_file())
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| SystemTime::now().duration_since(t).ok());
    if age.is_some_and(|age| age > RECENT) {
        let _ = fs::remove_file(request_file());
    }
}

/// Leaves the request and waits for it to be taken. If the other one was
/// quitting, its lock comes free: this launch takes over.
fn ask_the_other(lock: File) -> Option<File> {
    let _ = fs::write(request_file(), "");
    let deadline = Instant::now() + WAITING;
    while Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
        if lock.try_lock().is_ok() {
            let _ = fs::remove_file(request_file());
            return Some(lock);
        }
        if !request_file().exists() {
            // Taken, unless it was by a Papillon about to end.
            std::thread::sleep(Duration::from_millis(300));
            return lock.try_lock().is_ok().then_some(lock);
        }
    }
    None
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
