//! UI translations. Each language has its own Fluent file in `locales/`
//! (same keys, in the same order), embedded in the executable. English is the
//! fallback: a key missing from a translation is shown in English.
//!
//! In code, `t!("key")` or `t!("key", name = value)` gives the text in the
//! chosen language.

use std::path::PathBuf;
use std::sync::LazyLock;
use std::sync::atomic::{AtomicUsize, Ordering};

use fluent_bundle::concurrent::FluentBundle;
use fluent_bundle::{FluentArgs, FluentResource};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Lang {
    En,
    Fr,
}

impl Lang {
    /// English first: it is the fallback language.
    pub const ALL: [Lang; 2] = [Lang::En, Lang::Fr];

    pub fn code(self) -> &'static str {
        match self {
            Lang::En => "en",
            Lang::Fr => "fr",
        }
    }

    /// Name of the language in that language, for the menu.
    pub fn native_name(self) -> &'static str {
        match self {
            Lang::En => "English",
            Lang::Fr => "Français",
        }
    }

    fn source(self) -> &'static str {
        match self {
            Lang::En => include_str!("../locales/en.ftl"),
            Lang::Fr => include_str!("../locales/fr.ftl"),
        }
    }

    /// Accepts "fr", "fr-CA", "fr_FR.UTF-8"…
    fn from_code(code: &str) -> Option<Lang> {
        let primary = code.split(['-', '_', '.']).next()?.to_ascii_lowercase();
        Lang::ALL.into_iter().find(|l| l.code() == primary)
    }

    fn index(self) -> usize {
        Lang::ALL
            .iter()
            .position(|&l| l == self)
            .expect("known language")
    }
}

static CURRENT: AtomicUsize = AtomicUsize::new(0);

static BUNDLES: LazyLock<Vec<FluentBundle<FluentResource>>> =
    LazyLock::new(|| Lang::ALL.iter().map(|&l| bundle(l)).collect());

fn bundle(lang: Lang) -> FluentBundle<FluentResource> {
    let id = lang.code().parse().expect("valid language code");
    let mut bundle = FluentBundle::new_concurrent(vec![id]);
    // No Unicode isolation marks around values: egui's fonts show them as
    // empty boxes.
    bundle.set_use_isolating(false);
    // Syntax errors are caught by the tests; whatever parses is kept.
    let resource = FluentResource::try_new(lang.source().to_owned()).unwrap_or_else(|(r, _)| r);
    let _ = bundle.add_resource(resource);
    bundle
}

pub fn lang() -> Lang {
    Lang::ALL[CURRENT.load(Ordering::Relaxed)]
}

/// Language at startup: the one picked in the menu, else the system's, else
/// English.
pub fn init() {
    let saved = std::fs::read_to_string(settings_file()).ok();
    let lang = saved
        .as_deref()
        .map(str::trim)
        .and_then(Lang::from_code)
        .or_else(|| {
            sys_locale::get_locale()
                .as_deref()
                .and_then(Lang::from_code)
        })
        .unwrap_or(Lang::En);
    CURRENT.store(lang.index(), Ordering::Relaxed);
}

/// Switches language and remembers it for the next launches.
pub fn choose(lang: Lang) {
    CURRENT.store(lang.index(), Ordering::Relaxed);
    let file = settings_file();
    if let Some(dir) = file.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(file, lang.code());
}

fn settings_file() -> PathBuf {
    crate::config::dir().join("language")
}

/// Text of key `id` in the chosen language, falling back to English, then to
/// the key itself if it exists nowhere.
pub fn tr(id: &str, args: Option<&FluentArgs>) -> String {
    let bundles = &*BUNDLES;
    let current = &bundles[CURRENT.load(Ordering::Relaxed)];
    for bundle in [current, &bundles[0]] {
        if let Some(pattern) = bundle.get_message(id).and_then(|m| m.value()) {
            let mut errors = Vec::new();
            return bundle
                .format_pattern(pattern, args, &mut errors)
                .into_owned();
        }
    }
    id.to_owned()
}

macro_rules! t {
    ($id:literal) => {
        $crate::i18n::tr($id, None)
    };
    ($id:literal, $($name:ident = $value:expr),+ $(,)?) => {{
        let mut args = fluent_bundle::FluentArgs::new();
        $(args.set(stringify!($name), $value);)+
        $crate::i18n::tr($id, Some(&args))
    }};
}
pub(crate) use t;

#[cfg(test)]
mod tests {
    use super::*;
    use fluent_syntax::ast;
    use std::collections::BTreeSet;

