//! Colors, fonts and widget style of the list window, in a dark and a light
//! theme, and the colors of the notes, the same in both.

use std::sync::Arc;

use eframe::egui::{
    self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Margin, Rect,
    Sense, Stroke, TextStyle, Theme, ThemePreference, Vec2, vec2,
};

use crate::store::Color;

/// The colors of a theme.
pub struct Palette {
    pub bg: Color32,
    pub surface: Color32,
    pub raised: Color32,
    pub line: Color32,
    /// Around fields and controls, which must stand out.
    pub control_line: Color32,
    pub text: Color32,
    pub muted: Color32,
    pub faint: Color32,
    pub accent: Color32,
    pub accent_soft: Color32,
    pub accent_ink: Color32,
    pub on_accent: Color32,
}

const fn rgb(hex: u32) -> Color32 {
    Color32::from_rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

const fn rgba(r: u8, g: u8, b: u8, alpha: f32) -> Color32 {
    Color32::from_rgba_unmultiplied_const(r, g, b, (alpha * 255.0) as u8)
}

pub const DARK: Palette = Palette {
    bg: rgb(0x1B1E24),
    surface: rgb(0x22262E),
    raised: rgb(0x2A2F38),
    line: rgb(0x2C313A),
    control_line: rgb(0x6B7484),
    text: rgb(0xE7E9EE),
    muted: rgb(0x9AA3B2),
    faint: rgb(0x8F98A8),
    accent: rgb(0xFFD84D),
    accent_soft: rgba(0xFF, 0xD8, 0x4D, 0.16),
    accent_ink: rgb(0xFFD84D),
    on_accent: rgb(0x2B2308),
};

pub const LIGHT: Palette = Palette {
    bg: rgb(0xF6F7F9),
    surface: rgb(0xFFFFFF),
    raised: rgb(0xECEEF2),
    line: rgb(0xE2E5EA),
    control_line: rgb(0x868E9C),
    text: rgb(0x1A1D23),
    muted: rgb(0x576070),
    faint: rgb(0x6B7280),
    accent: rgb(0xFFD84D),
    accent_soft: rgba(0xE0, 0xB0, 0x10, 0.20),
    // The accent darkened to stand out on white.
    accent_ink: rgb(0x8A6A00),
    on_accent: rgb(0x2B2308),
};

/// The palette of the theme shown.
pub fn palette(ui: &egui::Ui) -> &'static Palette {
    if ui.visuals().dark_mode {
        &DARK
    } else {
        &LIGHT
    }
}

/// The colors of a note.
pub struct NoteColors {
    pub paper: Color32,
    /// The strip at the top, which moves the window.
    pub band: Color32,
    pub ink: Color32,
    pub muted: Color32,
    /// Behind a hovered button.
    pub hover: Color32,
    pub dark: bool,
}

pub fn note_colors(color: Color) -> NoteColors {
    let light = |paper, band| NoteColors {
        paper: rgb(paper),
        band: rgb(band),
        ink: rgb(0x26231A),
        muted: rgba(0x26, 0x23, 0x1A, 0.72),
        hover: rgba(0, 0, 0, 0.10),
        dark: false,
    };
    match color {
        Color::Yellow => light(0xFFF4B0, 0xFBE78A),
        Color::Green => light(0xCFF2C8, 0xB7E8AE),
        Color::Pink => light(0xFFD0E6, 0xFBB9D9),
        Color::Purple => light(0xE8D3FF, 0xDBBEFB),
        Color::Blue => light(0xCFE9FF, 0xB6DDFF),
        Color::Gray => light(0xEAEAEA, 0xD9D9D9),
        Color::Charcoal => NoteColors {
            paper: rgb(0x3A3B3F),
            band: rgb(0x2C2D31),
            ink: rgb(0xF1F1F1),
            muted: rgba(0xF1, 0xF1, 0xF1, 0.62),
            hover: rgba(255, 255, 255, 0.12),
            dark: true,
        },
    }
}

pub const SEMIBOLD: &str = "inter-semibold";
pub const MEDIUM: &str = "inter-medium";
const ICONS: &str = "icons";

/// Inter, the interface's font.
pub fn regular(size: f32) -> FontId {
    FontId::proportional(size)
}

pub fn medium(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(MEDIUM.into()))
}

pub fn semibold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(SEMIBOLD.into()))
}

/// Phosphor icons first: Inter has glyphs of its own where Phosphor puts
/// its icons (Unicode's private use area).
pub fn icons(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(ICONS.into()))
}

/// Fonts, styles of both themes, and the theme chosen last time.
pub fn install(ctx: &egui::Context) {
    ctx.set_fonts(fonts());
    ctx.style_mut_of(Theme::Dark, |s| style(s, &DARK));
    ctx.style_mut_of(Theme::Light, |s| style(s, &LIGHT));
    ctx.set_theme(saved_theme());
    // Zooming would change the size notes are saved with.
    ctx.options_mut(|o| o.zoom_with_keyboard = false);
}

