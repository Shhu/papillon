//! The notes and their file. Saved shortly after each change, by replacing
//! the file in one step, so that a crash or a power cut leaves either the old
//! version or the new one, never half of it. What can't be read is never
//! overwritten.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU8, AtomicU64, Ordering};
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// How long after a change the notes are written.
pub const SAVE_DELAY: Duration = Duration::from_secs(1);
/// However long the typing goes on, the notes are written this often.
const MAX_SAVE_DELAY: Duration = Duration::from_secs(5);

pub const DEFAULT_SIZE: [f32; 2] = [280.0, 280.0];
pub const MIN_SIZE: [f32; 2] = [170.0, 120.0];
/// How long a window takes to land where it was asked to be.
const SETTLING: Duration = Duration::from_millis(500);
/// How long a note insists on getting the focus: the system may refuse it.
const RAISING: Duration = Duration::from_secs(1);
const FIRST_POSITION: [f32; 2] = [80.0, 80.0];
/// How far a new note lands from the previous one.
const CASCADE: f32 = 32.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Color {
    #[default]
    Yellow,
    Green,
    Pink,
    Purple,
    Blue,
    Gray,
    Charcoal,
}

impl Color {
    pub const ALL: [Color; 7] = [
        Color::Yellow,
        Color::Green,
        Color::Pink,
        Color::Purple,
        Color::Blue,
        Color::Gray,
        Color::Charcoal,
    ];
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Note {
    pub id: u64,
    pub text: String,
    #[serde(default)]
    pub color: Color,
    /// Outer position of the window, in physical pixels.
    pub position: [f32; 2],
    pub size: [f32; 2],
    /// Stays above the other windows.
    #[serde(default)]
    pub pinned: bool,
    /// Shown on the desktop, or only in the list.
    #[serde(default = "yes")]
    pub open: bool,
    /// Last change of the text, in seconds since 1970.
    #[serde(default)]
    pub updated: u64,
}

fn yes() -> bool {
    true
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TextSize {
    Small,
    #[default]
    Medium,
    Large,
}

impl TextSize {
    pub fn points(self) -> f32 {
        match self {
            TextSize::Small => 13.0,
            TextSize::Medium => 15.0,
            TextSize::Large => 18.0,
        }
    }
}

/// What the file holds.
#[derive(Debug, Default, PartialEq, Deserialize)]
struct Saved {
    #[serde(default)]
    notes: Vec<Note>,
    #[serde(default)]
    text_size: TextSize,
}

/// Whether a window placed at this position, of this size, can be seen.
pub type OnScreen = fn([f32; 2], [f32; 2]) -> bool;

/// A window placed by the app, on its way to where it was asked to be.
struct Placed {
    /// It was seen where it landed.
    landed: bool,
    /// Until when moves are still the landing's (None once it's over).
    settling: Option<Instant>,
    seen: [f32; 2],
}

/// Everything the windows share.
#[derive(Default)]
pub struct State {
    pub notes: Vec<Note>,
    pub text_size: TextSize,
    /// The note to bring to the front, its text ready for typing.
    pub raise: Option<u64>,
    raise_until: Option<Instant>,
    /// The note whose text gets the focus back, after a click on a button.
    pub refocus: Option<u64>,
    /// Notes to take out of the taskbar, for the main window's pass.
    pub unminimize: Vec<u64>,
    /// Notes to move back on screen: their new position.
    pub relocate: HashMap<u64, [f32; 2]>,
    /// The note whose color choice is unfolded.
    pub choosing_color: Option<u64>,
    /// The note asking whether to delete it.
    pub confirming_delete: Option<u64>,
    /// Position and size each open window was created with: changing them
    /// in its description would move it back there.
    pub spawned: HashMap<u64, ([f32; 2], [f32; 2])>,
    placed: HashMap<u64, Placed>,
    /// Notes gone, whose undo history can go.
    pub forgotten: Vec<u64>,
    pub show_list: bool,
    pub list_visible: bool,
    pub quit: bool,
    /// Quitting was asked again after saving failed.
    pub quit_anyway: bool,
    pub search: String,
    pub autostart: bool,
    /// The unreadable notes file was kept under this name.
    pub set_aside: Option<PathBuf>,
    /// The notes file exists but can't be read or copied: nothing is written
    /// over it.
    pub protected: bool,
    pub save_failed: bool,
    /// Tells whether a window can be seen; everything can, without it.
    pub on_screen: Option<OnScreen>,
    /// Called from the writing thread when a write failed.
    pub on_write_failed: Option<Arc<dyn Fn() + Send + Sync>>,
    file: PathBuf,
    /// The last change not written yet, and the first one.
    changed: Option<Instant>,
    first_change: Option<Instant>,
    /// Something changed since launch: an untouched file stays as it is.
    touched: bool,
    revision: u64,
    writer: Option<Writer>,
    outcome: Arc<WriterShared>,
}

/// The notes of the usual file.
pub fn load() -> State {
    load_from(crate::config::dir().join("notes.json"))
}

fn load_from(file: PathBuf) -> State {
    let mut state = State {
        file: file.clone(),
        ..Default::default()
    };
    let bytes = match std::fs::read(&file) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return state,
        // Locked, no permission, bad disk: it may well come back.
        Err(_) => {
            state.protected = true;
            return state;
        }
    };
    let saved = serde_json::from_slice::<Saved>(&bytes).unwrap_or_else(|_| {
        // Never overwrite what can't be read: someone may rescue it. Copied,
        // since a file held open by another program can't be renamed.
        let copy = broken_name(&file);
        match std::fs::write(&copy, &bytes) {
            Ok(()) => state.set_aside = Some(copy),
            Err(_) => state.protected = true,
        }
        salvage(&bytes)
    });
    state.notes = saved.notes;
    state.text_size = saved.text_size;
    state
}

/// The notes that are still good in a file that isn't, one by one: a broken
/// byte or a field from a newer version shouldn't cost all the others.
fn salvage(bytes: &[u8]) -> Saved {
    let text = String::from_utf8_lossy(bytes);
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
        return Saved::default();
    };
    let notes = value
        .get("notes")
        .and_then(|n| n.as_array())
        .map(|notes| {
            notes
                .iter()
                .filter_map(|n| serde_json::from_value(n.clone()).ok())
                .collect()
        })
        .unwrap_or_default();
    let text_size = value
        .get("text_size")
        .and_then(|s| serde_json::from_value(s.clone()).ok())
        .unwrap_or_default();
    Saved { notes, text_size }
}

