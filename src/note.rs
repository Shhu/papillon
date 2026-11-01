//! A note: its own window without a system frame, a strip at the top that
//! moves it, its borders that resize it, and its text.

use std::ops::Range;
use std::time::{Duration, Instant};

use eframe::egui::text_edit::TextEditState;
use eframe::egui::util::undoer::{Settings, Undoer};
use eframe::egui::viewport::ResizeDirection;
use eframe::egui::{
    self, Color32, CursorIcon, Frame, Key, KeyboardShortcut, Modifiers, PointerButton, Pos2, Rect,
    RichText, Sense, Stroke, TextBuffer, TextEdit, UiBuilder, ViewportCommand, ViewportId,
    WindowLevel, pos2, vec2,
};
use egui_phosphor::regular as icon;

use crate::app::Shared;
use crate::i18n::t;
use crate::store::{self, Color, MIN_SIZE, Note, SAVE_DELAY, SELECTORS, State};
use crate::theme::{self, NoteColors, note_colors};

const BAND_HEIGHT: f32 = 30.0;
const BUTTON: f32 = 26.0;
/// Space between the strip's buttons.
const BUTTON_GAP: f32 = 1.0;
/// Width of the window border that resizes it.
const RESIZE_BORDER: f32 = 5.0;
/// Length of the corners' grip along each border.
const RESIZE_CORNER: f32 = 14.0;
/// A window sent away by the system while minimized (Windows puts it here).
const MINIMIZED_FAR_AWAY: f32 = -30000.0;
/// How far the arrow keys move or resize a window.
const KEYBOARD_STEP: f32 = 20.0;
/// Undo steps kept per note: each is a copy of the whole text.
const UNDO_STEPS: usize = 30;
/// Height of the question asked before deleting, with its buttons.
pub const CONFIRM_HEIGHT: f32 = 44.0;
/// How soon the list shows what changed in a note: its pass makes egui forget
/// the note's layout, which is slow to redo for long notes at each key.
const LIST_REFRESH_DELAY: Duration = Duration::from_millis(300);
/// The cursor stops blinking after this many seconds without input: blinking
/// is the only thing that keeps an idle window drawing.
const BLINK_FOR: f64 = 10.0;

const SHORTCUT_NEW: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::N);
const SHORTCUT_CLOSE: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::W);

pub fn viewport_id(id: u64) -> ViewportId {
    ViewportId::from_hash_of(("note", id))
}

pub fn text_id(id: u64) -> egui::Id {
    egui::Id::new(("note-text", id))
}

pub fn level(pinned: bool) -> WindowLevel {
    if pinned {
        WindowLevel::AlwaysOnTop
    } else {
        WindowLevel::Normal
    }
}

/// The title in the taskbar and when switching windows: the note's first line.
pub fn title(note: &Note) -> String {
    let (first, _) = note.preview();
    if first.is_empty() {
        crate::app::APP_NAME.to_owned()
    } else {
        first.chars().take(60).collect()
    }
}

/// Redraws the open notes, after a setting they show changed.
pub fn repaint_all(ctx: &egui::Context, state: &State) {
    for note in state.notes.iter().filter(|n| n.open) {
        ctx.request_repaint_of(viewport_id(note.id));
    }
}

/// A note's text as the text field sees it. Emoji style selectors stay out:
/// with one-color emoji they do nothing, but the cursor would stop on them and
/// Backspace would seem to do nothing.
struct PlainText<'a>(&'a mut String);

impl TextBuffer for PlainText<'_> {
    fn is_mutable(&self) -> bool {
        true
    }

    fn as_str(&self) -> &str {
        self.0.as_str()
    }

    fn insert_text(&mut self, text: &str, char_index: egui::text::CharIndex) -> usize {
        let clean = text.replace(SELECTORS, "");
        self.0.insert_text(&clean, char_index)
    }

    fn delete_char_range(&mut self, char_range: Range<egui::text::CharIndex>) {
        self.0.delete_char_range(char_range);
    }

    fn type_id(&self) -> std::any::TypeId {
        std::any::TypeId::of::<String>()
    }
}