/// Inter (assets/fonts, OFL), with Phosphor icons and egui's fonts as
/// fallback.
fn fonts() -> FontDefinitions {
    let mut fonts = FontDefinitions::default();
    egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
    let fallback = fonts.families[&FontFamily::Proportional].clone();
    let faces: [(&str, &'static [u8]); 3] = [
        ("inter", include_bytes!("../assets/fonts/Inter-Regular.ttf")),
        (MEDIUM, include_bytes!("../assets/fonts/Inter-Medium.ttf")),
        (
            SEMIBOLD,
            include_bytes!("../assets/fonts/Inter-SemiBold.ttf"),
        ),
    ];
    for (name, data) in faces {
        fonts
            .font_data
            .insert(name.into(), Arc::new(FontData::from_static(data)));
    }
    let family = |first: &str| {
        std::iter::once(first.to_owned())
            .chain(fallback.iter().cloned())
            .collect::<Vec<_>>()
    };
    let mut icon_fallback = vec!["phosphor".to_owned(), "inter".to_owned()];
    icon_fallback.extend(fallback.iter().filter(|f| *f != "phosphor").cloned());
    fonts.families.extend([
        (FontFamily::Name(ICONS.into()), icon_fallback),
        (FontFamily::Proportional, family("inter")),
        (FontFamily::Name(MEDIUM.into()), family(MEDIUM)),
        (FontFamily::Name(SEMIBOLD.into()), family(SEMIBOLD)),
    ]);
    fonts
}

fn style(style: &mut egui::Style, p: &Palette) {
    style.text_styles = [
        (TextStyle::Heading, semibold(17.0)),
        (TextStyle::Body, regular(13.0)),
        (TextStyle::Button, regular(13.0)),
        (TextStyle::Small, regular(11.0)),
        (TextStyle::Monospace, FontId::monospace(12.0)),
    ]
    .into();
    let s = &mut style.spacing;
    s.item_spacing = vec2(8.0, 6.0);
    s.button_padding = vec2(10.0, 4.0);
    s.interact_size.y = 26.0;
    s.menu_margin = Margin::same(6);
    s.scroll.bar_width = 6.0;
    s.scroll.floating = false;
    s.scroll.dormant_handle_opacity = 0.3;
    s.scroll.active_handle_opacity = 0.45;
    s.scroll.interact_handle_opacity = 0.6;
    s.scroll.dormant_background_opacity = 0.0;
    s.scroll.active_background_opacity = 0.0;
    s.scroll.interact_background_opacity = 0.0;

    let v = &mut style.visuals;
    // Crisp text: glyphs snapped to the pixel (fractional positions blur them).
    v.text_options.subpixel_binning = false;
    v.override_text_color = None;
    v.weak_text_color = Some(p.faint);
    v.hyperlink_color = p.accent_ink;
    v.extreme_bg_color = p.surface;
    v.text_edit_bg_color = Some(p.surface);
    v.panel_fill = p.bg;
    v.window_fill = p.bg;
    v.window_stroke = Stroke::new(1.0, p.line);
    v.menu_corner_radius = CornerRadius::same(10);
    v.selection.bg_fill = p.accent_soft;
    v.selection.stroke = Stroke::new(1.0, p.accent_ink);
    v.text_cursor.stroke = Stroke::new(2.0, p.accent_ink);

    let radius = CornerRadius::same(8);
    let w = &mut v.widgets;
    w.noninteractive.bg_fill = p.bg;
    w.noninteractive.weak_bg_fill = p.bg;
    w.noninteractive.bg_stroke = Stroke::new(1.0, p.line);
    w.noninteractive.fg_stroke = Stroke::new(1.0, p.text);
    w.noninteractive.corner_radius = radius;
    for (state, fill, stroke, text) in [
        (&mut w.inactive, p.surface, p.control_line, p.text),
        (&mut w.hovered, p.raised, p.line, p.text),
        (&mut w.active, p.raised, p.accent_ink, p.text),
        (&mut w.open, p.raised, p.line, p.text),
    ] {
        state.bg_fill = fill;
        state.weak_bg_fill = fill;
        state.bg_stroke = Stroke::new(1.0, stroke);
        state.fg_stroke = Stroke::new(1.0, text);
        state.corner_radius = radius;
        state.expansion = 0.0;
    }
}

// Theme choice

pub fn saved_theme() -> ThemePreference {
    match std::fs::read_to_string(theme_file())
        .as_deref()
        .map(str::trim)
    {
        Ok("dark") => ThemePreference::Dark,
        Ok("light") => ThemePreference::Light,
        _ => ThemePreference::System,
    }
}

pub fn choose_theme(ctx: &egui::Context, theme: ThemePreference) {
    ctx.set_theme(theme);
    let name = match theme {
        ThemePreference::Dark => "dark",
        ThemePreference::Light => "light",
        ThemePreference::System => "system",
    };
    let file = theme_file();
    if let Some(dir) = file.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(file, name);
}

fn theme_file() -> std::path::PathBuf {
    crate::config::dir().join("theme")
}

// Painted pieces

/// A square button with a Phosphor glyph, transparent until hovered. Named by
/// its tooltip, for screen readers too.
pub fn glyph_button(
    ui: &mut egui::Ui,
    glyph: &str,
    size: f32,
    color: Color32,
    hover_fill: Color32,
    on: bool,
    tip: &str,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(size), Sense::click());
    if on || response.hovered() {
        ui.painter().rect_filled(rect, 6.0, hover_fill);
    }
    focus_ring(ui, rect, &response, color);
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        glyph,
        icons(size * 0.62),
        color,
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, tip));
    response.on_hover_text(tip)
}

/// Shows where the keyboard is.
pub fn focus_ring(ui: &egui::Ui, rect: Rect, response: &egui::Response, color: Color32) {
    if response.has_focus() {
        ui.painter()
            .rect_stroke(rect, 6.0, Stroke::new(1.5, color), egui::StrokeKind::Inside);
    }
}

/// Paints `text` on one line in `rect`, cut with "…" when it doesn't fit.
pub fn truncated_text(ui: &egui::Ui, rect: Rect, text: &str, font: FontId, color: Color32) {
    let mut job = egui::text::LayoutJob::simple_singleline(text.to_owned(), font, color);
    job.wrap = egui::text::TextWrapping::truncate_at_width(rect.width().max(0.0));
    let galley = ui.painter().layout_job(job);
    let pos = egui::pos2(rect.left(), rect.center().y - galley.size().y / 2.0);
    ui.painter().galley(pos, galley, color);
}
