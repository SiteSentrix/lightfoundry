//! Translating the interface. Every fixed piece of text is passed through [`tr`], which looks the
//! English text up in a catalog the person supplies and returns it unchanged when there is no
//! catalog or no entry. No translation ships with the program; a catalog is a JSON file:
//!
//! ```json
//! { "name": "Deutsch", "font": "/path/to/a/font.ttf", "strings": { "Open…": "Öffnen…" } }
//! ```
//!
//! `font` is optional: egui's built-in font covers Latin scripts only, so a language written in
//! another script needs a font file that has it (the program ships none). "Save translation template"
//! in the Help menu writes every string seen so far with itself as the value, to start from.
//! Messages that come from the engine (errors, command results) are not translated.

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, RwLock};

/// Largest catalog file read, in bytes.
pub const MAX_FILE_BYTES: u64 = 4 * 1024 * 1024;
/// Most entries accepted.
pub const MAX_STRINGS: usize = 20_000;
/// Longest key or translation accepted, in characters.
pub const MAX_LEN: usize = 2_000;
/// Largest font file read, in bytes.
pub const MAX_FONT_BYTES: u64 = 32 * 1024 * 1024;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Translation {
    pub name: String,
    pub font: Option<PathBuf>,
    pub strings: HashMap<String, String>,
}

static ACTIVE: RwLock<Option<Translation>> = RwLock::new(None);
static SEEN: Mutex<BTreeSet<String>> = Mutex::new(BTreeSet::new());

/// The text to show for `english`: its translation if the active catalog has one.
pub fn tr(english: &str) -> String {
    if let Ok(mut seen) = SEEN.lock()
        && !seen.contains(english)
    {
        seen.insert(english.to_string());
    }
    let active = ACTIVE.read().unwrap_or_else(|e| e.into_inner());
    active.as_ref().and_then(|t| t.strings.get(english)).map_or_else(|| english.to_string(), Clone::clone)
}

