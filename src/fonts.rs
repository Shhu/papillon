//! Writing systems the embedded fonts don't cover: the system's fonts, found
//! and read on a thread the first time a note needs them, since some weigh
//! tens of megabytes. Files are checked first: egui aborts on a damaged font.
//! egui draws each character on its own, without joining or reordering them:
//! Arabic letters stay unjoined and right-to-left text runs left to right.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Sender, channel};
use std::sync::{LazyLock, Mutex};

use eframe::egui::epaint::text::{FontInsert, FontPriority, InsertFontFamily};
use eframe::egui::{self, FontData, FontFamily, FontId};
use skrifa::{FontRef, MetadataProvider as _};

/// How deep font folders are searched.
const SEARCH_DEPTH: u8 = 4;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum Script {
    Cjk,
    Hangul,
    Arabic,
    Hebrew,
    ArmenianGeorgian,
    Indic,
    Thai,
    SoutheastAsian,
    Ethiopic,
    Tibetan,
    Symbols,
}

fn script_of(c: char) -> Option<Script> {
    Some(match c as u32 {
        0x0530..=0x058F | 0x10A0..=0x10FF | 0x1C90..=0x1CBF | 0x2D00..=0x2D2F => {
            Script::ArmenianGeorgian
        }
        0x0590..=0x05FF | 0xFB1D..=0xFB4F => Script::Hebrew,
        0x0600..=0x08FF | 0xFB50..=0xFDFF | 0xFE70..=0xFEFF => Script::Arabic,
        0x0900..=0x0DFF => Script::Indic,
        0x0E00..=0x0EFF => Script::Thai,
        0x0F00..=0x0FFF => Script::Tibetan,
        0x1000..=0x109F | 0x1780..=0x17FF => Script::SoutheastAsian,
        0x1200..=0x139F => Script::Ethiopic,
        0x1100..=0x11FF | 0xAC00..=0xD7FF => Script::Hangul,
        0x2E80..=0x9FFF | 0xF900..=0xFAFF | 0xFF00..=0xFFEF | 0x20000..=0x3FFFF => Script::Cjk,
        0x2190..=0x2BFF | 0x1F000..=0x1FAFF => Script::Symbols,
        _ => return None,
    })
}