fn broken_name(file: &Path) -> PathBuf {
    file.with_extension(format!("broken-{}.json", now()))
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

impl State {
    pub fn note_mut(&mut self, id: u64) -> Option<&mut Note> {
        self.notes.iter_mut().find(|n| n.id == id)
    }

    pub fn touch(&mut self) {
        let now = Instant::now();
        self.changed = Some(now);
        self.first_change.get_or_insert(now);
        self.touched = true;
        self.revision += 1;
    }

    /// Goes up with each change: tells the list there is something to redraw.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// The text of a note changed.
    pub fn text_changed(&mut self, id: u64) {
        if let Some(note) = self.note_mut(id) {
            note.updated = now();
        }
        self.touch();
    }

    fn shows(&self, position: [f32; 2], size: [f32; 2]) -> bool {
        self.on_screen.is_none_or(|f| f(position, size))
    }

    /// A new note, a little beside the note `near` if any, otherwise beside
    /// the newest one, or in the corner if that would be out of sight.
    pub fn create(&mut self, near: Option<u64>) -> u64 {
        let id = self.notes.iter().map(|n| n.id).max().unwrap_or(0) + 1;
        let beside = near
            .and_then(|near| self.notes.iter().find(|n| n.id == near))
            .or_else(|| self.notes.iter().rfind(|n| n.open));
        let color = beside.map_or(Color::default(), |n| n.color);
        let position = beside
            .map(|n| [n.position[0] + CASCADE, n.position[1] + CASCADE])
            .filter(|&p| self.shows(p, DEFAULT_SIZE))
            .unwrap_or(FIRST_POSITION);
        self.notes.push(Note {
            id,
            text: String::new(),
            color,
            position,
            size: DEFAULT_SIZE,
            pinned: false,
            open: true,
            updated: now(),
        });
        self.ask_raise(id);
        self.touch();
        id
    }

    /// Brings the note to the front.
    pub fn ask_raise(&mut self, id: u64) {
        self.raise = Some(id);
        self.raise_until = Some(Instant::now() + RAISING);
        self.unminimize.push(id);
    }

    /// The note stops insisting; true once it got the focus or gave up.
    pub fn raise_over(&mut self, focused: bool) -> bool {
        let over = focused || self.raise_until.is_none_or(|t| Instant::now() >= t);
        if over {
            self.raise = None;
            self.raise_until = None;
        }
        over
    }

    /// Takes the note off the desktop; it stays in the list unless it's empty.
    pub fn close(&mut self, id: u64) {
        let empty = self
            .notes
            .iter()
            .any(|n| n.id == id && n.text.trim().is_empty());
        if empty {
            self.delete(id);
            return;
        }
        if let Some(note) = self.note_mut(id) {
            note.open = false;
        }
        self.window_gone(id);
    }

    pub fn delete(&mut self, id: u64) {
        self.notes.retain(|n| n.id != id);
        self.window_gone(id);
    }

    /// Deletes an empty note, or asks before deleting a written one. True if
    /// the note is gone.
    pub fn request_delete(&mut self, id: u64) -> bool {
        let empty = self
            .notes
            .iter()
            .any(|n| n.id == id && n.text.trim().is_empty());
        if empty {
            self.delete(id);
        } else {
            self.confirming_delete = Some(id);
        }
        empty
    }

    fn window_gone(&mut self, id: u64) {
        self.spawned.remove(&id);
        self.placed.remove(&id);
        self.relocate.remove(&id);
        self.forgotten.push(id);
        if self.raise == Some(id) {
            self.raise = None;
        }
        if self.confirming_delete == Some(id) {
            self.confirming_delete = None;
        }
        if self.choosing_color == Some(id) {
            self.choosing_color = None;
        }
        // Without any note left on the desktop, the list is the only way back.
        if !self.notes.iter().any(|n| n.open) {
            self.show_list = true;
        }
        self.touch();
    }

    /// Shows the note, in front of the other windows.
    pub fn open(&mut self, id: u64) {
        let lost = self
            .notes
            .iter()
            .find(|n| n.id == id)
            .is_some_and(|n| !self.shows(n.position, n.size));
        if let Some(note) = self.note_mut(id) {
            note.open = true;
            if lost {
                note.position = FIRST_POSITION;
            }
        }
        self.ask_raise(id);
        self.touch();
    }

    /// Gathers the notes in the corner of the screen, for those left where
    /// no monitor shows them any more.
    pub fn bring_back(&mut self) {
        for (i, note) in self.notes.iter_mut().enumerate() {
            let offset = CASCADE * (i % 12) as f32;
            note.position = [FIRST_POSITION[0] + offset, FIRST_POSITION[1] + offset];
            if note.open {
                self.relocate.insert(note.id, note.position);
                self.unminimize.push(note.id);
            }
        }
        self.touch();
    }

    /// At launch: open notes out of sight, on a monitor that is gone, come
    /// back to the screen.
    pub fn rescue_lost(&mut self) {
        let on_screen = self.on_screen;
        let shows = |n: &Note| on_screen.is_none_or(|f| f(n.position, n.size));
        let lost: Vec<u64> = self
            .notes
            .iter()
            .filter(|n| n.open && !shows(n))
            .map(|n| n.id)
            .collect();
        for (i, id) in lost.into_iter().enumerate() {
            let offset = CASCADE * (i % 12) as f32;
            if let Some(note) = self.note_mut(id) {
                note.position = [FIRST_POSITION[0] + offset, FIRST_POSITION[1] + offset];
            }
            self.touch();
        }
    }

    /// Takes note of where a window is now. The first time, returns where it
    /// must go. A position counts only if the window moved after landing:
    /// window managers may not put it exactly where asked, and saving that
    /// would make notes walk away a little more at each launch.
    pub fn window_seen(
        &mut self,
        id: u64,
        position: [f32; 2],
        size: [f32; 2],
        now: Instant,
    ) -> Option<[f32; 2]> {
        // One last pass of a window on its way out.
        let note = self.notes.iter().find(|n| n.id == id && n.open)?;
        let Some(placed) = self.placed.get_mut(&id) else {
            let target = note.position;
            self.placed.insert(
                id,
                Placed {
                    landed: false,
                    settling: None,
                    seen: position,
                },
            );
            return Some(target);
        };
        if !placed.landed {
            // Whenever this pass comes, it's the first after the request.
            placed.landed = true;
            if placed.settling.is_none() {
                placed.settling = Some(now + SETTLING);
                placed.seen = position;
                return None;
            }
        }
        if placed.settling.is_some_and(|until| now < until) {
            placed.seen = position;
            return None;
        }
        placed.settling = None;
        let moved = placed.seen != position;
        placed.seen = position;
        let note = self.note_mut(id)?;
        if moved || note.size != size {
            if moved {
                note.position = position;
            }
            note.size = size;
            self.touch();
        }
        None
    }

    /// The user is moving or resizing this window: what follows counts.
    pub fn user_moves(&mut self, id: u64) {
        if let Some(placed) = self.placed.get_mut(&id) {
            placed.landed = true;
            placed.settling = None;
        }
    }

    pub fn has_unsaved(&self) -> bool {
        self.changed.is_some()
    }

    /// Hands the notes to the writing thread if it's time: a slow disk must
    /// not freeze the windows.
    pub fn save_if_due(&mut self) {
        self.collect_outcome();
        let quiet = self.changed.is_some_and(|t| t.elapsed() >= SAVE_DELAY);
        let overdue = self
            .first_change
            .is_some_and(|t| t.elapsed() >= MAX_SAVE_DELAY);
        if !quiet && !overdue {
            return;
        }
        let Some(content) = self.serialize() else {
            return;
        };
        self.changed = None;
        self.first_change = None;
        let generation = self.outcome.latest.fetch_add(1, Ordering::SeqCst) + 1;
        let writer = self.writer.get_or_insert_with(|| {
            Writer::start(
                self.file.clone(),
                self.outcome.clone(),
                self.on_write_failed.clone(),
            )
        });
        let _ = writer.sender.send((generation, content));
    }

    /// What the writing thread last reported.
    fn collect_outcome(&mut self) {
        match self.outcome.status.swap(WRITE_NONE, Ordering::SeqCst) {
            WRITE_OK => self.save_failed = false,
            WRITE_FAILED => self.write_failed(),
            _ => {}
        }
    }

    fn write_failed(&mut self) {
        if !self.save_failed {
            self.show_list = true;
        }
        self.save_failed = true;
        // Tried again after the usual delay.
        let now = Instant::now();
        self.changed = Some(now);
        self.first_change.get_or_insert(now);
    }

    /// Writes the notes now if anything changed since launch, overtaking the
    /// writing thread. False if they couldn't be.
    pub fn save(&mut self) -> bool {
        if !self.touched {
            return true;
        }
        let Some(content) = self.serialize() else {
            return false;
        };
        // Whatever the thread was about to write is older than this.
        self.outcome.latest.fetch_add(1, Ordering::SeqCst);
        let written = {
            let _file = lock(&self.outcome.file);
            write_file(&self.file, &content).is_ok()
        };
        if written {
            self.save_failed = false;
            self.changed = None;
            self.first_change = None;
        } else {
            self.write_failed();
        }
        written
    }

    /// The file's future content, or nothing if it must not be written.
    fn serialize(&mut self) -> Option<Vec<u8>> {
        #[derive(Serialize)]
        struct Saving<'a> {
            notes: &'a [Note],
            text_size: TextSize,
        }
        if self.protected {
            self.save_failed = true;
            self.changed = None;
            self.first_change = None;
            return None;
        }
        serde_json::to_vec_pretty(&Saving {
            notes: &self.notes,
            text_size: self.text_size,
        })
        .ok()
    }

    /// Notes in the list's order, most recently changed first, those matching
    /// `search` only.
    pub fn listed(&self) -> Vec<u64> {
        let search = self.search.trim().to_lowercase();
        let mut notes: Vec<&Note> = self
            .notes
            .iter()
            .filter(|n| search.is_empty() || n.text.to_lowercase().contains(&search))
            .collect();
        notes.sort_by_key(|n| (std::cmp::Reverse(n.updated), std::cmp::Reverse(n.id)));
        notes.into_iter().map(|n| n.id).collect()
    }
}

