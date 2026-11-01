//! A note: its own window without a system frame, a strip at the top that
//! moves it, its borders that resize it, and its text.

use std::time::Instant;

use eframe::egui::viewport::ResizeDirection;
use eframe::egui::{
    self, Color32, CursorIcon, Frame, Key, KeyboardShortcut, Modifiers, Pos2, Rect, RichText,
    Sense, Stroke, TextEdit, UiBuilder, ViewportCommand, ViewportId, WindowLevel, pos2, vec2,
};
use egui_phosphor::regular as icon;

use crate::app::Shared;
use crate::i18n::t;
use crate::store::{Color, Note, SAVE_DELAY, State};
use crate::theme::{self, NoteColors, note_colors};

const BAND_HEIGHT: f32 = 30.0;
const BUTTON: f32 = 26.0;
/// Width of the window border that resizes it.
const RESIZE_BORDER: f32 = 5.0;
/// Length of the corners' grip along each border.
const RESIZE_CORNER: f32 = 14.0;
/// A window sent away by the system while minimized (Windows puts it here).
const MINIMIZED_FAR_AWAY: f32 = -30000.0;

const SHORTCUT_NEW: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::N);
const SHORTCUT_CLOSE: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::W);

pub fn viewport_id(id: u64) -> ViewportId {
    ViewportId::from_hash_of(("note", id))
}

pub fn level(pinned: bool) -> WindowLevel {
    if pinned {
        WindowLevel::AlwaysOnTop
    } else {
        WindowLevel::Normal
    }
}

/// One pass of a note's window.
pub fn window(ui: &mut egui::Ui, shared: &Shared, id: u64) {
    let ctx = ui.ctx().clone();
    // Locked for the whole pass: every window runs on the same thread, and
    // the only other user is the short request from `instance::watch`.
    let mut guard = shared.lock().expect("notes state");
    let state = &mut *guard;

    let before = state.notes.iter().filter(|n| n.open).count();
    let pixels = ctx.pixels_per_point();
    let (close, geometry) = ctx.input(|i| {
        let v = i.viewport();
        let geometry = v
            .outer_rect
            .zip(v.inner_rect)
            .map(|(outer, inner)| ((outer.min.to_vec2() * pixels).round(), inner.size()))
            .filter(|(position, _)| v.minimized != Some(true) && position.x > MINIMIZED_FAR_AWAY);
        (v.close_requested(), geometry)
    });
    let mut place = state.relocate.remove(&id);
    if let Some((position, size)) = geometry {
        place = place.or(state.window_seen(id, position.into(), size.into(), Instant::now()));
    }
    if let Some(position) = place {
        // In physical pixels, which is what is saved: a window created at a
        // logical position lands elsewhere when the display is scaled.
        let points = egui::Vec2::from(position) / pixels;
        ctx.send_viewport_cmd(ViewportCommand::OuterPosition(points.to_pos2()));
    }
    if state.raise == Some(id) {
        ctx.send_viewport_cmd(ViewportCommand::Focus);
    }

    body(ui, state, id);
    resize_grips(&ctx);

    if close || ctx.input_mut(|i| i.consume_shortcut(&SHORTCUT_CLOSE)) {
        state.close(id);
    }
    if ctx.input_mut(|i| i.consume_shortcut(&SHORTCUT_NEW)) {
        state.create(Some(id));
    }

    // Windows appear and disappear in the main window's pass.
    let wanted = state.notes.iter().filter(|n| n.open).count();
    if wanted != before || state.show_list || state.raise.is_some_and(|r| r != id) {
        ctx.request_repaint_of(ViewportId::ROOT);
    }
    state.save_if_due();
    if state.has_unsaved() {
        ctx.request_repaint_after(SAVE_DELAY);
    }
}

