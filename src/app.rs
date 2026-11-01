//! The application: the main window holds the list of notes, each open note
//! is a window of its own. Deferred windows, so that typing in a note redraws
//! that note only, instead of every window one screen refresh after another.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use eframe::egui::text_edit::TextEditState;
use eframe::egui::{self, IconData, ViewportBuilder, ViewportCommand, ViewportId};

use crate::instance::{self, Instance};
use crate::note;
use crate::store::{self, MIN_SIZE, SAVE_DELAY, State};

pub const APP_NAME: &str = "Papillon";
pub const APP_ID: &str = "io.github.Shhu.Papillon";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
/// Of Papillon and everything built into it (packaging/third-party-licenses.py).
pub const LICENSES: &str = include_str!("../assets/licenses.txt");

/// Passes during which the main window is hidden again after launch: eframe
/// shows it after its first paint, whatever it was built with.
const HIDING_PASSES: u8 = 5;
/// How long the list insists on getting the focus when it shows.
const LIST_FOCUS: Duration = Duration::from_secs(1);

pub type Shared = Arc<Mutex<State>>;

/// Window and taskbar icon.
pub fn icon() -> Arc<IconData> {
    Arc::new(
        eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon.png"))
            .expect("valid icon PNG"),
    )
}

pub struct App {
    shared: Shared,
    icon: Arc<IconData>,
    hiding_passes: u8,
    list_focus_until: Option<Instant>,
    _instance: Instance,
}

impl App {
    pub fn new(ctx: &egui::Context, mut state: State, instance: Instance) -> Self {
        crate::autostart::refresh();
        state.autostart = crate::autostart::is_enabled();
        state.on_screen = Some(crate::screens::shows);
        state.rescue_lost();
        // The list also has to say when something is wrong with the notes.
        let problem = state.protected || state.set_aside.is_some();
        state.list_visible = problem || !state.notes.iter().any(|n| n.open);
        let hiding_passes = if state.list_visible { 0 } else { HIDING_PASSES };
        let woken = ctx.clone();
        state.on_write_failed = Some(Arc::new(move || {
            woken.request_repaint_of(ViewportId::ROOT);
        }));
        let shared: Shared = Arc::new(Mutex::new(state));
        let (watched, ctx) = (shared.clone(), ctx.clone());
        instance::watch(move || {
            store::lock(&watched).show_list = true;
            ctx.request_repaint_of(ViewportId::ROOT);
        });
        Self {
            shared,
            icon: icon(),
            hiding_passes,
            list_focus_until: None,
            _instance: instance,
        }
    }

    /// Declares the open notes' windows, which keeps them alive and creates
    /// the new ones.
    fn show_notes(&self, ctx: &egui::Context, state: &mut State) {
        for n in state.notes.iter().filter(|n| n.open) {
            let (position, size) = *state.spawned.entry(n.id).or_insert((n.position, n.size));
            let (id, shared) = (n.id, self.shared.clone());
            let builder = ViewportBuilder::default()
                .with_title(note::title(n))
                .with_icon(self.icon.clone())
                .with_app_id(APP_ID)
                .with_decorations(false)
                .with_position(position)
                .with_inner_size(size)
                .with_min_inner_size(MIN_SIZE)
                .with_window_level(note::level(n.pinned));
            ctx.show_viewport_deferred(note::viewport_id(id), builder, move |ui, _class| {
                note::window(ui, &shared, id);
            });
            if state.raise == Some(id) || state.relocate.contains_key(&id) {
                ctx.request_repaint_of(note::viewport_id(id));
            }
        }
        // A minimized window gets no pass of its own: it must be asked from
        // here, and in that order, since the system can't focus it minimized.
        for id in std::mem::take(&mut state.unminimize) {
            let viewport = note::viewport_id(id);
            ctx.send_viewport_cmd_to(viewport, ViewportCommand::Minimized(false));
            ctx.send_viewport_cmd_to(viewport, ViewportCommand::Focus);
        }
        for id in std::mem::take(&mut state.forgotten) {
            let text = egui::Id::new(("note-text", id));
            ctx.data_mut(|d| d.remove::<TextEditState>(text));
        }
    }

