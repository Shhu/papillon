//! The list of notes: the main window, out of sight while notes are on the
//! desktop. Opens, finds and deletes notes, and holds the settings.

use std::sync::LazyLock;

use eframe::egui::{
    self, Frame, Key, KeyboardShortcut, Modifiers, Rect, RichText, Sense, TextEdit,
    ThemePreference, UiBuilder, vec2,
};
use egui_phosphor::regular as icon;

use crate::app::{APP_NAME, VERSION};
use crate::i18n::{self, Lang, t};
use crate::note;
use crate::store::{State, TextSize};
use crate::theme::{self, note_colors, palette};

const CARD_HEIGHT: f32 = 58.0;
const CARD_GAP: f32 = 6.0;

const SHORTCUT_NEW: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::N);

pub fn ui(ui: &mut egui::Ui, state: &mut State) {
    let p = palette(ui);
    if ui.ctx().input_mut(|i| i.consume_shortcut(&SHORTCUT_NEW)) {
        state.create(None);
    }
    if state.show_licenses {
        licenses(ui, state);
        return;
    }
    Frame::new().fill(p.bg).inner_margin(14).show(ui, |ui| {
        ui.set_min_size(ui.available_size());
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(APP_NAME)
                    .font(theme::semibold(17.0))
                    .color(p.text),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                settings_menu(ui, state);
                let new = egui::Button::new(
                    RichText::new(format!("{}  {}", icon::PLUS, t!("list-new")))
                        .font(theme::medium(13.0))
                        .color(p.on_accent),
                )
                .fill(p.accent)
                .stroke(egui::Stroke::NONE);
                let response = ui.add(new);
                theme::focus_ring(ui, response.rect, &response, p.on_accent);
                name(ui, &response, t!("list-new"));
                if response.clicked() {
                    state.create(None);
                }
            });
        });
        banners(ui, state);
        ui.add_space(6.0);
        let search = ui.add(
            TextEdit::singleline(&mut state.search)
                .hint_text(format!("{}  {}", icon::MAGNIFYING_GLASS, t!("list-search")))
                .desired_width(f32::INFINITY)
                .margin(vec2(10.0, 7.0)),
        );
        name(ui, &search, t!("list-search"));
        ui.ctx().accesskit_node_builder(search.id, |node| {
            node.clear_placeholder();
        });
        crate::fonts::ensure_for(ui.ctx(), &state.search);
        ui.add_space(6.0);

        let listed = state.listed();
        if listed.is_empty() {
            ui.add_space(30.0);
            let message = if state.notes.is_empty() {
                t!("list-empty")
            } else {
                t!("list-no-match")
            };
            ui.vertical_centered(|ui| ui.label(RichText::new(message).color(p.muted)));
            return;
        }
        // All of them, even out of sight: the Tab key can only reach what's built.
        ui.spacing_mut().item_spacing.y = CARD_GAP;
        egui::ScrollArea::vertical()
            .auto_shrink(false)
            .show(ui, |ui| {
                for (i, &id) in listed.iter().enumerate() {
                    let next = listed
                        .get(i + 1)
                        .or(i.checked_sub(1).and_then(|p| listed.get(p)));
                    card(ui, state, id, next.copied());
                }
            });
    });
}

/// The name for screen readers, without the icons in the visible text.
fn name(ui: &egui::Ui, response: &egui::Response, name: String) {
    ui.ctx().accesskit_node_builder(response.id, |node| {
        node.set_label(name);
    });
}

/// What the user must know about their notes file.
fn banners(ui: &mut egui::Ui, state: &mut State) {
    let error = egui::Color32::from_rgb(0xC6, 0x28, 0x28);
    let message = if state.protected {
        Some(t!("banner-unreadable"))
    } else if state.save_failed {
        Some(t!("banner-save-failed"))
    } else {
        state
            .set_aside
            .as_ref()
            .map(|path| t!("banner-set-aside", path = path.display().to_string()))
    };
    let Some(message) = message else {
        return;
    };
    crate::fonts::ensure_for(ui.ctx(), &message);
    ui.add_space(6.0);
    Frame::new()
        .fill(error)
        .corner_radius(8)
        .inner_margin(10)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            // Selected, to copy the path: still legible on this background.
            let selection = &mut ui.visuals_mut().selection;
            selection.bg_fill = egui::Color32::from_black_alpha(120);
            selection.stroke.color = egui::Color32::WHITE;
            let label = ui.label(RichText::new(message).color(egui::Color32::WHITE));
            // Announced by screen readers as soon as it shows.
            ui.ctx().accesskit_node_builder(label.id, |node| {
                node.set_live(egui::accesskit::Live::Polite);
            });
            if state.set_aside.is_some() && !state.protected && !state.save_failed {
                let dismiss = egui::Button::new(
                    RichText::new(t!("banner-dismiss")).color(egui::Color32::WHITE),
                )
                .fill(egui::Color32::from_white_alpha(30))
                .stroke(egui::Stroke::NONE);
                let dismiss = ui.add(dismiss);
                theme::focus_ring(ui, dismiss.rect, &dismiss, egui::Color32::WHITE);
                if dismiss.clicked() {
                    state.set_aside = None;
                }
            }
        });
}