/// What's inside the window: also what the website's pictures show.
pub fn body(ui: &mut egui::Ui, state: &mut State, id: u64) {
    let Some(color) = state.notes.iter().find(|n| n.id == id).map(|n| n.color) else {
        return;
    };
    let colors = note_colors(color);
    let rect = ui.max_rect();
    ui.painter().rect_filled(rect, 0.0, colors.paper);

    let awake = ui
        .ctx()
        .input(|i| i.viewport().focused.unwrap_or(true) || i.pointer.hover_pos().is_some());
    let band = Rect::from_min_size(rect.min, vec2(rect.width(), BAND_HEIGHT));
    title_band(ui, state, id, band, &colors, awake);

    let mut top = band.bottom();
    if state.choosing_color == Some(id) {
        let strip = Rect::from_min_size(pos2(rect.left(), top), vec2(rect.width(), 34.0));
        color_strip(ui, state, id, strip, color);
        top = strip.bottom();
    }
    if state.confirming_delete == Some(id) {
        let strip = Rect::from_min_size(pos2(rect.left(), top), vec2(rect.width(), 40.0));
        confirm_strip(ui, state, id, strip, &colors);
        top = strip.bottom();
    }
    let text_rect = Rect::from_min_max(pos2(rect.left(), top), rect.max);
    text(ui, state, id, text_rect, &colors);
}

