//! The list of notes: the main window, out of sight while notes are on the
//! desktop. Opens, finds and deletes notes, and holds the settings.

use eframe::egui::{
    self, Frame, Rect, RichText, Sense, TextEdit, ThemePreference, UiBuilder, vec2,
};
use egui_phosphor::regular as icon;

use crate::app::{APP_NAME, VERSION};
use crate::i18n::{self, Lang, t};
use crate::store::{Note, State, TextSize};
use crate::theme::{self, note_colors, palette};

const CARD_HEIGHT: f32 = 58.0;

pub fn ui(ui: &mut egui::Ui, state: &mut State) {
    let p = palette(ui);
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
                if ui.add(new).clicked() {
                    state.create(None);
                }
            });
        });
        ui.add_space(6.0);
        ui.add(
            TextEdit::singleline(&mut state.search)
                .hint_text(format!("{}  {}", icon::MAGNIFYING_GLASS, t!("list-search")))
                .desired_width(f32::INFINITY)
                .margin(vec2(10.0, 7.0)),
        );
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
        egui::ScrollArea::vertical()
            .auto_shrink(false)
            .show(ui, |ui| {
                for id in listed {
                    card(ui, state, id);
                }
            });
    });
}

fn card(ui: &mut egui::Ui, state: &mut State, id: u64) {
    let Some(note) = state.notes.iter().find(|n| n.id == id).cloned() else {
        return;
    };
    let colors = note_colors(note.color);
    let width = ui.available_width() - 8.0;
    let (rect, response) = ui.allocate_exact_size(vec2(width, CARD_HEIGHT), Sense::click());
    // A note that is off the desktop looks paler.
    let opacity = if note.open { 1.0 } else { 0.55 };
    let painter = ui.painter();
    painter.rect_filled(rect, 8.0, colors.paper.gamma_multiply(opacity));
    if response.hovered() {
        painter.rect_stroke(
            rect,
            8.0,
            egui::Stroke::new(1.5, colors.muted),
            egui::StrokeKind::Inside,
        );
    }

    if state.confirming_delete == Some(id) {
        confirm(ui, state, id, rect, colors.ink);
        return;
    }
    preview(ui, &note, rect, colors.ink, colors.muted);
    if response.clicked() {
        state.open(id);
        ui.ctx().request_repaint();
    }
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, note.preview().0)
    });

    if ui.rect_contains_pointer(rect) {
        let button = Rect::from_center_size(
            egui::pos2(rect.right() - 20.0, rect.center().y),
            egui::Vec2::splat(26.0),
        );
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
            state.confirming_delete = Some(id);
        }
    }
}

fn preview(ui: &egui::Ui, note: &Note, rect: Rect, ink: egui::Color32, muted: egui::Color32) {
    let (first, second) = note.preview();
    let inner = rect.shrink2(vec2(12.0, 9.0));
    let inner = Rect::from_min_max(inner.min, egui::pos2(inner.right() - 26.0, inner.bottom()));
    let (top, bottom) = inner.split_top_bottom_at_fraction(0.5);
    if first.is_empty() {
        let text = t!("note-empty");
        let middle = Rect::from_center_size(inner.center(), vec2(inner.width(), 20.0));
        theme::truncated_text(ui, middle, &text, theme::regular(13.0), muted);
    } else if second.is_empty() {
        let middle = Rect::from_center_size(inner.center(), vec2(inner.width(), 20.0));
        theme::truncated_text(ui, middle, first, theme::medium(13.0), ink);
    } else {
        theme::truncated_text(ui, top, first, theme::medium(13.0), ink);
        theme::truncated_text(ui, bottom, second, theme::regular(12.0), muted);
    }
}

fn confirm(ui: &mut egui::Ui, state: &mut State, id: u64, rect: Rect, ink: egui::Color32) {
    let mut row = ui.new_child(
        UiBuilder::new()
            .max_rect(rect.shrink2(vec2(12.0, 8.0)))
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    row.label(RichText::new(t!("delete-question")).color(ink));
    row.with_layout(egui::Layout::right_to_left(egui::Align::Center), |row| {
        let button = |text: String| {
            egui::Button::new(RichText::new(text).color(ink))
                .fill(egui::Color32::from_black_alpha(26))
                .stroke(egui::Stroke::NONE)
        };
        if row.add(button(t!("delete-yes"))).clicked() {
            state.delete(id);
        }
        if row.add(button(t!("delete-no"))).clicked() {
            state.confirming_delete = None;
        }
    });
}

fn settings_menu(ui: &mut egui::Ui, state: &mut State) {
    let glyph = RichText::new(icon::DOTS_THREE).font(theme::icons(18.0));
    ui.menu_button(glyph, |ui| {
        ui.menu_button(t!("menu-text-size"), |ui| {
            for (size, name) in [
                (TextSize::Small, t!("size-small")),
                (TextSize::Medium, t!("size-medium")),
                (TextSize::Large, t!("size-large")),
            ] {
                if ui.radio(state.text_size == size, name).clicked() {
                    state.text_size = size;
                    state.touch();
                    // The notes redraw with it.
                    ui.ctx().request_repaint();
                }
            }
        });
        ui.menu_button(t!("menu-language"), |ui| {
            for lang in Lang::ALL {
                if ui.radio(i18n::lang() == lang, lang.native_name()).clicked() {
                    i18n::choose(lang);
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
            ui.close();
        }
        ui.separator();
        ui.label(RichText::new(format!("{APP_NAME} {VERSION}")).weak());
        ui.hyperlink_to(t!("menu-website"), "https://shu-app.com/papillon/");
        ui.separator();
        if ui.button(t!("menu-quit")).clicked() {
            state.quit = true;
            ui.ctx().request_repaint();
        }
    });
}