impl Note {
    /// The first two lines with text in them, for the list.
    pub fn preview(&self) -> (&str, &str) {
        let mut lines = self.text.lines().map(str::trim).filter(|l| !l.is_empty());
        (lines.next().unwrap_or(""), lines.next().unwrap_or(""))
    }
}

/// A poisoned lock only says another thread panicked: the data is still the
/// best there is, and the notes still deserve saving.
pub fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

const WRITE_NONE: u8 = 0;
const WRITE_OK: u8 = 1;
const WRITE_FAILED: u8 = 2;

/// Between the windows and the writing thread.
#[derive(Default)]
struct WriterShared {
    /// The newest version of the notes: older ones are not written.
    latest: AtomicU64,
    status: AtomicU8,
    /// Held while writing.
    file: Mutex<()>,
}

struct Writer {
    sender: Sender<(u64, Vec<u8>)>,
}

impl Writer {
    fn start(
        file: PathBuf,
        shared: Arc<WriterShared>,
        on_failed: Option<Arc<dyn Fn() + Send + Sync>>,
    ) -> Self {
        let (sender, receiver) = channel::<(u64, Vec<u8>)>();
        std::thread::spawn(move || {
            while let Ok(mut job) = receiver.recv() {
                while let Ok(newer) = receiver.try_recv() {
                    job = newer;
                }
                let _file = lock(&shared.file);
                if job.0 < shared.latest.load(Ordering::SeqCst) {
                    continue;
                }
                let ok = write_file(&file, &job.1).is_ok();
                let status = if ok { WRITE_OK } else { WRITE_FAILED };
                shared.status.store(status, Ordering::SeqCst);
                if let (false, Some(on_failed)) = (ok, &on_failed) {
                    on_failed();
                }
            }
        });
        Self { sender }
    }
}