fn title_band(
    ui: &mut egui::Ui,
    state: &mut State,
    id: u64,
    band: Rect,
    colors: &NoteColors,
    awake: bool,
) {
    ui.painter().rect_filled(band, 0.0, colors.band);
    // Empty space moves the window. Registered first so that the buttons on
    // top of it take precedence.
    let background = ui.interact(band, ui.id().with("drag"), Sense::click_and_drag());
    if background.drag_started_by(egui::PointerButton::Primary) {
        state.user_moves(id);
        ui.ctx().send_viewport_cmd(ViewportCommand::StartDrag);
    }
    let pinned = state.notes.iter().any(|n| n.id == id && n.pinned);
    // A pinned note always says so; the buttons otherwise wait to be needed.
    if !awake && !pinned {
        return;
    }
    let mut row = ui.new_child(
        UiBuilder::new()
            .max_rect(band.shrink2(vec2(3.0, 2.0)))
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    row.spacing_mut().item_spacing.x = 1.0;
    let (ink, hover) = (colors.ink, colors.hover);
    if !awake {
        row.with_layout(egui::Layout::right_to_left(egui::Align::Center), |row| {
            pin_button(row, state, id, pinned, colors);
        });
        return;
    }
    if theme::glyph_button(
        &mut row,
        icon::PLUS,
        BUTTON,
        ink,
        hover,
        false,
        &t!("note-new"),
    )
    .clicked()
    {
        state.create(Some(id));
        row.ctx().request_repaint_of(ViewportId::ROOT);
    }
    if theme::glyph_button(
        &mut row,
        icon::LIST,
        BUTTON,
        ink,
        hover,
        false,
        &t!("note-list"),
    )
    .clicked()
    {
        state.show_list = true;
        row.ctx().request_repaint_of(ViewportId::ROOT);
    }
    row.with_layout(egui::Layout::right_to_left(egui::Align::Center), |row| {
        if theme::glyph_button(row, icon::X, BUTTON, ink, hover, false, &t!("note-close")).clicked()
        {
            state.close(id);
            row.ctx().request_repaint_of(ViewportId::ROOT);
        }
        if theme::glyph_button(
            row,
            icon::TRASH,
            BUTTON,
            ink,
            hover,
            false,
            &t!("note-delete"),
        )
        .clicked()
        {
            let empty = state
                .notes
                .iter()
                .any(|n| n.id == id && n.text.trim().is_empty());
            if empty {
                state.delete(id);
                row.ctx().request_repaint_of(ViewportId::ROOT);
            } else {
                state.confirming_delete = Some(id);
            }
        }
        pin_button(row, state, id, pinned, colors);
        let choosing = state.choosing_color == Some(id);
        if theme::glyph_button(
            row,
            icon::PALETTE,
            BUTTON,
            ink,
            hover,
            choosing,
            &t!("note-color"),
        )
        .clicked()
        {
            state.choosing_color = (!choosing).then_some(id);
        }
    });
}

fn pin_button(ui: &mut egui::Ui, state: &mut State, id: u64, pinned: bool, colors: &NoteColors) {
    let tip = if pinned {
        t!("note-unpin")
    } else {
        t!("note-pin")
    };
    if theme::glyph_button(
        ui,
        icon::PUSH_PIN,
        BUTTON,
        colors.ink,
        colors.hover,
        pinned,
        &tip,
    )
    .clicked()
    {
        if let Some(note) = state.note_mut(id) {
            note.pinned = !pinned;
        }
        state.touch();
        ui.ctx()
            .send_viewport_cmd(ViewportCommand::WindowLevel(level(!pinned)));
    }
}

fn color_strip(ui: &mut egui::Ui, state: &mut State, id: u64, strip: Rect, current: Color) {
    let colors = note_colors(current);
    ui.painter().rect_filled(strip, 0.0, colors.band);
    let width = strip.width() / Color::ALL.len() as f32;
    for (i, color) in Color::ALL.into_iter().enumerate() {
        let cell = Rect::from_min_size(
            strip.min + vec2(width * i as f32, 0.0),
            vec2(width, strip.height()),
        );
        let response = ui.interact(cell, ui.id().with(("color", i)), Sense::click());
        let swatch = cell.shrink2(vec2(2.0, 6.0));
        ui.painter()
            .rect_filled(swatch, 4.0, note_colors(color).paper);
        if color == current || response.hovered() {
            ui.painter().rect_stroke(
                swatch,
                4.0,
                Stroke::new(1.5, colors.ink),
                egui::StrokeKind::Inside,
            );
        }
        if response.clicked() {
            if let Some(note) = state.note_mut(id) {
                note.color = color;
            }
            state.choosing_color = None;
            state.touch();
        }
    }
}

fn confirm_strip(ui: &mut egui::Ui, state: &mut State, id: u64, strip: Rect, colors: &NoteColors) {
    ui.painter().rect_filled(strip, 0.0, colors.band);
    let mut row = ui.new_child(
        UiBuilder::new()
            .max_rect(strip.shrink2(vec2(10.0, 6.0)))
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    row.label(RichText::new(t!("delete-question")).color(colors.ink));
    row.with_layout(egui::Layout::right_to_left(egui::Align::Center), |row| {
        let button = |text: String| {
            egui::Button::new(RichText::new(text).color(colors.ink))
                .fill(colors.hover)
                .stroke(Stroke::NONE)
        };
        if row.add(button(t!("delete-yes"))).clicked() {
            state.delete(id);
            row.ctx().request_repaint_of(ViewportId::ROOT);
        }
        if row.add(button(t!("delete-no"))).clicked() {
            state.confirming_delete = None;
        }
    });
}

fn text(ui: &mut egui::Ui, state: &mut State, id: u64, rect: Rect, colors: &NoteColors) {
    let size = state.text_size.points();
    let raise = state.raise == Some(id);
    let Some(note) = state.note_mut(id) else {
        return;
    };
    let mut child = ui.new_child(UiBuilder::new().max_rect(rect));
    child.set_clip_rect(rect);
    {
        let v = child.visuals_mut();
        v.dark_mode = colors.dark;
        v.selection.bg_fill = Color32::from_rgba_unmultiplied(0x5A, 0x8C, 0xE6, 110);
        v.text_cursor.stroke = Stroke::new(2.0, colors.ink);
        v.widgets.inactive.fg_stroke.color = colors.muted;
        v.widgets.hovered.fg_stroke.color = colors.ink;
        v.widgets.active.fg_stroke.color = colors.ink;
    }
    let mut changed = false;
    let mut focused = false;
    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show(&mut child, |ui| {
            Frame::new().inner_margin(10).show(ui, |ui| {
                let hint = RichText::new(t!("note-hint"))
                    .font(theme::regular(size))
                    .color(colors.muted);
                // Never smaller than the window: a click anywhere goes to the text.
                let height = (rect.height() - 20.0).max(0.0);
                let rows = (height / (size * 1.3)).floor().max(1.0) as usize;
                let response = ui.add(
                    TextEdit::multiline(&mut note.text)
                        .id(egui::Id::new(("note-text", id)))
                        .frame(Frame::NONE)
                        .font(theme::regular(size))
                        .text_color(colors.ink)
                        .hint_text(hint)
                        .desired_width(f32::INFINITY)
                        .desired_rows(rows)
                        .lock_focus(true),
                );
                changed = response.changed();
                focused = response.has_focus();
                if raise {
                    response.request_focus();
                }
            });
        });
    if changed {
        update_title(ui, note);
        state.text_changed(id);
    }
    // Keep asking until the window has the focus and the text with it.
    if raise {
        if focused && ui.ctx().input(|i| i.viewport().focused == Some(true)) {
            state.raise = None;
        } else {
            ui.ctx().request_repaint();
        }
    }
}

/// The title in the taskbar and when switching windows: the note's first line.
fn update_title(ui: &egui::Ui, note: &Note) {
    let (first, _) = note.preview();
    let title = if first.is_empty() {
        crate::app::APP_NAME
    } else {
        first
    };
    ui.ctx()
        .send_viewport_cmd(ViewportCommand::Title(title.chars().take(60).collect()));
}

/// Invisible strips along the window borders that resize it (the window has
/// no system frame).
fn resize_grips(ctx: &egui::Context) {
    let r = ctx.content_rect();
    let b = RESIZE_BORDER;
    let strips = [
        ("n", Rect::from_min_max(r.min, pos2(r.max.x, r.min.y + b))),
        ("s", Rect::from_min_max(pos2(r.min.x, r.max.y - b), r.max)),
        ("w", Rect::from_min_max(r.min, pos2(r.min.x + b, r.max.y))),
        ("e", Rect::from_min_max(pos2(r.max.x - b, r.min.y), r.max)),
    ];
    for (name, strip) in strips {
        egui::Area::new(egui::Id::new("resize_grip").with(name))
            .order(egui::Order::Foreground)
            .fixed_pos(strip.min)
            .show(ctx, |ui| {
                let (_, response) = ui.allocate_exact_size(strip.size(), Sense::drag());
                let Some(pos) = response.hover_pos() else {
                    return;
                };
                let direction = resize_direction(r, pos);
                ctx.set_cursor_icon(resize_cursor(direction));
                if response.is_pointer_button_down_on() && ui.input(|i| i.pointer.primary_pressed())
                {
                    ctx.send_viewport_cmd(ViewportCommand::BeginResize(direction));
                }
            });
    }
}

/// Which border or corner of `window` is under `pos`.
fn resize_direction(window: Rect, pos: Pos2) -> ResizeDirection {
    let c = RESIZE_CORNER;
    let west = pos.x < window.left() + c;
    let east = pos.x > window.right() - c;
    let north = pos.y < window.top() + c;
    let south = pos.y > window.bottom() - c;
    match (north, south, west, east) {
        (true, _, true, _) => ResizeDirection::NorthWest,
        (true, _, _, true) => ResizeDirection::NorthEast,
        (_, true, true, _) => ResizeDirection::SouthWest,
        (_, true, _, true) => ResizeDirection::SouthEast,
        (true, ..) => ResizeDirection::North,
        (_, true, ..) => ResizeDirection::South,
        (_, _, true, _) => ResizeDirection::West,
        _ => ResizeDirection::East,
    }
}

fn resize_cursor(direction: ResizeDirection) -> CursorIcon {
    match direction {
        ResizeDirection::North => CursorIcon::ResizeNorth,
        ResizeDirection::South => CursorIcon::ResizeSouth,
        ResizeDirection::East => CursorIcon::ResizeEast,
        ResizeDirection::West => CursorIcon::ResizeWest,
        ResizeDirection::NorthEast => CursorIcon::ResizeNorthEast,
        ResizeDirection::SouthEast => CursorIcon::ResizeSouthEast,
        ResizeDirection::NorthWest => CursorIcon::ResizeNorthWest,
        ResizeDirection::SouthWest => CursorIcon::ResizeSouthWest,
    }
}