/// One pass of a note's window.
pub fn window(ui: &mut egui::Ui, shared: &Shared, id: u64) {
    let ctx = ui.ctx().clone();
    // Locked for the whole pass: every window runs on the same thread, and
    // the only other user is the short request from `instance::watch`.
    let mut guard = store::lock(shared);
    let state = &mut *guard;

    let before = state.notes.iter().filter(|n| n.open).count();
    let revision = state.revision();
    let pixels = ctx.pixels_per_point();
    if ctx.input(|i| !i.events.is_empty()) {
        state.last_input = ctx.input(|i| i.time);
    }
    // The system holds the minimum size in pixels, from the screen the window
    // was created on.
    if state.scales.insert(id, pixels) != Some(pixels) {
        ctx.send_viewport_cmd(ViewportCommand::MinInnerSize(MIN_SIZE.into()));
    }
    let (close, geometry) = ctx.input(|i| {
        let v = i.viewport();
        let geometry = v
            .outer_rect
            .zip(v.inner_rect)
            .map(|(outer, inner)| ((outer.min.to_vec2() * pixels).round(), inner.size()))
            .filter(|(position, _)| {
                v.minimized != Some(true)
                    // Screen-sized: not the size to come back to.
                    && v.maximized != Some(true)
                    && position.x > MINIMIZED_FAR_AWAY
            });
        (v.close_requested(), geometry)
    });
    if let Some((position, size)) = geometry {
        let first = state.window_seen(id, position.into(), size.into(), Instant::now());
        if let Some(position) = first {
            limit_undo(&ctx, id);
            // In physical pixels, which is what is saved: a window created at
            // a logical position lands elsewhere when the display is scaled.
            let points = egui::Vec2::from(position) / pixels;
            ctx.send_viewport_cmd(ViewportCommand::OuterPosition(points.to_pos2()));
        }
    }
    if state.raise == Some(id) {
        ctx.send_viewport_cmd(ViewportCommand::Focus);
    }

    keyboard_geometry(&ctx, state, id);
    body(ui, state, id);
    if resize_grips(&ctx) {
        state.user_moves(id);
    }

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
    } else if state.list_visible && state.revision() != revision {
        ctx.request_repaint_after_for(LIST_REFRESH_DELAY, ViewportId::ROOT);
    }
    // Asked of the main window, which runs even if this one gets minimized in
    // the meantime.
    if state.has_unsaved() {
        ctx.request_repaint_after_for(SAVE_DELAY, ViewportId::ROOT);
    }
}

/// Each step of undo is a copy of the text: long notes would weigh a lot.
fn limit_undo(ctx: &egui::Context, id: u64) {
    let mut edit = TextEditState::load(ctx, text_id(id)).unwrap_or_default();
    edit.set_undoer(Undoer::with_settings(Settings {
        max_undos: UNDO_STEPS,
        ..Default::default()
    }));
    edit.store(ctx, text_id(id));
}

