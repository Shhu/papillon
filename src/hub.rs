//! The list of notes: the main window, out of sight while notes are on the
//! desktop. Opens, finds and deletes notes, and holds the settings.

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
        // Only the cards in sight are built.
        ui.spacing_mut().item_spacing.y = CARD_GAP;
        egui::ScrollArea::vertical().auto_shrink(false).show_rows(
            ui,
            CARD_HEIGHT,
            listed.len(),
            |ui, range| {
                for &id in &listed[range] {
                    card(ui, state, id);
                }
            },
        );
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
    ui.add_space(6.0);
    Frame::new()
        .fill(error)
        .corner_radius(8)
        .inner_margin(10)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(message).color(egui::Color32::WHITE));
            if state.set_aside.is_some() && !state.protected && !state.save_failed {
                let dismiss = egui::Button::new(
                    RichText::new(t!("banner-dismiss")).color(egui::Color32::WHITE),
                )
                .fill(egui::Color32::from_white_alpha(40))
                .stroke(egui::Stroke::NONE);
                if ui.add(dismiss).clicked() {
                    state.set_aside = None;
                }
            }
        });
}

fn card(ui: &mut egui::Ui, state: &mut State, id: u64) {
    let Some(note) = state.notes.iter().find(|n| n.id == id) else {
        return;
    };
    let colors = note_colors(note.color);
    let open = note.open;
    let (first, second) = note.preview();
    let (first, second) = (first.to_owned(), second.to_owned());
    let width = ui.available_width() - 8.0;
    let (rect, response) = ui.allocate_exact_size(vec2(width, CARD_HEIGHT), Sense::click());
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

    if state.confirming_delete == Some(id) {
        match note::confirm_row(ui, rect.shrink2(vec2(12.0, 5.0)), colors.ink, colors.hover) {
            Some(true) => {
                state.delete(id);
                note::repaint_all(ui.ctx(), state);
            }
            Some(false) => state.confirming_delete = None,
            None => {}
        }
        return;
    }
    crate::fonts::ensure_for(ui.ctx(), &first);
    crate::fonts::ensure_for(ui.ctx(), &second);
    preview(ui, &first, &second, rect, colors.ink, colors.muted);

    let mut label = if first.is_empty() {
        t!("note-empty")
    } else {
        first
    };
    if !open {
        label = format!("{label} ({})", t!("note-closed"));
    }
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &label));
    if response.clicked() {
        state.open(id);
        ui.ctx().request_repaint();
    }
    if response.has_focus()
        && ui
            .ctx()
            .input_mut(|i| i.consume_key(Modifiers::NONE, Key::Delete))
    {
        state.request_delete(id);
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
    let mut child = ui.new_child(UiBuilder::new().max_rect(button));
    if theme::glyph_button(
        &mut child,
        icon::TRASH,
        26.0,
        colors.ink,
        colors.hover,
        false,
        &t!("note-delete"),
    )
    .clicked()
    {
        state.request_delete(id);
    }
}

fn preview(
    ui: &egui::Ui,
    first: &str,
    second: &str,
    rect: Rect,
    ink: egui::Color32,
    muted: egui::Color32,
) {
    let inner = rect.shrink2(vec2(12.0, 9.0));
    // Clear of the trash button and the mark beside it.
    let inner = Rect::from_min_max(inner.min, egui::pos2(inner.right() - 48.0, inner.bottom()));
    let (top, bottom) = inner.split_top_bottom_at_fraction(0.5);
    let middle = Rect::from_center_size(inner.center(), vec2(inner.width(), 20.0));
    let (first, second) = (
        crate::fonts::displayable(first),
        crate::fonts::displayable(second),
    );
    if first.is_empty() {
        theme::truncated_text(ui, middle, &t!("note-empty"), theme::regular(13.0), muted);
    } else if second.is_empty() {
        theme::truncated_text(ui, middle, &first, theme::medium(13.0), ink);
    } else {
        theme::truncated_text(ui, top, &first, theme::medium(13.0), ink);
        theme::truncated_text(ui, bottom, &second, theme::regular(12.0), muted);
    }
}

/// The full licenses, where the user can open them.
fn show_licenses(ui: &egui::Ui) {
    let file = crate::config::dir().join("licenses.txt");
    if std::fs::write(&file, crate::app::LICENSES).is_ok() {
        ui.ctx().open_url(egui::OpenUrl::new_tab(format!(
            "file:///{}",
            file.display()
                .to_string()
                .replace('\\', "/")
                .trim_start_matches('/')
        )));
    }
}

fn settings_menu(ui: &mut egui::Ui, state: &mut State) {
    let glyph = RichText::new(icon::DOTS_THREE).font(theme::icons(18.0));
    let menu = ui.menu_button(glyph, |ui| {
        ui.menu_button(t!("menu-text-size"), |ui| {
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
        ui.menu_button(t!("menu-language"), |ui| {
            for lang in Lang::ALL {
                if ui.radio(i18n::lang() == lang, lang.native_name()).clicked() {
                    i18n::choose(lang);
                    note::repaint_all(ui.ctx(), state);
                }
            }
        });
        ui.menu_button(t!("menu-theme"), |ui| {
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
        let mut autostart = state.autostart;
        if ui.checkbox(&mut autostart, t!("menu-autostart")).clicked()
            && crate::autostart::set(autostart)
        {
            state.autostart = autostart;
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
            show_licenses(ui);
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
