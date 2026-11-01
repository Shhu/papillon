//! The notes and their file. Saved shortly after each change, by replacing
//! the file in one step, so that a crash or a power cut leaves either the old
//! version or the new one, never half of it.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// How long after a change the notes are written.
pub const SAVE_DELAY: Duration = Duration::from_secs(1);

pub const DEFAULT_SIZE: [f32; 2] = [280.0, 280.0];
pub const MIN_SIZE: [f32; 2] = [170.0, 120.0];
/// How long a window takes to land where it was asked to be.
const SETTLING: Duration = Duration::from_millis(500);
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
#[derive(Debug, Default, PartialEq, Serialize, Deserialize)]
struct Saved {
    #[serde(default)]
    notes: Vec<Note>,
    #[serde(default)]
    text_size: TextSize,
}

/// Everything the windows share.
#[derive(Default)]
pub struct State {
    pub notes: Vec<Note>,
    pub text_size: TextSize,
    /// The note to bring to the front, its text ready for typing.
    pub raise: Option<u64>,
    /// Notes to move back on screen: their new position.
    pub relocate: HashMap<u64, [f32; 2]>,
    /// The note whose color choice is unfolded.
    pub choosing_color: Option<u64>,
    /// The note asking whether to delete it.
    pub confirming_delete: Option<u64>,
    /// Position and size each open window was created with: changing them
    /// in its description would move it back there.
    pub spawned: HashMap<u64, ([f32; 2], [f32; 2])>,
    /// Open windows already placed: until when they may still be landing
    /// (None once the user took over), and where they were last seen.
    placed: HashMap<u64, (Option<Instant>, [f32; 2])>,
    pub show_list: bool,
    pub list_visible: bool,
    pub quit: bool,
    pub search: String,
    pub autostart: bool,
    file: PathBuf,
    /// The last change not written yet.
    changed: Option<Instant>,
}

/// The notes of the usual file.
pub fn load() -> State {
    load_from(crate::config::dir().join("notes.json"))
}