/// Alt + arrows moves the window, Alt + Shift + arrows resizes it.
fn keyboard_geometry(ctx: &egui::Context, state: &mut State, id: u64) {
    let (mut moved, mut resized) = (egui::Vec2::ZERO, egui::Vec2::ZERO);
    let alt_shift = Modifiers::ALT | Modifiers::SHIFT;
    ctx.input_mut(|i| {
        for (key, step) in [
            (Key::ArrowLeft, vec2(-KEYBOARD_STEP, 0.0)),
            (Key::ArrowRight, vec2(KEYBOARD_STEP, 0.0)),
            (Key::ArrowUp, vec2(0.0, -KEYBOARD_STEP)),
            (Key::ArrowDown, vec2(0.0, KEYBOARD_STEP)),
        ] {
            if i.consume_key(alt_shift, key) {
                resized += step;
            }
            if i.consume_key(Modifiers::ALT, key) {
                moved += step;
            }
        }
    });
    if moved == egui::Vec2::ZERO && resized == egui::Vec2::ZERO {
        return;
    }
    state.user_moves(id);
    let (outer, inner) = ctx.input(|i| (i.viewport().outer_rect, i.viewport().inner_rect));
    if let (true, Some(outer)) = (moved != egui::Vec2::ZERO, outer) {
        ctx.send_viewport_cmd(ViewportCommand::OuterPosition(outer.min + moved));
    }
    if let (true, Some(inner)) = (resized != egui::Vec2::ZERO, inner) {
        let size = (inner.size() + resized).max(MIN_SIZE.into());
        ctx.send_viewport_cmd(ViewportCommand::InnerSize(size));
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
        let height = CONFIRM_HEIGHT + 12.0;
        let strip = Rect::from_min_size(pos2(rect.left(), top), vec2(rect.width(), height));
        ui.painter().rect_filled(strip, 0.0, colors.band);
        let inside = strip.shrink2(vec2(10.0, 6.0));
        let focus_keep = std::mem::take(&mut state.confirm_fresh);
        match confirm_row(ui, inside, colors.ink, colors.hover, focus_keep) {
            Some(true) => {
                state.delete(id);
                ui.ctx().request_repaint_of(ViewportId::ROOT);
            }
            Some(false) => {
                state.confirming_delete = None;
                state.refocus = Some(id);
            }
            None => {}
        }
        top = strip.bottom();
    }
    let text_rect = Rect::from_min_max(pos2(rect.left(), top), rect.max);
    text(ui, state, id, text_rect, &colors);
}

