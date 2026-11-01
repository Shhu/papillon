//! Writing systems the embedded fonts don't cover: the system's fonts, loaded
//! the first time a note needs them, since some weigh tens of megabytes.
//! egui draws each character on its own, without joining or reordering them:
//! Arabic letters stay unjoined and right-to-left text runs left to right.

use std::borrow::Cow;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};

use eframe::egui::epaint::text::{FontInsert, FontPriority, InsertFontFamily};
use eframe::egui::{self, FontData, FontFamily, FontId};

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum Script {
    Cjk,
    Hangul,
    ArabicHebrew,
    ArmenianGeorgian,
    Indic,
    Thai,
    Symbols,
}

fn script_of(c: char) -> Option<Script> {
    Some(match c as u32 {
        0x0530..=0x058F | 0x10A0..=0x10FF => Script::ArmenianGeorgian,
        0x0590..=0x08FF | 0xFB1D..=0xFDFF | 0xFE70..=0xFEFF => Script::ArabicHebrew,
        0x0900..=0x0DFF => Script::Indic,
        0x0E00..=0x0EFF => Script::Thai,
        0x1100..=0x11FF | 0xAC00..=0xD7FF => Script::Hangul,
        0x2E80..=0x9FFF | 0xF900..=0xFAFF | 0xFF00..=0xFFEF | 0x20000..=0x3FFFF => Script::Cjk,
        0x2190..=0x2BFF | 0x1F000..=0x1FAFF => Script::Symbols,
        _ => return None,
    })
}

/// Font files to try for a script, best first.
fn candidates(script: Script) -> &'static [&'static str] {
    if cfg!(windows) {
        match script {
            Script::Cjk => &["msyh.ttc", "YuGothR.ttc", "msgothic.ttc"],
            Script::Hangul => &["malgun.ttf"],
            Script::ArabicHebrew | Script::ArmenianGeorgian => &["segoeui.ttf", "tahoma.ttf"],
            Script::Indic => &["Nirmala.ttc", "Nirmala.ttf"],
            Script::Thai => &["LeelawUI.ttf", "tahoma.ttf"],
            Script::Symbols => &["seguiemj.ttf", "seguisym.ttf"],
        }
    } else {
        match script {
            Script::Cjk | Script::Hangul => &[
                "NotoSansCJK-Regular.ttc",
                "NotoSansCJKsc-Regular.otf",
                "NotoSansCJKjp-Regular.otf",
                "wqy-microhei.ttc",
                "DroidSansFallbackFull.ttf",
                "NanumGothic.ttf",
            ],
            Script::ArabicHebrew => &["NotoSansArabic-Regular.ttf", "DejaVuSans.ttf"],
            Script::ArmenianGeorgian => &["DejaVuSans.ttf", "FreeSans.ttf"],
            Script::Indic => &[
                "NotoSansDevanagari-Regular.ttf",
                "Lohit-Devanagari.ttf",
                "FreeSans.ttf",
            ],
            Script::Thai => &["NotoSansThai-Regular.ttf", "FreeSerif.ttf"],
            Script::Symbols => &["NotoEmoji-Regular.ttf", "Symbola.ttf", "DejaVuSans.ttf"],
        }
    }
}

fn font_dirs() -> Vec<PathBuf> {
    if cfg!(windows) {
        let root = std::env::var_os("WINDIR").unwrap_or_else(|| r"C:\Windows".into());
        return vec![PathBuf::from(root).join("Fonts")];
    }
    let mut dirs: Vec<PathBuf> = [
        // The host's fonts, inside a Flatpak.
        "/run/host/fonts",
        "/run/host/local-fonts",
        "/run/host/user-fonts",
        "/usr/share/fonts",
        "/usr/local/share/fonts",
    ]
    .iter()
    .map(PathBuf::from)
    .collect();
    if let Some(home) = std::env::var_os("HOME") {
        dirs.push(PathBuf::from(home).join(".local/share/fonts"));
    }
    dirs
}

/// Where `name` is under `dir`, at any depth.
fn find(dir: &Path, name: &str, depth: u8) -> Option<PathBuf> {
    let direct = dir.join(name);
    if direct.is_file() {
        return Some(direct);
    }
    if depth == 0 {
        return None;
    }
    std::fs::read_dir(dir)
        .ok()?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .find_map(|p| find(&p, name, depth - 1))
}

#[derive(Default)]
struct Loaded {
    scripts: HashSet<Script>,
    /// Characters known to need nothing more.
    checked: HashSet<char>,
}

static LOADED: LazyLock<Mutex<Loaded>> = LazyLock::new(Default::default);

/// Loads what `text` needs and the embedded fonts lack; shown from the next
/// frame on.
pub fn ensure_for(ctx: &egui::Context, text: &str) {
    if text.is_ascii() {
        return;
    }
    let mut loaded = crate::store::lock(&LOADED);
    let font = FontId::proportional(13.0);
    for c in text.chars().filter(|c| !c.is_ascii()) {
        if loaded.checked.contains(&c) {
            continue;
        }
        loaded.checked.insert(c);
        let Some(script) = script_of(c) else {
            continue;
        };
        if loaded.scripts.contains(&script) || ctx.fonts_mut(|f| f.has_glyph(&font, c)) {
            continue;
        }
        loaded.scripts.insert(script);
        load(ctx, script);
    }
}

fn load(ctx: &egui::Context, script: Script) {
    let dirs = font_dirs();
    let file = candidates(script)
        .iter()
        .find_map(|name| dirs.iter().find_map(|dir| find(dir, name, 4)));
    let Some(data) = file.and_then(|file| std::fs::read(file).ok()) else {
        return;
    };
    let families = [
        FontFamily::Proportional,
        FontFamily::Name(crate::theme::MEDIUM.into()),
        FontFamily::Name(crate::theme::SEMIBOLD.into()),
    ]
    .map(|family| InsertFontFamily {
        family,
        priority: FontPriority::Lowest,
    });
    ctx.add_font(FontInsert {
        name: format!("system-{script:?}"),
        data: FontData::from_owned(data),
        families: families.to_vec(),
    });
    ctx.request_repaint();
}

/// `text` with the emoji style selectors swapped for characters of the same
/// count that draw nothing: fonts rarely have them, and the replacement box
/// would follow each ❤️.
pub fn displayable(text: &str) -> Cow<'_, str> {
    if text.contains(['\u{FE0E}', '\u{FE0F}']) {
        Cow::Owned(text.replace(['\u{FE0E}', '\u{FE0F}'], "\u{200B}"))
    } else {
        Cow::Borrowed(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selectors_are_hidden_without_moving_the_cursor() {
        let shown = displayable("a❤\u{FE0F}b");
        assert_eq!(shown.chars().count(), 4);
        assert!(!shown.contains('\u{FE0F}'));
        assert!(matches!(displayable("café"), Cow::Borrowed(_)));
    }

    #[test]
    fn scripts_are_recognized() {
        assert_eq!(script_of('日'), Some(Script::Cjk));
        assert_eq!(script_of('한'), Some(Script::Hangul));
        assert_eq!(script_of('م'), Some(Script::ArabicHebrew));
        assert_eq!(script_of('🦋'), Some(Script::Symbols));
        assert_eq!(script_of('é'), None);
    }

    #[test]
    fn fonts_are_found_at_any_depth() {
        let dir = std::env::temp_dir().join(format!("papillon-fonts-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("a/b")).unwrap();
        std::fs::write(dir.join("a/b/x.ttf"), "").unwrap();
        assert!(find(&dir, "x.ttf", 4).is_some());
        assert!(find(&dir, "x.ttf", 1).is_none());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