fn write_file(file: &Path, content: &[u8]) -> std::io::Result<()> {
    use std::io::Write as _;
    let dir = file.parent();
    if let Some(dir) = dir {
        std::fs::create_dir_all(dir)?;
    }
    let temporary = file.with_extension("json.tmp");
    {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        // Notes are private, whatever the system's defaults.
        #[cfg(unix)]
        {
            use std::os::unix::fs::{OpenOptionsExt as _, PermissionsExt as _};
            options.mode(0o600);
            if let Some(dir) = dir {
                let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700));
            }
        }
        let mut output = options.open(&temporary)?;
        // A temporary file left by an older version may be more open.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            output.set_permissions(std::fs::Permissions::from_mode(0o600))?;
        }
        output.write_all(content)?;
        output.sync_all()?;
    }
    // Antivirus and sync tools hold files for a moment.
    let mut attempts = 0;
    loop {
        match replace(&temporary, file) {
            Ok(()) => break,
            Err(_) if attempts < 3 => {
                attempts += 1;
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(e) => return Err(e),
        }
    }
    // The new name is on disk too, for a power cut.
    #[cfg(unix)]
    if let Some(dir) = dir {
        std::fs::File::open(dir)?.sync_all()?;
    }
    Ok(())
}

#[cfg(unix)]
fn replace(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::rename(from, to)
}

