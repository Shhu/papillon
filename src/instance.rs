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

/// Held for as long as the app runs: the system frees the locks when it ends,
/// even after a crash. Empty when no lock is possible at all.
pub struct Instance {
    _locks: Vec<File>,
}

fn lock_file() -> PathBuf {
    crate::config::dir().join("lock")
}

fn request_file() -> PathBuf {
    crate::config::dir().join("show-request")
}

/// A second place to lock, for a settings folder that can't (read-only,
/// network disk…). Private to the user, and shared by all runs of the app.
fn fallback_lock_file() -> Option<PathBuf> {
    if cfg!(windows) {
        return Some(std::env::temp_dir().join("papillon.lock"));
    }
    let runtime = PathBuf::from(std::env::var_os("XDG_RUNTIME_DIR")?);
    Some(match std::env::var_os("FLATPAK_ID") {
        Some(id) => runtime.join("app").join(id).join("papillon.lock"),
        None => runtime.join("papillon.lock"),
    })
}

enum Attempt {
    Mine(File),
    /// Another Papillon holds it.
    Taken(File),
    /// This place can't lock.
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

/// The locks if no other Papillon runs; otherwise tells that one to show itself.
pub fn acquire() -> Option<Instance> {
    let _ = fs::create_dir_all(crate::config::dir());
    let mut locks = Vec::new();
    // Both places: an earlier launch may have managed with only one of them.
    let mut places = vec![lock_file()];
    places.extend(fallback_lock_file());
    for place in places {
        match try_lock(&place) {
            Attempt::Mine(lock) => locks.push(lock),
            Attempt::Taken(lock) => locks.push(ask_the_other(lock)?),
            // Better notes that can't be protected than no notes at all.
            Attempt::Impossible => {}
        }
    }
    discard_old_request();
    Some(Instance { _locks: locks })
}

/// A request left when nobody was running is out of date, but a launch
/// happening at the same time as this one is waiting for an answer.
fn discard_old_request() {
    let Ok(written) = fs::metadata(request_file()).and_then(|m| m.modified()) else {
        return;
    };
    // From the future: the clock moved, so it says nothing about now.
    let recent = SystemTime::now()
        .duration_since(written)
        .is_ok_and(|age| age <= RECENT);
    if !recent {
        let _ = fs::remove_file(request_file());
    }
}

/// Leaves the request and waits. If the other one was quitting, its lock comes
/// free, even after it took the request: this launch takes over.
fn ask_the_other(lock: File) -> Option<File> {
    let _ = fs::write(request_file(), "");
    let deadline = Instant::now() + WAITING;
    while Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
        if lock.try_lock().is_ok() {
            let _ = fs::remove_file(request_file());
            return Some(lock);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_held_lock_is_taken_and_comes_free_when_dropped() {
        let dir = std::env::temp_dir().join(format!("papillon-lock-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("lock");
        let Attempt::Mine(first) = try_lock(&path) else {
            panic!("the first one gets it");
        };
        assert!(matches!(try_lock(&path), Attempt::Taken(_)));
        drop(first);
        assert!(matches!(try_lock(&path), Attempt::Mine(_)));
        assert!(matches!(
            try_lock(&dir.join("no-such-folder").join("lock")),
            Attempt::Impossible
        ));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_waiting_launch_takes_over_when_the_other_one_ends() {
        let dir = std::env::temp_dir().join(format!("papillon-wait-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("lock");
        let Attempt::Mine(first) = try_lock(&path) else {
            panic!("the first one gets it");
        };
        let Attempt::Taken(second) = try_lock(&path) else {
            panic!("held by the first one");
        };
        let quitting = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(200));
            drop(first);
        });
        assert!(ask_the_other(second).is_some());
        quitting.join().unwrap();
        fs::remove_dir_all(dir).unwrap();
    }
}