/// [`tr`] for a text with values in it: the template has `{name}` places, filled from `args`. The
/// values are inserted as they are (a value that itself contains `{name}` is not filled again), and
/// a place with no matching argument is left in the text.
pub fn tr_fill(template: &str, args: &[(&str, String)]) -> String {
    let text = tr(template);
    let mut out = String::with_capacity(text.len() + 16);
    let mut rest = text.as_str();
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        match after.find('}').map(|close| (&after[..close], &after[close + 1..])) {
            Some((name, tail)) if args.iter().any(|(k, _)| *k == name) => {
                if let Some((_, value)) = args.iter().find(|(k, _)| *k == name) {
                    out.push_str(value);
                }
                rest = tail;
            }
            _ => {
                out.push('{');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// `trf!("Opened {name}", name = file)`: a translated text with values filled in (see [`tr_fill`]).
macro_rules! trf {
    ($template:literal $(, $key:ident = $value:expr)* $(,)?) => {
        $crate::i18n::tr_fill($template, &[$((stringify!($key), ($value).to_string())),*])
    };
}
pub(crate) use trf;

/// Makes `translation` the one in use (`None` goes back to English).
pub fn set_active(translation: Option<Translation>) {
    *ACTIVE.write().unwrap_or_else(|e| e.into_inner()) = translation;
}

/// The font file the active catalog asks for, if any.
pub fn active_font() -> Option<PathBuf> {
    ACTIVE.read().unwrap_or_else(|e| e.into_inner()).as_ref().and_then(|t| t.font.clone())
}

/// The name of the active catalog, if there is one.
pub fn active_name() -> Option<String> {
    ACTIVE.read().unwrap_or_else(|e| e.into_inner()).as_ref().map(|t| t.name.clone())
}

/// Reads a font file for the active catalog, refusing one that is too large.
pub fn read_font(path: &Path) -> Result<Vec<u8>, String> {
    use std::io::Read;
    let file = std::fs::File::open(path).map_err(|e| format!("cannot open the font {}: {e}", path.display()))?;
    let mut data = Vec::new();
    file.take(MAX_FONT_BYTES + 1).read_to_end(&mut data).map_err(|e| format!("cannot read the font {}: {e}", path.display()))?;
    if data.len() as u64 > MAX_FONT_BYTES {
        return Err("the font file is too large".into());
    }
    Ok(data)
}

/// Every string asked for so far, as a catalog with each string as its own translation.
pub fn template() -> String {
    let seen = SEEN.lock().unwrap_or_else(|e| e.into_inner());
    let strings: serde_json::Map<String, serde_json::Value> = seen.iter().map(|s| (s.clone(), serde_json::Value::String(s.clone()))).collect();
    serde_json::to_string_pretty(&serde_json::json!({ "name": "", "font": "", "strings": strings })).unwrap_or_default()
}

/// Parses a catalog. Unusable input is an error with a reason; nothing here can panic.
pub fn parse(text: &str) -> Result<Translation, String> {
    let v: serde_json::Value = serde_json::from_str(text).map_err(|e| format!("not a translation file: {e}"))?;
    let obj = v.as_object().ok_or("a translation file is a JSON object")?;
    let name = obj.get("name").and_then(|n| n.as_str()).unwrap_or("").chars().take(80).collect();
    let font = obj.get("font").and_then(|f| f.as_str()).filter(|f| !f.is_empty()).map(PathBuf::from);
    let raw = obj.get("strings").and_then(|s| s.as_object()).ok_or("the file has no \"strings\" object")?;
    if raw.len() > MAX_STRINGS {
        return Err(format!("more than {MAX_STRINGS} strings"));
    }
    let mut strings = HashMap::with_capacity(raw.len());
    for (key, value) in raw {
        let Some(value) = value.as_str() else { return Err(format!("the translation of {key:?} is not text")) };
        if key.chars().count() > MAX_LEN || value.chars().count() > MAX_LEN {
            return Err("a string is too long".into());
        }
        if !value.is_empty() && value != key {
            strings.insert(key.clone(), value.to_string());
        }
    }
    Ok(Translation { name, font, strings })
}

/// Reads and parses the catalog at `path`.
pub fn load(path: &Path) -> Result<Translation, String> {
    use std::io::Read;
    let file = std::fs::File::open(path).map_err(|e| format!("cannot open {}: {e}", path.display()))?;
    let mut text = String::new();
    file.take(MAX_FILE_BYTES + 1).read_to_string(&mut text).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    if text.len() as u64 > MAX_FILE_BYTES {
        return Err("the translation file is too large".into());
    }
    parse(&text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_catalog_parses_and_translates_with_english_as_the_fallback() {
        let t = parse(r#"{"name":"Test","strings":{"Open…":"Öffnen…","Same":"Same","Empty":""}}"#).unwrap();
        assert_eq!(t.name, "Test");
        assert_eq!(t.strings.len(), 1, "identical and empty entries are not kept");
        assert_eq!(t.strings.get("Open…").map(String::as_str), Some("Öffnen…"));
    }

    #[test]
    fn values_are_filled_into_translated_templates_without_being_filled_again() {
        assert_eq!(tr_fill("Job {n} of {total}", &[("n", "3".into()), ("total", "9".into())]), "Job 3 of 9");
        assert_eq!(tr_fill("{a}{b}", &[("a", "{b}".into()), ("b", "x".into())]), "{b}x", "values are not re-expanded");
        assert_eq!(tr_fill("odd { brace and {missing}", &[("n", "1".into())]), "odd { brace and {missing}");
        assert_eq!(tr_fill("", &[]), "");
        let t = parse(r#"{"strings":{"i18n-fill {n} items":"{n} Einträge (i18n-fill)"}}"#).unwrap();
        set_active(Some(t));
        assert_eq!(trf!("i18n-fill {n} items", n = 4), "4 Einträge (i18n-fill)");
        set_active(None);
        assert_eq!(trf!("i18n-fill {n} items", n = 4), "i18n-fill 4 items");
    }

    #[test]
    fn hostile_catalogs_are_errors() {
        for text in ["", "nope", "[]", "{}", r#"{"strings":[]}"#, r#"{"strings":{"a":1}}"#] {
            assert!(parse(text).is_err(), "{text}");
        }
        let long = format!(r#"{{"strings":{{"a":"{}"}}}}"#, "x".repeat(MAX_LEN + 1));
        assert!(parse(&long).is_err());
        let many: String = (0..=MAX_STRINGS).map(|i| format!("\"k{i}\":\"v\"")).collect::<Vec<_>>().join(",");
        assert!(parse(&format!(r#"{{"strings":{{{many}}}}}"#)).is_err());
    }

    #[test]
    fn files_are_read_with_limits_and_the_template_lists_what_was_asked_for() {
        let dir = std::env::temp_dir().join(format!("lf-i18n-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("t.json");
        std::fs::write(&file, r#"{"strings":{"i18n-test-key":"Übersetzt"}}"#).unwrap();
        let t = load(&file).unwrap();
        set_active(Some(t));
        assert_eq!(tr("i18n-test-key"), "Übersetzt");
        assert_eq!(tr("i18n-other-key"), "i18n-other-key");
        let template: serde_json::Value = serde_json::from_str(&template()).unwrap();
        assert!(template["strings"]["i18n-test-key"].is_string() && template["strings"]["i18n-other-key"].is_string());
        set_active(None);
        assert_eq!(tr("i18n-test-key"), "i18n-test-key");
        assert!(load(&dir.join("missing.json")).is_err());
        std::fs::write(&file, vec![b' '; MAX_FILE_BYTES as usize + 10]).unwrap();
        assert!(load(&file).is_err());
        assert!(read_font(&dir.join("missing.ttf")).is_err());
        std::fs::remove_dir_all(&dir).ok();
    }
}