/// A preview needs no more: layout time grows with a line's length.
const PREVIEW_CHARS: usize = 200;

/// `next` is the card to go to if this one goes.
fn card(ui: &mut egui::Ui, state: &mut State, id: u64, next: Option<u64>) {
    let Some(note) = state.notes.iter().find(|n| n.id == id) else {
        return;
    };
    let colors = note_colors(note.color);
    let open = note.open;
    let (first, second) = note.preview();
    let [first, second] =
        [first, second].map(|l| l.chars().take(PREVIEW_CHARS).collect::<String>());
    let width = ui.available_width() - 8.0;
    let (rect, _) = ui.allocate_exact_size(vec2(width, CARD_HEIGHT), Sense::hover());
    // Named after the note and not its row: the keyboard's focus stays on the
    // note when a search or scrolling moves the rows.
    let response = ui.interact(rect, egui::Id::new(("card", id)), Sense::click());
    let painter = ui.painter();
    painter.rect_filled(rect, 8.0, colors.paper);
    if response.hovered() {
        painter.rect_stroke(
            rect,
            8.0,
            egui::Stroke::new(1.5, colors.muted),
            egui::StrokeKind::Inside,
        );
    }
    theme::focus_ring(ui, rect, &response, colors.ink);
    if response.gained_focus() {
        response.scroll_to_me(None);
    }
    if state.refocus_card.take_if(|c| *c == id).is_some() {
        response.request_focus();
    }

    let mut label = if first.is_empty() {
        t!("note-empty")
    } else {
        first.clone()
    };
    if !open {
        label = format!("{label} ({})", t!("note-closed"));
    }
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &label));

    if state.confirming_delete == Some(id) {
        let inside = rect.shrink2(vec2(12.0, 5.0));
        let focus_keep = std::mem::take(&mut state.confirm_fresh);
        match note::confirm_row(ui, inside, colors.ink, colors.hover, focus_keep) {
            Some(true) => {
                state.delete(id);
                state.refocus_card = next;
            }
            Some(false) => {
                state.confirming_delete = None;
                state.refocus_card = Some(id);
            }
            None => {}
        }
        return;
    }
    crate::fonts::ensure_for(ui.ctx(), &first);
    crate::fonts::ensure_for(ui.ctx(), &second);
    let base = state.text_size.points() - 2.0;
    preview(ui, &first, &second, rect, base, colors.ink, colors.muted);

    if response.clicked() {
        state.open(id);
        ui.ctx().request_repaint();
    }
    let delete_key = response.has_focus()
        && ui
            .ctx()
            .input_mut(|i| i.consume_key(Modifiers::NONE, Key::Delete));
    if delete_key && state.request_delete(id) {
        state.refocus_card = next;
    }

    let button = Rect::from_center_size(
        egui::pos2(rect.right() - 20.0, rect.center().y),
        egui::Vec2::splat(26.0),
    );
    if !open {
        // Off the desktop: said by a mark, a paler note would be hard to read.
        let mark = button.center() - vec2(26.0, 0.0);
        ui.painter().text(
            mark,
            egui::Align2::CENTER_CENTER,
            icon::EYE_SLASH,
            theme::icons(15.0),
            colors.muted,
        );
    }
    let mut child = ui.new_child(UiBuilder::new().id_salt(("trash", id)).max_rect(button));
    let tip = format!("{} {label}", t!("note-delete"));
    let trash = theme::glyph_button(
        &mut child,
        icon::TRASH,
        26.0,
        colors.ink,
        colors.hover,
        false,
        &tip,
    );
    if trash.gained_focus() {
        response.scroll_to_me(None);
    }
    if trash.clicked() && state.request_delete(id) {
        state.refocus_card = next;
    }
}

fn preview(
    ui: &egui::Ui,
    first: &str,
    second: &str,
    rect: Rect,
    size: f32,
    ink: egui::Color32,
    muted: egui::Color32,
) {
    let inner = rect.shrink2(vec2(12.0, 9.0));
    // Clear of the trash button and the mark beside it.
    let inner = Rect::from_min_max(inner.min, egui::pos2(inner.right() - 48.0, inner.bottom()));
    let (top, bottom) = inner.split_top_bottom_at_fraction(0.5);
    let middle = Rect::from_center_size(inner.center(), vec2(inner.width(), 20.0));
    if first.is_empty() {
        theme::truncated_text(ui, middle, &t!("note-empty"), theme::regular(size), muted);
    } else if second.is_empty() {
        theme::truncated_text(ui, middle, first, theme::medium(size), ink);
    } else {
        theme::truncated_text(ui, top, first, theme::medium(size), ink);
        theme::truncated_text(ui, bottom, second, theme::regular(size - 1.0), muted);
    }
}