    fn show_list(&mut self, ctx: &egui::Context, state: &mut State) {
        if state.show_list {
            state.show_list = false;
            state.list_visible = true;
            self.hiding_passes = 0;
            self.list_focus_until = Some(Instant::now() + LIST_FOCUS);
            ctx.send_viewport_cmd(ViewportCommand::Minimized(false));
            ctx.send_viewport_cmd(ViewportCommand::Visible(true));
        }
        // Asked again: a window just shown can't take the focus yet.
        if let Some(until) = self.list_focus_until {
            let focused = ctx.input(|i| i.viewport().focused) == Some(true);
            if focused || Instant::now() >= until {
                self.list_focus_until = None;
            } else {
                ctx.send_viewport_cmd(ViewportCommand::Focus);
                ctx.request_repaint_after(Duration::from_millis(50));
            }
        }
    }
}

impl eframe::App for App {
    /// Runs even while the main window is hidden.
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let shared = self.shared.clone();
        let mut guard = store::lock(&shared);
        let state = &mut *guard;
        if state.quit {
            // Not without saying so, if the notes can't be saved.
            if state.save() || state.quit_anyway {
                ctx.send_viewport_cmd(ViewportCommand::Close);
                return;
            }
            state.quit = false;
            state.quit_anyway = true;
            state.show_list = true;
        }
        self.show_list(ctx, state);
        if self.hiding_passes > 0 {
            self.hiding_passes -= 1;
            ctx.send_viewport_cmd(ViewportCommand::Visible(false));
            ctx.request_repaint_after(Duration::from_millis(50));
        }
        self.show_notes(ctx, state);
        state.save_if_due();
        if state.has_unsaved() {
            ctx.request_repaint_after(SAVE_DELAY);
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let shared = self.shared.clone();
        let mut state = store::lock(&shared);
        if ui.ctx().input(|i| i.viewport().close_requested()) && !state.quit {
            // With notes on the desktop, closing the list only hides it.
            if state.notes.iter().any(|n| n.open) {
                ui.ctx().send_viewport_cmd(ViewportCommand::CancelClose);
                ui.ctx().send_viewport_cmd(ViewportCommand::Visible(false));
                state.list_visible = false;
            }
        }
        // This pass also runs while hidden, to keep the notes' windows going.
        if state.list_visible {
            crate::hub::ui(ui, &mut state);
        }
    }

    fn on_exit(&mut self) {
        store::lock(&self.shared).save();
    }
}

#[cfg(test)]
mod tests {
    use eframe::egui::{Rect, UiBuilder, pos2, vec2};

    use super::*;
    use egui_kittest::kittest::Queryable;

    use crate::i18n::t;
    use crate::store::{Color, TextSize};
    use crate::{hub, theme};