/// Font files to try for a script, best first: the first that has the
/// character wanted is used.
fn candidates(script: Script) -> &'static [&'static str] {
    if cfg!(windows) {
        match script {
            Script::Cjk => &["msyh.ttc", "YuGothR.ttc", "msgothic.ttc"],
            Script::Hangul => &["malgun.ttf"],
            Script::Arabic | Script::Hebrew | Script::ArmenianGeorgian => {
                &["segoeui.ttf", "tahoma.ttf"]
            }
            Script::Indic => &["Nirmala.ttc", "Nirmala.ttf"],
            Script::Thai => &["LeelawUI.ttf", "tahoma.ttf"],
            Script::SoutheastAsian => &["mmrtext.ttf", "LeelawUI.ttf"],
            Script::Ethiopic => &["ebrima.ttf"],
            Script::Tibetan => &["himalaya.ttf"],
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
            Script::Arabic => &["NotoSansArabic-Regular.ttf", "DejaVuSans.ttf"],
            Script::Hebrew => &["NotoSansHebrew-Regular.ttf", "DejaVuSans.ttf"],
            Script::ArmenianGeorgian => &[
                "NotoSansGeorgian-Regular.ttf",
                "NotoSansArmenian-Regular.ttf",
                "DejaVuSans.ttf",
                "FreeSans.ttf",
            ],
            Script::Indic => &[
                "NotoSansDevanagari-Regular.ttf",
                "NotoSansTamil-Regular.ttf",
                "NotoSansBengali-Regular.ttf",
                "Lohit-Devanagari.ttf",
                "FreeSans.ttf",
            ],
            Script::Thai => &["NotoSansThai-Regular.ttf", "FreeSerif.ttf"],
            Script::SoutheastAsian => &["NotoSansMyanmar-Regular.ttf", "NotoSansKhmer-Regular.ttf"],
            Script::Ethiopic => &["NotoSansEthiopic-Regular.ttf", "FreeSerif.ttf"],
            Script::Tibetan => &["NotoSerifTibetan-Regular.ttf"],
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

/// Where `name` is under `dir`, down to `depth` folders.
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

/// A font file ready to go in, with what it covers.
struct Found {
    name: String,
    data: Vec<u8>,
    covers: Vec<char>,
}

/// What a font file holds, if it is one egui can use.
fn examine(name: &str, data: Vec<u8>) -> Option<Found> {
    // The parser egui itself uses.
    let font = FontRef::from_index(&data, 0).ok()?;
    let covers = font
        .charmap()
        .mappings()
        .filter_map(|(c, _)| char::from_u32(c))
        .collect();
    Some(Found {
        name: name.to_owned(),
        data,
        covers,
    })
}

/// The first font for `script` that has `c`, out of those not loaded yet.
fn look_for(script: Script, c: char, dirs: &[PathBuf], loaded: &HashSet<String>) -> Option<Found> {
    candidates(script)
        .iter()
        .filter(|name| !loaded.contains(**name))
        .find_map(|name| {
            let file = dirs.iter().find_map(|dir| find(dir, name, SEARCH_DEPTH))?;
            let found = examine(name, std::fs::read(file).ok()?)?;
            found.covers.contains(&c).then_some(found)
        })
}

#[derive(Default)]
struct Loading {
    /// Characters known to need nothing more, or already asked for.
    checked: HashSet<char>,
    /// Fonts found by the thread, waiting for the next pass.
    ready: Vec<Found>,
    sender: Option<Sender<(Script, char)>>,
}

static LOADING: LazyLock<Mutex<Loading>> = LazyLock::new(Default::default);

fn start_thread(ctx: &egui::Context) -> Sender<(Script, char)> {
    let (sender, receiver) = channel::<(Script, char)>();
    let ctx = ctx.clone();
    std::thread::spawn(move || {
        let dirs = font_dirs();
        let mut loaded = HashSet::new();
        let mut covered = HashSet::new();
        while let Ok((script, c)) = receiver.recv() {
            if covered.contains(&c) {
                continue;
            }
            let Some(found) = look_for(script, c, &dirs, &loaded) else {
                continue;
            };
            loaded.insert(found.name.clone());
            covered.extend(found.covers.iter().copied());
            crate::store::lock(&LOADING).ready.push(found);
            ctx.request_repaint();
        }
    });
    sender
}

/// Asks for what `text` needs and the embedded fonts lack; it shows a little
/// later. Fonts already found go in here, between two passes' worth of text.
pub fn ensure_for(ctx: &egui::Context, text: &str) {
    let mut loading = crate::store::lock(&LOADING);
    for found in std::mem::take(&mut loading.ready) {
        add(ctx, found);
    }
    if text.is_ascii() {
        return;
    }
    let font = FontId::proportional(13.0);
    for c in text.chars().filter(|c| !c.is_ascii()) {
        if !loading.checked.insert(c) {
            continue;
        }
        let Some(script) = script_of(c) else {
            continue;
        };
        if ctx.fonts_mut(|f| f.has_glyph(&font, c)) {
            continue;
        }
        let sender = loading.sender.get_or_insert_with(|| start_thread(ctx));
        let _ = sender.send((script, c));
    }
}

fn add(ctx: &egui::Context, found: Found) {
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
        name: format!("system-{}", found.name),
        data: FontData::from_owned(found.data),
        families: families.to_vec(),
    });
    ctx.request_repaint();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scripts_are_recognized() {
        assert_eq!(script_of('日'), Some(Script::Cjk));
        assert_eq!(script_of('한'), Some(Script::Hangul));
        assert_eq!(script_of('م'), Some(Script::Arabic));
        assert_eq!(script_of('ש'), Some(Script::Hebrew));
        assert_eq!(script_of('🦋'), Some(Script::Symbols));
        assert_eq!(script_of('é'), None);
    }

    #[test]
    fn fonts_are_found_down_to_the_depth_asked() {
        let dir = std::env::temp_dir().join(format!("papillon-fonts-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("a/b")).unwrap();
        std::fs::write(dir.join("a/b/x.ttf"), "").unwrap();
        assert!(find(&dir, "x.ttf", 4).is_some());
        assert!(find(&dir, "x.ttf", 1).is_none());
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// egui aborts on a damaged font: such a file must never reach it.
    #[test]
    fn damaged_font_files_are_refused() {
        assert!(examine("empty", Vec::new()).is_none());
        assert!(examine("noise", vec![7; 4096]).is_none());
        let inter = include_bytes!("../assets/fonts/Inter-Regular.ttf").to_vec();
        let found = examine("inter", inter).unwrap();
        assert!(found.covers.contains(&'é'));
    }

    #[test]
    fn a_damaged_file_gives_way_to_the_next_candidate() {
        let dir = std::env::temp_dir().join(format!("papillon-next-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let [first, second] = candidates(Script::Symbols)[..2] else {
            panic!("two candidates");
        };
        std::fs::write(dir.join(first), "not a font").unwrap();
        std::fs::write(
            dir.join(second),
            include_bytes!("../assets/fonts/Inter-Regular.ttf"),
        )
        .unwrap();
        let dirs = [dir.clone()];
        let found = look_for(Script::Symbols, 'é', &dirs, &HashSet::new()).unwrap();
        assert_eq!(found.name, second);
        // Already loaded, or lacking the character: nothing more to offer.
        assert!(
            look_for(
                Script::Symbols,
                'é',
                &dirs,
                &HashSet::from([second.to_owned()])
            )
            .is_none()
        );
        assert!(look_for(Script::Symbols, '🦋', &dirs, &HashSet::new()).is_none());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