    /// Each message of a language, in file order: its key and the
    /// variables it uses.
    fn messages(lang: Lang) -> Vec<(String, BTreeSet<String>)> {
        let resource = fluent_syntax::parser::parse(lang.source())
            .unwrap_or_else(|(_, errors)| panic!("{}.ftl: {errors:?}", lang.code()));
        resource
            .body
            .iter()
            .filter_map(|entry| match entry {
                ast::Entry::Message(m) => {
                    let mut vars = BTreeSet::new();
                    if let Some(value) = &m.value {
                        collect_vars(value, &mut vars);
                    }
                    Some((m.id.name.to_owned(), vars))
                }
                _ => None,
            })
            .collect()
    }

    fn collect_vars(pattern: &ast::Pattern<&str>, vars: &mut BTreeSet<String>) {
        for element in &pattern.elements {
            if let ast::PatternElement::Placeable { expression } = element {
                collect_expression(expression, vars);
            }
        }
    }

    fn collect_expression(expression: &ast::Expression<&str>, vars: &mut BTreeSet<String>) {
        match expression {
            ast::Expression::Inline(ast::InlineExpression::VariableReference { id }) => {
                vars.insert(id.name.to_owned());
            }
            ast::Expression::Select { selector, variants } => {
                if let ast::InlineExpression::VariableReference { id } = selector {
                    vars.insert(id.name.to_owned());
                }
                for v in variants {
                    collect_vars(&v.value, vars);
                }
            }
            _ => {}
        }
    }

    /// Keys given to `t!` in the sources.
    fn used_keys() -> BTreeSet<String> {
        let mut keys = BTreeSet::new();
        let mut dirs = vec![std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src")];
        let mut files = Vec::new();
        while let Some(dir) = dirs.pop() {
            for entry in std::fs::read_dir(dir).expect("source folder") {
                let path = entry.expect("entry").path();
                if path.is_dir() {
                    dirs.push(path);
                } else {
                    files.push(path);
                }
            }
        }
        for file in files {
            let source = std::fs::read_to_string(file).expect("source");
            let code: Vec<&str> = source
                .lines()
                .filter(|l| !l.trim_start().starts_with("//"))
                .collect();
            let code = code.join("\n");
            for (i, _) in code.match_indices("t!(") {
                // Not the end of `format!(` and the like, nor this string.
                if code[..i].ends_with(|c: char| c.is_alphanumeric() || c == '_' || c == '"') {
                    continue;
                }
                if let Some(rest) = code[i + 3..].trim_start().strip_prefix('"') {
                    keys.insert(rest[..rest.find('"').expect("closing quote")].to_owned());
                }
            }
        }
        keys
    }

    #[test]
    fn every_language_has_the_same_keys_in_the_same_order() {
        let en = messages(Lang::En);
        for lang in Lang::ALL {
            let other = messages(lang);
            let keys = |m: &[(String, BTreeSet<String>)]| {
                m.iter().map(|(k, _)| k.clone()).collect::<Vec<_>>()
            };
            assert_eq!(keys(&other), keys(&en), "{}: keys", lang.code());
            for ((key, vars), (_, other_vars)) in en.iter().zip(&other) {
                assert_eq!(vars, other_vars, "{}: variables of {key}", lang.code());
            }
        }
    }

    #[test]
    fn every_key_is_translated_and_used() {
        let en: BTreeSet<String> = messages(Lang::En).into_iter().map(|(k, _)| k).collect();
        let used = used_keys();
        let undefined: Vec<_> = used.difference(&en).collect();
        let unused: Vec<_> = en.difference(&used).collect();
        assert!(
            undefined.is_empty(),
            "keys missing from en.ftl: {undefined:?}"
        );
        assert!(unused.is_empty(), "unused keys: {unused:?}");
    }

    /// French puts a no-break space before : ; ! ? and inside « »: a line
    /// must not have a breakable one there.
    #[test]
    fn french_typography() {
        for line in Lang::Fr.source().lines().filter(|l| !l.starts_with('#')) {
            for bad in [" :", " ;", " !", " ?", "« ", " »"] {
                assert!(
                    !line.contains(bad),
                    "fr: breakable space in {bad:?}: {line}"
                );
            }
        }
    }

    #[test]
    fn recognizes_system_locales() {
        assert_eq!(Lang::from_code("fr-FR"), Some(Lang::Fr));
        assert_eq!(Lang::from_code("fr_CA.UTF-8"), Some(Lang::Fr));
        assert_eq!(Lang::from_code("en-US"), Some(Lang::En));
        assert_eq!(Lang::from_code("de-DE"), None);
    }
}