    /// A note's window content, driven like a user would.
    fn note_harness(text: &str) -> egui_kittest::Harness<'static, State> {
        let mut state = State::default();
        let id = state.create(None);
        state.note_mut(id).expect("just created").text = text.into();
        state.raise = None;
        let mut fonts_ready = false;
        let mut harness = egui_kittest::Harness::builder()
            .with_size([300.0, 300.0])
            .build_ui_state(
                move |ui, state: &mut State| {
                    if !fonts_ready {
                        theme::install(ui.ctx());
                        fonts_ready = true;
                        return;
                    }
                    note::body(ui, state, id);
                },
                state,
            );
        harness.run_steps(3);
        harness
    }

    #[test]
    fn deleting_a_written_note_asks_first() {
        let mut harness = note_harness("Ne pas perdre");
        harness.get_by_label(&t!("note-delete")).click();
        harness.run_steps(2);
        assert_eq!(harness.state().notes.len(), 1);
        harness.get_by_label(&t!("delete-no")).click();
        harness.run_steps(2);
        assert_eq!(harness.state().notes.len(), 1);
        assert_eq!(harness.state().confirming_delete, None);
        harness.get_by_label(&t!("note-delete")).click();
        harness.run_steps(2);
        harness.get_by_label(&t!("delete-yes")).click();
        harness.run_steps(2);
        assert!(harness.state().notes.is_empty());
    }

    #[test]
    fn deleting_an_empty_note_does_not_ask() {
        let mut harness = note_harness("");
        harness.get_by_label(&t!("note-delete")).click();
        harness.run_steps(2);
        assert!(harness.state().notes.is_empty());
    }

    #[test]
    fn colors_are_named_and_change_the_note() {
        let mut harness = note_harness("x");
        harness.get_by_label(&t!("note-color")).click();
        harness.run_steps(2);
        harness.get_by_label(&t!("color-pink")).click();
        harness.run_steps(2);
        assert_eq!(harness.state().notes[0].color, Color::Pink);
        assert_eq!(harness.state().choosing_color, None);
    }

    #[test]
    fn pinning_keeps_the_note_on_top() {
        let mut harness = note_harness("x");
        harness.get_by_label(&t!("note-pin")).click();
        harness.run_steps(2);
        assert!(harness.state().notes[0].pinned);
        harness.get_by_label(&t!("note-unpin")).click();
        harness.run_steps(2);
        assert!(!harness.state().notes[0].pinned);
    }

    #[test]
    fn the_new_note_button_adds_a_note() {
        let mut harness = note_harness("x");
        harness.get_by_label(&t!("note-new")).click();
        harness.run_steps(2);
        assert_eq!(harness.state().notes.len(), 2);
    }

    fn type_in_note(harness: &mut egui_kittest::Harness<'static, State>, text: &str) {
        harness
            .get_by_role(egui::accesskit::Role::MultilineTextInput)
            .focus();
        harness.run_steps(2);
        harness
            .get_by_role(egui::accesskit::Role::MultilineTextInput)
            .type_text(text);
        harness.run_steps(2);
    }

    #[test]
    fn typing_goes_into_the_note_and_marks_it_changed() {
        let mut harness = note_harness("");
        let revision = harness.state().revision();
        type_in_note(&mut harness, "Bonjour");
        assert_eq!(harness.state().notes[0].text, "Bonjour");
        assert!(harness.state().revision() > revision);
    }

    #[test]
    fn typing_goes_on_after_a_band_button() {
        let mut harness = note_harness("");
        type_in_note(&mut harness, "abc");
        harness.get_by_label(&t!("note-pin")).click();
        harness.run_steps(3);
        harness
            .get_by_role(egui::accesskit::Role::MultilineTextInput)
            .type_text("d");
        harness.run_steps(2);
        assert_eq!(harness.state().notes[0].text, "abcd");
    }

    /// The list with some notes.
    fn list_harness(texts: &[&str]) -> egui_kittest::Harness<'static, State> {
        let mut state = State::default();
        for text in texts {
            let id = state.create(None);
            state.note_mut(id).expect("just created").text = (*text).into();
        }
        state.raise = None;
        let mut fonts_ready = false;
        let mut harness = egui_kittest::Harness::builder()
            .with_size([360.0, 540.0])
            .build_ui_state(
                move |ui, state: &mut State| {
                    if !fonts_ready {
                        theme::install(ui.ctx());
                        fonts_ready = true;
                        return;
                    }
                    hub::ui(ui, state);
                },
                state,
            );
        harness.run_steps(3);
        harness
    }

    #[test]
    fn a_second_click_on_the_list_trash_does_not_delete() {
        let mut harness = list_harness(&["Ne pas perdre"]);
        let trash = harness.get_by_label(&t!("note-delete")).rect().center();
        harness.get_by_label(&t!("note-delete")).click();
        harness.run_steps(2);
        assert_eq!(harness.state().confirming_delete, Some(1));
        let keep = harness.get_by_label(&t!("delete-no")).rect();
        let delete = harness.get_by_label(&t!("delete-yes")).rect();
        assert!(!delete.contains(trash), "{delete:?} under {trash:?}");
        assert!(keep.left() > delete.right() - 1.0);
        harness.get_by_label(&t!("delete-yes")).click();
        harness.run_steps(2);
        assert!(harness.state().notes.is_empty());
    }

    #[test]
    fn empty_notes_go_at_once_from_the_list_too() {
        let mut harness = list_harness(&[""]);
        harness.get_by_label(&t!("note-delete")).click();
        harness.run_steps(2);
        assert!(harness.state().notes.is_empty());
    }

    #[test]
    fn a_card_reopens_its_note() {
        let mut harness = list_harness(&["Courses"]);
        harness.state_mut().notes[0].open = false;
        harness.run_steps(2);
        let label = format!("Courses ({})", t!("note-closed"));
        harness.get_by_label(&label).click();
        harness.run_steps(2);
        assert!(harness.state().notes[0].open);
        assert_eq!(harness.state().raise, Some(1));
    }

    #[test]
    fn the_list_names_its_controls_and_says_what_is_wrong() {
        let mut harness = list_harness(&["x"]);
        harness.get_by_label(&t!("menu-settings"));
        harness.get_by_label(&t!("list-new"));
        harness.get_by_label(&t!("list-search"));
        harness.state_mut().save_failed = true;
        harness.run_steps(2);
        harness.get_by_label(&t!("banner-save-failed"));
    }

    #[test]
    fn hidden_text_styles_do_not_move_the_text() {
        let mut harness = note_harness("a\u{FE0F}b");
        type_in_note(&mut harness, "c");
        assert!(harness.state().notes[0].text.contains('\u{FE0F}'));
    }

    /// A desktop with notes and the list, made of the real pieces, for the
    /// website: `cargo test capture -- --ignored`, on its own since it changes
    /// the language of the whole process. `PAPILLON_CAPTURE` names
    /// the file, `PAPILLON_LANG` the language (en, fr), `PAPILLON_THEME` the
    /// list's theme (light, dark).
    #[test]
    #[ignore = "needs a graphics renderer"]
    fn capture() {
        let out = std::env::var("PAPILLON_CAPTURE").unwrap_or_else(|_| "capture.png".into());
        let french = std::env::var("PAPILLON_LANG").as_deref() == Ok("fr");
        crate::i18n::choose(if french {
            crate::i18n::Lang::Fr
        } else {
            crate::i18n::Lang::En
        });
        type DemoNote<'a> = (&'a str, &'a str, Color, [f32; 2], [f32; 2]);
        let notes: [DemoNote; 4] = [
            (
                "Courses\n- café\n- citrons\n- papier pour l'imprimante\n- timbres",
                "Groceries\n- coffee\n- lemons\n- printer paper\n- stamps",
                Color::Yellow,
                [60.0, 50.0],
                [270.0, 250.0],
            ),
            (
                "Appeler le garage avant 17 h\n\n02 40 00 00 00",
                "Call the garage before 5 pm\n\n555 0100",
                Color::Green,
                [370.0, 90.0],
                [270.0, 170.0],
            ),
            (
                "Idée pour le week-end :\nle sentier côtier, départ tôt, pique-nique à la pointe.",
                "Weekend idea:\nthe coastal path, early start, picnic at the point.",
                Color::Pink,
                [120.0, 340.0],
                [290.0, 180.0],
            ),
            (
                "Le mot de passe du wifi est derrière la box.",
                "The wifi password is on the back of the router.",
                Color::Charcoal,
                [440.0, 310.0],
                [240.0, 140.0],
            ),
        ];
        let theme = if std::env::var("PAPILLON_THEME").as_deref() == Ok("light") {
            egui::Theme::Light
        } else {
            egui::Theme::Dark
        };
        let mut state = State::default();
        state.text_size = TextSize::Medium;
        for (i, (fr, en, color, position, size)) in notes.into_iter().enumerate() {
            let id = state.create(None);
            let note = state.note_mut(id).expect("just created");
            note.text = if french { fr } else { en }.into();
            note.color = color;
            note.position = position;
            note.size = size;
            note.updated = i as u64;
            note.pinned = i == 1;
        }
        state.raise = None;
        let mut fonts_ready = false;
        let mut harness = egui_kittest::Harness::builder()
            .with_size([1160.0, 620.0])
            .with_theme(theme)
            .wgpu()
            .build_ui(move |ui| {
                // Fonts take effect at the next pass.
                if !fonts_ready {
                    theme::install(ui.ctx());
                    ui.ctx().set_theme(theme);
                    fonts_ready = true;
                    return;
                }
                let screen = ui.max_rect();
                ui.painter()
                    .rect_filled(screen, 0.0, egui::Color32::from_rgb(0x2E, 0x4A, 0x5E));
                let ids: Vec<u64> = state.notes.iter().map(|n| n.id).collect();
                for id in ids {
                    let note = state.notes.iter().find(|n| n.id == id).expect("note");
                    let rect = Rect::from_min_size(
                        pos2(note.position[0], note.position[1]),
                        vec2(note.size[0], note.size[1]),
                    );
                    let mut child = ui.new_child(UiBuilder::new().id_salt(id).max_rect(rect));
                    child.set_clip_rect(rect);
                    note::body(&mut child, &mut state, id);
                }
                let list = Rect::from_min_size(pos2(760.0, 40.0), vec2(360.0, 540.0));
                let mut child = ui.new_child(UiBuilder::new().id_salt("list").max_rect(list));
                child.set_clip_rect(list);
                hub::ui(&mut child, &mut state);
            });
        harness.run_steps(4);
        let image = harness.render().expect("render");
        image.save(&out).expect("writing the capture");
    }
}