/// "Delete this note?" above its answers: true to delete, false to keep.
/// Keep is on the right, where the trash button is: a second click on the
/// trash must not delete. It also takes the keyboard's focus when asked.
pub fn confirm_row(
    ui: &mut egui::Ui,
    rect: Rect,
    ink: Color32,
    fill: Color32,
    focus_keep: bool,
) -> Option<bool> {
    let mut answer = None;
    let mut area = ui.new_child(
        UiBuilder::new()
            .max_rect(rect)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    area.spacing_mut().item_spacing.y = 2.0;
    area.spacing_mut().interact_size.y = 22.0;
    area.spacing_mut().button_padding = vec2(8.0, 2.0);
    area.add(
        egui::Label::new(RichText::new(t!("delete-question")).color(ink))
            .truncate()
            .selectable(false),
    );
    area.with_layout(egui::Layout::right_to_left(egui::Align::Center), |row| {
        let button = |text: String| {
            egui::Button::new(RichText::new(text).color(ink))
                .fill(fill)
                .stroke(Stroke::NONE)
        };
        let keep = row.add(button(t!("delete-no")));
        theme::focus_ring(row, keep.rect, &keep, ink);
        if focus_keep {
            keep.request_focus();
        }
        if keep.clicked() {
            answer = Some(false);
        }
        let delete = row.add(button(t!("delete-yes")));
        theme::focus_ring(row, delete.rect, &delete, ink);
        if delete.clicked() {
            answer = Some(true);
        }
    });
    answer
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
    // top of it take precedence. `DRAG` alone: not a stop for the Tab key.
    let background = ui.interact(band, ui.id().with("drag"), Sense::DRAG);
    if background.drag_started_by(PointerButton::Primary) {
        state.user_moves(id);
        ui.ctx().send_viewport_cmd(ViewportCommand::StartDrag);
    }
    // A click on the strip isn't a reason to stop typing.
    if background.hovered() && ui.input(|i| i.pointer.primary_released()) {
        state.refocus = Some(id);
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
    row.spacing_mut().item_spacing.x = BUTTON_GAP;
    let (ink, hover) = (colors.ink, colors.hover);
    let button = |ui: &mut egui::Ui, glyph, on, tip: String| {
        theme::glyph_button(ui, glyph, BUTTON, ink, hover, on, &tip)
    };
    if !awake {
        // In the very place it has when awake, where Close must not come
        // under a pointer aiming at it.
        let slot = row.available_width() - 3.0 * (BUTTON + BUTTON_GAP);
        row.add_space(slot.max(0.0));
        pin_button(&mut row, state, id, pinned, colors);
        return;
    }
    if button(&mut row, icon::PLUS, false, t!("note-new")).clicked() {
        state.create(Some(id));
        row.ctx().request_repaint_of(ViewportId::ROOT);
    }
    let list = button(&mut row, icon::LIST, false, t!("note-list"));
    if list.clicked() {
        state.show_list = true;
        row.ctx().request_repaint_of(ViewportId::ROOT);
        if list.clicked_by(PointerButton::Primary) {
            state.refocus = Some(id);
        }
    }
    // Added in the order they are seen, for the Tab key.
    let slot = row.available_width() - 4.0 * (BUTTON + BUTTON_GAP);
    row.add_space(slot.max(0.0));
    let choosing = state.choosing_color == Some(id);
    let palette = button(&mut row, icon::PALETTE, choosing, t!("note-color"));
    if palette.clicked() {
        state.choosing_color = (!choosing).then_some(id);
        // From the keyboard, the next stop must be the colors, not the text.
        if palette.clicked_by(PointerButton::Primary) {
            state.refocus = Some(id);
        }
    }
    pin_button(&mut row, state, id, pinned, colors);
    if button(&mut row, icon::TRASH, false, t!("note-delete")).clicked() && state.request_delete(id)
    {
        row.ctx().request_repaint_of(ViewportId::ROOT);
    }
    if button(&mut row, icon::X, false, t!("note-close")).clicked() {
        state.close(id);
        row.ctx().request_repaint_of(ViewportId::ROOT);
    }
}

fn pin_button(ui: &mut egui::Ui, state: &mut State, id: u64, pinned: bool, colors: &NoteColors) {
    let tip = if pinned {
        t!("note-unpin")
    } else {
        t!("note-pin")
    };
    let glyph = icon::PUSH_PIN;
    let response = theme::glyph_button(ui, glyph, BUTTON, colors.ink, colors.hover, pinned, &tip);
    if response.clicked() {
        if let Some(note) = state.note_mut(id) {
            note.pinned = !pinned;
        }
        state.touch();
        ui.ctx()
            .send_viewport_cmd(ViewportCommand::WindowLevel(level(!pinned)));
        if response.clicked_by(PointerButton::Primary) {
            state.refocus = Some(id);
        }
    }
}

fn color_name(color: Color) -> String {
    match color {
        Color::Yellow => t!("color-yellow"),
        Color::Green => t!("color-green"),
        Color::Pink => t!("color-pink"),
        Color::Purple => t!("color-purple"),
        Color::Blue => t!("color-blue"),
        Color::Gray => t!("color-gray"),
        Color::Charcoal => t!("color-charcoal"),
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
        let name = color_name(color);
        response.widget_info(|| {
            egui::WidgetInfo::selected(egui::WidgetType::RadioButton, true, color == current, &name)
        });
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
        // Around the cell, unlike the ring of the color chosen.
        theme::focus_ring(ui, cell.shrink(1.0), &response, colors.ink);
        let clicked = response.clicked();
        response.on_hover_text(name);
        if clicked {
            if let Some(note) = state.note_mut(id) {
                note.color = color;
            }
            state.choosing_color = None;
            state.touch();
            state.refocus = Some(id);
        }
    }
}

fn text(ui: &mut egui::Ui, state: &mut State, id: u64, rect: Rect, colors: &NoteColors) {
    let size = state.text_size.points();
    let raise = state.raise == Some(id);
    let refocus = state.refocus.take_if(|r| *r == id).is_some();
    let blink = ui.ctx().input(|i| i.time) - state.last_input < BLINK_FOR;
    let Some(note) = state.note_mut(id) else {
        return;
    };
    crate::fonts::ensure_for(ui.ctx(), &note.text);
    let mut child = ui.new_child(UiBuilder::new().max_rect(rect));
    child.set_clip_rect(rect);
    {
        // Clear of the border that resizes the window.
        child.spacing_mut().scroll.bar_outer_margin = RESIZE_BORDER + 1.0;
        let v = child.visuals_mut();
        v.dark_mode = colors.dark;
        v.weak_text_color = Some(colors.muted);
        v.selection.bg_fill = Color32::from_rgba_unmultiplied(0x5A, 0x8C, 0xE6, 150);
        // Selected text takes this color: not the list's theme.
        v.selection.stroke.color = colors.ink;
        v.text_cursor.stroke = Stroke::new(2.0, colors.ink);
        v.text_cursor.blink = blink;
        v.widgets.inactive.fg_stroke.color = colors.muted;
        v.widgets.hovered.fg_stroke.color = colors.ink;
        v.widgets.active.fg_stroke.color = colors.ink;
    }
    let mut changed = false;
    let mut focused = false;
    let ink = colors.ink;
    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show(&mut child, |ui| {
            Frame::new().inner_margin(10).show(ui, |ui| {
                let hint = RichText::new(t!("note-hint")).font(theme::regular(size));
                // Never smaller than the window: a click anywhere goes to the text.
                let height = (rect.height() - 20.0).max(0.0);
                let mut buffer = PlainText(&mut note.text);
                let output = TextEdit::multiline(&mut buffer)
                    .id(text_id(id))
                    .frame(Frame::NONE)
                    .font(theme::regular(size))
                    .text_color(ink)
                    .hint_text(hint)
                    .desired_width(f32::INFINITY)
                    .min_size(vec2(0.0, height))
                    .lock_focus(true)
                    .show(ui);
                let response = &output.response.response;
                ui.ctx().accesskit_node_builder(response.id, |node| {
                    node.set_label(t!("note-text-label"));
                });
                changed = response.changed();
                focused = response.has_focus();
                // After the widget: it gives the focus up in the very pass of
                // a click elsewhere.
                if raise || refocus {
                    response.request_focus();
                }
                // A strip that opened may have pushed the cursor out of sight.
                if refocus && let Some(range) = output.cursor_range {
                    let cursor = output.galley.pos_from_cursor(range.primary);
                    ui.scroll_to_rect(cursor.translate(output.galley_pos.to_vec2()), None);
                }
            });
        });
    if changed {
        let title = title(note);
        ui.ctx().send_viewport_cmd(ViewportCommand::Title(title));
        state.text_changed(id);
    }
    // Keep asking until the window has the focus and the text with it, for a
    // moment: the system may well refuse.
    if raise {
        let got_it = focused && ui.ctx().input(|i| i.viewport().focused == Some(true));
        if !state.raise_over(got_it) {
            ui.ctx().request_repaint_after(Duration::from_millis(50));
        }
    }
}

/// Invisible strips along the window borders that resize it (the window has
/// no system frame).
fn resize_grips(ctx: &egui::Context) -> bool {
    let mut resizing = false;
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
                let (_, response) = ui.allocate_exact_size(strip.size(), Sense::DRAG);
                let Some(pos) = response.hover_pos() else {
                    return;
                };
                let direction = resize_direction(r, pos);
                ctx.set_cursor_icon(resize_cursor(direction));
                if response.is_pointer_button_down_on() && ui.input(|i| i.pointer.primary_pressed())
                {
                    resizing = true;
                    ctx.send_viewport_cmd(ViewportCommand::BeginResize(direction));
                }
            });
    }
    resizing
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn borders_and_corners_resize_the_right_way() {
        let window = Rect::from_min_size(pos2(0.0, 0.0), vec2(300.0, 300.0));
        let at = |x, y| resize_direction(window, pos2(x, y));
        assert_eq!(at(1.0, 1.0), ResizeDirection::NorthWest);
        assert_eq!(at(299.0, 299.0), ResizeDirection::SouthEast);
        assert_eq!(at(150.0, 1.0), ResizeDirection::North);
        assert_eq!(at(299.0, 150.0), ResizeDirection::East);
    }

    #[test]
    fn pinned_means_always_on_top() {
        assert_eq!(level(true), WindowLevel::AlwaysOnTop);
        assert_eq!(level(false), WindowLevel::Normal);
    }

    #[test]
    fn emoji_selectors_never_enter_a_note() {
        let mut text = String::from("ab");
        let mut buffer = PlainText(&mut text);
        assert_eq!(buffer.insert_text("❤\u{FE0F}", egui::text::CharIndex(1)), 1);
        assert_eq!(text, "a❤b");
    }
}