fn load_from(file: PathBuf) -> State {
    let saved = match std::fs::read_to_string(&file) {
        Ok(content) => serde_json::from_str::<Saved>(&content).unwrap_or_else(|_| {
            // Never overwrite what can't be read: someone may rescue it.
            let _ = std::fs::rename(&file, broken_name(&file));
            Saved::default()
        }),
        Err(_) => Saved::default(),
    };
    State {
        notes: saved.notes,
        text_size: saved.text_size,
        file,
        ..Default::default()
    }
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
        self.changed = Some(Instant::now());
    }

    /// The text of a note changed.
    pub fn text_changed(&mut self, id: u64) {
        if let Some(note) = self.note_mut(id) {
            note.updated = now();
        }
        self.touch();
    }

    /// A new note, a little beside the note `near` if any, otherwise beside
    /// the newest one.
    pub fn create(&mut self, near: Option<u64>) -> u64 {
        let id = self.notes.iter().map(|n| n.id).max().unwrap_or(0) + 1;
        let beside = near
            .and_then(|near| self.notes.iter().find(|n| n.id == near))
            .or_else(|| self.notes.iter().rfind(|n| n.open));
        let position = beside.map_or(FIRST_POSITION, |n| {
            [n.position[0] + CASCADE, n.position[1] + CASCADE]
        });
        self.notes.push(Note {
            id,
            text: String::new(),
            color: beside.map_or(Color::default(), |n| n.color),
            position,
            size: DEFAULT_SIZE,
            pinned: false,
            open: true,
            updated: now(),
        });
        self.raise = Some(id);
        self.touch();
        id
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

    fn window_gone(&mut self, id: u64) {
        self.spawned.remove(&id);
        self.placed.remove(&id);
        self.relocate.remove(&id);
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
        if let Some(note) = self.note_mut(id) {
            note.open = true;
        }
        self.raise = Some(id);
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
            }
        }
        self.touch();
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
        let Some((settling, seen)) = self.placed.get_mut(&id) else {
            self.placed.insert(id, (Some(now + SETTLING), position));
            return self.notes.iter().find(|n| n.id == id).map(|n| n.position);
        };
        if settling.is_some_and(|until| now < until) {
            *seen = position;
            return None;
        }
        *settling = None;
        let moved = *seen != position;
        *seen = position;
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

    /// The user is moving this window: what follows counts.
    pub fn user_moves(&mut self, id: u64) {
        if let Some((settling, _)) = self.placed.get_mut(&id) {
            *settling = None;
        }
    }

    pub fn has_unsaved(&self) -> bool {
        self.changed.is_some()
    }

    /// Writes the notes if the last change is old enough.
    pub fn save_if_due(&mut self) {
        if self.changed.is_some_and(|t| t.elapsed() >= SAVE_DELAY) {
            self.save();
        }
    }

    /// Writes the notes now. On failure they stay marked as changed, and the
    /// next attempt comes with the next delay.
    pub fn save(&mut self) {
        if self.write().is_ok() {
            self.changed = None;
        } else {
            self.changed = Some(Instant::now());
        }
    }

    fn write(&self) -> std::io::Result<()> {
        // Field by field rather than through `Saved`, which would have to own
        // or copy every note.
        #[derive(Serialize)]
        struct Saving<'a> {
            notes: &'a [Note],
            text_size: TextSize,
        }
        let content = serde_json::to_vec_pretty(&Saving {
            notes: &self.notes,
            text_size: self.text_size,
        })?;
        if let Some(dir) = self.file.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let temporary = self.file.with_extension("json.tmp");
        {
            use std::io::Write as _;
            let mut file = std::fs::File::create(&temporary)?;
            file.write_all(&content)?;
            file.sync_all()?;
        }
        std::fs::rename(temporary, &self.file)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_file(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("papillon-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join(format!("{name}.json"));
        let _ = std::fs::remove_file(&file);
        file
    }

    #[test]
    fn notes_come_back_as_they_were_saved() {
        let file = temp_file("round-trip");
        let mut state = load_from(file.clone());
        let id = state.create(None);
        let note = state.note_mut(id).unwrap();
        note.text = "Acheter du café\n« vite »".into();
        note.color = Color::Pink;
        note.pinned = true;
        state.text_size = TextSize::Large;
        state.save();
        assert!(!state.has_unsaved());
        let again = load_from(file);
        assert_eq!(again.notes, state.notes);
        assert_eq!(again.text_size, TextSize::Large);
    }

    #[test]
    fn an_unreadable_file_is_set_aside_not_overwritten() {
        let file = temp_file("broken");
        std::fs::write(&file, "{ not json").unwrap();
        let mut state = load_from(file.clone());
        assert!(state.notes.is_empty());
        state.create(None);
        state.save();
        let kept = std::fs::read_dir(file.parent().unwrap())
            .unwrap()
            .filter_map(Result::ok)
            .any(|e| {
                let name = e.file_name().to_string_lossy().into_owned();
                name.starts_with("broken.broken-")
                    && std::fs::read_to_string(e.path()).unwrap() == "{ not json"
            });
        assert!(kept);
    }

    #[test]
    fn fields_added_later_have_defaults() {
        let file = temp_file("old");
        std::fs::write(
            &file,
            r#"{"notes":[{"id":3,"text":"a","position":[1,2],"size":[3,4]}]}"#,
        )
        .unwrap();
        let state = load_from(file);
        assert!(state.notes[0].open);
        assert_eq!(state.notes[0].color, Color::Yellow);
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
    fn bringing_back_moves_open_windows_only() {
        let mut state = State::default();
        let open = state.create(None);
        let closed = state.create(None);
        state.note_mut(open).unwrap().position = [9000.0, 9000.0];
        state.note_mut(closed).unwrap().position = [9000.0, 9000.0];
        state.note_mut(closed).unwrap().open = false;
        state.bring_back();
        assert_eq!(state.relocate.len(), 1);
        assert!(state.notes.iter().all(|n| n.position[0] < 1000.0));
    }
}