/// The license texts, line by line: only those in sight are laid out.
fn licenses(ui: &mut egui::Ui, state: &mut State) {
    static LINES: LazyLock<Vec<&str>> = LazyLock::new(|| crate::app::LICENSES.lines().collect());
    let p = palette(ui);
    Frame::new().fill(p.bg).inner_margin(14).show(ui, |ui| {
        ui.set_min_size(ui.available_size());
        let close_key = ui
            .ctx()
            .input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape));
        if ui.button(t!("licenses-back")).clicked() || close_key {
            state.show_licenses = false;
        }
        ui.add_space(6.0);
        let height = ui.text_style_height(&egui::TextStyle::Small) + 2.0;
        let page = (ui.available_height() - height).max(height);
        let everything = height * LINES.len() as f32;
        let delta = ui.ctx().input_mut(|i| {
            let mut delta = 0.0;
            for (key, step) in [
                (Key::ArrowUp, height * 3.0),
                (Key::ArrowDown, -height * 3.0),
                (Key::PageUp, page),
                (Key::PageDown, -page),
                (Key::Home, everything),
                (Key::End, -everything),
            ] {
                if i.consume_key(Modifiers::NONE, key) {
                    delta += step;
                }
            }
            delta
        });
        ui.spacing_mut().item_spacing.y = 2.0;
        egui::ScrollArea::both().auto_shrink(false).show_rows(
            ui,
            height,
            LINES.len(),
            |ui, range| {
                if delta != 0.0 {
                    ui.scroll_with_delta(vec2(0.0, delta));
                }
                for line in &LINES[range] {
                    ui.add(
                        egui::Label::new(RichText::new(*line).small().color(p.text))
                            .extend()
                            .selectable(false),
                    );
                }
            },
        );
    });
}

fn settings_menu(ui: &mut egui::Ui, state: &mut State) {
    let glyph = RichText::new(icon::DOTS_THREE).font(theme::icons(18.0));
    let menu = ui.menu_button(glyph, |ui| {
        let size_menu = ui.menu_button(t!("menu-text-size"), |ui| {
            for (size, name) in [
                (TextSize::Small, t!("size-small")),
                (TextSize::Medium, t!("size-medium")),
                (TextSize::Large, t!("size-large")),
            ] {
                if ui.radio(state.text_size == size, name).clicked() {
                    state.text_size = size;
                    state.touch();
                    note::repaint_all(ui.ctx(), state);
                }
            }
        });
        name(ui, &size_menu.response, t!("menu-text-size"));
        let language_menu = ui.menu_button(t!("menu-language"), |ui| {
            for lang in Lang::ALL {
                if ui.radio(i18n::lang() == lang, lang.native_name()).clicked() {
                    i18n::choose(lang);
                    note::repaint_all(ui.ctx(), state);
                }
            }
        });
        name(ui, &language_menu.response, t!("menu-language"));
        let theme_menu = ui.menu_button(t!("menu-theme"), |ui| {
            let current = ui.ctx().options(|o| o.theme_preference);
            for (theme, name) in [
                (ThemePreference::System, t!("theme-system")),
                (ThemePreference::Dark, t!("theme-dark")),
                (ThemePreference::Light, t!("theme-light")),
            ] {
                if ui.radio(current == theme, name).clicked() {
                    theme::choose_theme(ui.ctx(), theme);
                }
            }
        });
        name(ui, &theme_menu.response, t!("menu-theme"));
        let mut autostart = state.autostart;
        if ui.checkbox(&mut autostart, t!("menu-autostart")).clicked() {
            state.autostart = autostart;
            crate::autostart::set_in_background(ui.ctx(), autostart);
        }
        if ui.button(t!("menu-bring-back")).clicked() {
            state.bring_back();
            note::repaint_all(ui.ctx(), state);
            ui.close();
        }
        ui.separator();
        ui.label(RichText::new(format!("{APP_NAME} {VERSION}")).weak());
        ui.hyperlink_to(t!("menu-website"), "https://shu-app.com/papillon/");
        if ui.button(t!("menu-licenses")).clicked() {
            state.show_licenses = true;
            ui.close();
        }
        ui.separator();
        if ui.button(t!("menu-quit")).clicked() {
            state.quit = true;
            ui.ctx().request_repaint();
        }
    });
    name(ui, &menu.response, t!("menu-settings"));
    menu.response.on_hover_text(t!("menu-settings"));
}