/// As `rename`, and on disk when it returns.
#[cfg(windows)]
fn replace(from: &Path, to: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt as _;
    const MOVEFILE_REPLACE_EXISTING: u32 = 1;
    const MOVEFILE_WRITE_THROUGH: u32 = 8;
    unsafe extern "system" {
        fn MoveFileExW(from: *const u16, to: *const u16, flags: u32) -> i32;
    }
    let wide = |p: &Path| {
        p.as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect::<Vec<u16>>()
    };
    let (from, to) = (wide(from), wide(to));
    // Both strings end with their zero and outlive the call.
    let done = unsafe {
        MoveFileExW(
            from.as_ptr(),
            to.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if done != 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A folder of its own for each test, removed afterwards.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let dir =
                std::env::temp_dir().join(format!("papillon-test-{}-{name}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }

        fn file(&self) -> PathBuf {
            self.0.join("notes.json")
        }

        fn kept_copy(&self) -> Option<Vec<u8>> {
            std::fs::read_dir(&self.0)
                .unwrap()
                .filter_map(Result::ok)
                .find(|e| e.file_name().to_string_lossy().contains(".broken-"))
                .map(|e| std::fs::read(e.path()).unwrap())
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn notes_come_back_as_they_were_saved() {
        let dir = TempDir::new("round-trip");
        let mut state = load_from(dir.file());
        let id = state.create(None);
        let note = state.note_mut(id).unwrap();
        note.text = "Acheter du café\n« vite »".into();
        note.color = Color::Pink;
        note.pinned = true;
        state.text_size = TextSize::Large;
        assert!(state.save());
        assert!(!state.has_unsaved());
        let again = load_from(dir.file());
        assert_eq!(again.notes, state.notes);
        assert_eq!(again.text_size, TextSize::Large);
    }

    #[test]
    fn bad_json_is_kept_aside_not_overwritten() {
        let dir = TempDir::new("broken");
        std::fs::write(dir.file(), "{ not json").unwrap();
        let mut state = load_from(dir.file());
        assert!(state.notes.is_empty());
        assert!(state.set_aside.is_some());
        state.create(None);
        state.save();
        assert_eq!(dir.kept_copy().unwrap(), b"{ not json");
    }

    #[test]
    fn broken_bytes_are_kept_aside_and_the_notes_saved_from_them() {
        let dir = TempDir::new("bytes");
        let mut bytes = br#"{"notes":[{"id":1,"text":"caf"#.to_vec();
        bytes.push(0xE9);
        bytes.extend_from_slice(br#"","position":[1,2],"size":[3,4]}]}"#);
        std::fs::write(dir.file(), &bytes).unwrap();
        let state = load_from(dir.file());
        assert_eq!(state.notes.len(), 1);
        assert!(state.notes[0].text.starts_with("caf"));
        assert_eq!(dir.kept_copy().unwrap(), bytes);
    }

    #[test]
    fn one_note_from_a_newer_version_does_not_cost_the_others() {
        let dir = TempDir::new("newer");
        std::fs::write(
            dir.file(),
            r#"{"notes":[
                {"id":1,"text":"a","position":[1,2],"size":[3,4]},
                {"id":2,"text":"b","color":"teal","position":[1,2],"size":[3,4]}
            ]}"#,
        )
        .unwrap();
        let state = load_from(dir.file());
        assert_eq!(state.notes.len(), 1);
        assert_eq!(state.notes[0].id, 1);
        assert!(dir.kept_copy().is_some());
    }

    #[test]
    fn an_untouched_file_is_not_rewritten() {
        let dir = TempDir::new("untouched");
        std::fs::write(dir.file(), "{ not json").unwrap();
        let mut state = load_from(dir.file());
        assert!(state.save());
        assert_eq!(std::fs::read(dir.file()).unwrap(), b"{ not json");
    }

    /// Reading a folder fails with something else than "not found".
    #[test]
    fn a_file_that_cannot_be_read_is_never_written_over() {
        let dir = TempDir::new("protected");
        std::fs::create_dir(dir.file()).unwrap();
        let mut state = load_from(dir.file());
        assert!(state.protected);
        state.create(None);
        assert!(!state.save());
        assert!(state.save_failed);
        assert!(dir.file().is_dir());
    }

    #[test]
    fn a_failed_save_says_so_and_stays_to_do() {
        let dir = TempDir::new("failed");
        // The file's folder is a file.
        std::fs::write(dir.0.join("in-the-way"), "").unwrap();
        let mut state = load_from(dir.file());
        state.file = dir.0.join("in-the-way").join("notes.json");
        state.create(None);
        assert!(!state.save());
        assert!(state.save_failed);
        assert!(state.show_list, "the list shows the problem");
        assert!(state.has_unsaved());
    }

    #[test]
    fn the_writing_thread_writes_and_reports_failures() {
        let dir = TempDir::new("thread");
        let mut state = load_from(dir.file());
        state.create(None);
        // Not due yet.
        state.save_if_due();
        assert!(!dir.file().exists());
        state.changed = Some(Instant::now() - SAVE_DELAY);
        state.save_if_due();
        assert!(!state.has_unsaved());
        let deadline = Instant::now() + Duration::from_secs(5);
        while !dir.file().exists() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(load_from(dir.file()).notes.len(), 1);

        std::fs::write(dir.0.join("in-the-way"), "").unwrap();
        let mut state = load_from(dir.0.join("other.json"));
        state.file = dir.0.join("in-the-way").join("notes.json");
        state.create(None);
        state.changed = Some(Instant::now() - SAVE_DELAY);
        state.save_if_due();
        let deadline = Instant::now() + Duration::from_secs(5);
        while !state.save_failed && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
            state.collect_outcome();
        }
        assert!(state.save_failed);
    }

    #[test]
    fn long_typing_is_saved_even_without_a_pause() {
        let dir = TempDir::new("typing");
        let mut state = load_from(dir.file());
        state.create(None);
        state.first_change = Some(Instant::now() - MAX_SAVE_DELAY);
        state.touch();
        state.save_if_due();
        assert!(!state.has_unsaved());
    }

    #[cfg(unix)]
    #[test]
    fn notes_are_private() {
        use std::os::unix::fs::PermissionsExt as _;
        let dir = TempDir::new("private");
        let mut state = load_from(dir.file());
        state.create(None);
        state.save();
        let mode = std::fs::metadata(dir.file()).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[test]
    fn fields_added_later_have_defaults() {
        let dir = TempDir::new("old");
        std::fs::write(
            dir.file(),
            r#"{"notes":[{"id":3,"text":"a","position":[1,2],"size":[3,4]}]}"#,
        )
        .unwrap();
        let state = load_from(dir.file());
        assert!(state.notes[0].open);
        assert_eq!(state.notes[0].color, Color::Yellow);
        assert!(state.set_aside.is_none());
    }

    #[test]
    fn closing_keeps_a_written_note_and_forgets_an_empty_one() {
        let mut state = State::default();
        let written = state.create(None);
        state.note_mut(written).unwrap().text = "x".into();
        let empty = state.create(None);
        state.note_mut(empty).unwrap().text = "  \n".into();
        state.close(empty);
        assert_eq!(state.notes.len(), 1);
        assert!(!state.show_list, "a note is still on the desktop");
        state.close(written);
        assert!(!state.notes[0].open);
        assert!(state.show_list, "nothing left on the desktop");
    }

    #[test]
    fn new_notes_land_beside_the_one_they_came_from_with_its_color() {
        let mut state = State::default();
        let first = state.create(None);
        state.note_mut(first).unwrap().color = Color::Blue;
        let second = state.create(Some(first));
        let note = state.notes.iter().find(|n| n.id == second).unwrap();
        assert_eq!(note.color, Color::Blue);
        assert_eq!(
            note.position,
            [FIRST_POSITION[0] + CASCADE, FIRST_POSITION[1] + CASCADE]
        );
        assert_ne!(first, second);
    }

    #[test]
    fn notes_never_land_out_of_sight() {
        let mut state = State {
            on_screen: Some(|p, _| p[0] < 1000.0),
            ..Default::default()
        };
        let first = state.create(None);
        state.note_mut(first).unwrap().position = [5000.0, 5000.0];
        let second = state.create(None);
        let new = state.notes.iter().find(|n| n.id == second).unwrap();
        assert_eq!(new.position, FIRST_POSITION);

        state.rescue_lost();
        assert!(state.notes.iter().all(|n| n.position[0] < 1000.0));

        state.note_mut(first).unwrap().position = [5000.0, 5000.0];
        state.note_mut(first).unwrap().open = false;
        state.open(first);
        let reopened = state.notes.iter().find(|n| n.id == first).unwrap();
        assert_eq!(reopened.position, FIRST_POSITION);
    }

    #[test]
    fn search_ignores_case_and_recent_notes_come_first() {
        let mut state = State::default();
        for (text, updated) in [("Café", 1), ("thé", 3), ("CAFÉ noir", 2)] {
            let id = state.create(None);
            let note = state.note_mut(id).unwrap();
            note.text = text.into();
            note.updated = updated;
        }
        assert_eq!(state.listed(), vec![2, 3, 1]);
        state.search = "café".into();
        assert_eq!(state.listed(), vec![3, 1]);
    }

    #[test]
    fn preview_skips_blank_lines() {
        let mut state = State::default();
        let id = state.create(None);
        state.note_mut(id).unwrap().text = "\n  un \n\n deux\ntrois".into();
        assert_eq!(state.notes[0].preview(), ("un", "deux"));
    }

    #[test]
    fn where_a_window_lands_is_not_saved_but_where_it_is_moved_is() {
        let mut state = State::default();
        let id = state.create(None);
        state.note_mut(id).unwrap().position = [300.0, 200.0];
        let size = DEFAULT_SIZE;
        let start = Instant::now();
        // Created elsewhere, then asked to go to its place.
        assert_eq!(
            state.window_seen(id, [10.0, 10.0], size, start),
            Some([300.0, 200.0])
        );
        // The window manager puts it slightly off: not the user's doing.
        let soon = start + Duration::from_millis(100);
        assert_eq!(state.window_seen(id, [270.0, 153.0], size, soon), None);
        let later = start + Duration::from_secs(5);
        assert_eq!(state.window_seen(id, [270.0, 153.0], size, later), None);
        assert_eq!(state.notes[0].position, [300.0, 200.0]);
        // Dragged by the user.
        state.window_seen(id, [500.0, 400.0], size, later);
        assert_eq!(state.notes[0].position, [500.0, 400.0]);
    }

    #[test]
    fn a_late_first_frame_does_not_count_as_a_move() {
        let mut state = State::default();
        let id = state.create(None);
        state.note_mut(id).unwrap().position = [300.0, 200.0];
        let (size, start) = (DEFAULT_SIZE, Instant::now());
        state.window_seen(id, [10.0, 10.0], size, start);
        // The next frame comes long after the window landed, slightly off.
        let late = start + Duration::from_secs(3);
        assert_eq!(state.window_seen(id, [270.0, 153.0], size, late), None);
        let later = late + Duration::from_secs(1);
        state.window_seen(id, [270.0, 153.0], size, later);
        assert_eq!(state.notes[0].position, [300.0, 200.0]);
    }

    #[test]
    fn a_window_dragged_right_after_it_appeared_keeps_its_new_place() {
        let mut state = State::default();
        let id = state.create(None);
        let (size, start) = (DEFAULT_SIZE, Instant::now());
        state.window_seen(id, [10.0, 10.0], size, start);
        state.user_moves(id);
        state.window_seen(id, [700.0, 300.0], size, start);
        assert_eq!(state.notes[0].position, [700.0, 300.0]);
    }

    #[test]
    fn a_closing_window_leaves_no_trace_of_its_placement() {
        let mut state = State::default();
        let id = state.create(None);
        state.note_mut(id).unwrap().text = "x".into();
        let (size, start) = (DEFAULT_SIZE, Instant::now());
        state.window_seen(id, [10.0, 10.0], size, start);
        state.close(id);
        // One more pass before the window really goes.
        assert_eq!(state.window_seen(id, [10.0, 10.0], size, start), None);
        state.open(id);
        // Placed again when it comes back.
        assert!(state.window_seen(id, [1.0, 1.0], size, start).is_some());
    }

    #[test]
    fn bringing_back_moves_every_note_and_only_open_windows() {
        let mut state = State::default();
        let open = state.create(None);
        let closed = state.create(None);
        state.note_mut(open).unwrap().position = [9000.0, 9000.0];
        state.note_mut(closed).unwrap().position = [9000.0, 9000.0];
        state.note_mut(closed).unwrap().open = false;
        state.bring_back();
        assert!(state.relocate.contains_key(&open));
        assert!(!state.relocate.contains_key(&closed));
        assert!(state.notes.iter().all(|n| n.position[0] < 1000.0));
    }

    #[test]
    fn raising_gives_up_when_the_focus_never_comes() {
        let mut state = State::default();
        let id = state.create(None);
        assert_eq!(state.raise, Some(id));
        assert!(!state.raise_over(false));
        state.raise_until = Some(Instant::now() - Duration::from_millis(1));
        assert!(state.raise_over(false));
        assert_eq!(state.raise, None);
    }
}
