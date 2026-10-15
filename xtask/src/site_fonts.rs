//! Puts the font library's drawable fonts on the hosted site: `cargo xtask site-fonts <fonts-repo> <site-dir>`
//! copies `fonts/<folder>/{font.json,glyphs.stitchfoundry,OFL.txt}` for every complete folder into
//! `<site-dir>/fonts/<folder>/` and writes `<site-dir>/fonts/index.json`, which the browser build reads to
//! download them from the site itself (never from a third party). Nothing is added to this repository.

use std::fs;
use std::path::Path;

use serde_json::{Value, json};

const REQUIRED: [&str; 2] = ["font.json", "glyphs.stitchfoundry"];
/// Copied when present: the licence text travels with the fonts.
const OPTIONAL: [&str; 2] = ["OFL.txt", "LICENSE"];
/// Largest file copied, in bytes (the browser holds these in memory).
const MAX_FILE_BYTES: u64 = 8 * 1024 * 1024;

pub fn run(args: &[String]) -> Result<(), String> {
    let [repo, site] = args else { return Err("usage: cargo xtask site-fonts <fonts-repo> <site-dir>".into()) };
    let count = build(Path::new(repo), Path::new(site))?;
    println!("site-fonts: {count} fonts copied to {site}/fonts");
    Ok(())
}

/// Returns how many fonts were copied. Fails when there are none, so an empty site is never published silently.
pub fn build(repo: &Path, site: &Path) -> Result<usize, String> {
    let source = repo.join("fonts");
    let mut folders: Vec<_> = fs::read_dir(&source)
        .map_err(|e| format!("cannot read {}: {e}", source.display()))?
        .flatten()
        .filter(|e| e.path().is_dir())
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|name| plain(name) && REQUIRED.iter().all(|f| source.join(name).join(f).is_file()))
        .collect();
    folders.sort();
    let out = site.join("fonts");
    let mut fonts = Vec::new();
    for folder in &folders {
        let mut files = Vec::new();
        for name in REQUIRED.iter().chain(&OPTIONAL) {
            let from = source.join(folder).join(name);
            let Ok(meta) = fs::metadata(&from) else { continue };
            if !meta.is_file() || meta.len() > MAX_FILE_BYTES {
                return Err(format!("{} is not a file or is over {MAX_FILE_BYTES} bytes", from.display()));
            }
            let to = out.join(folder).join(name);
            fs::create_dir_all(to.parent().ok_or("no parent")?).map_err(|e| format!("cannot create {}: {e}", out.display()))?;
            fs::copy(&from, &to).map_err(|e| format!("cannot copy {}: {e}", from.display()))?;
            files.push(json!({ "path": name, "size": meta.len() }));
        }
        let display = fs::read_to_string(source.join(folder).join("font.json"))
            .ok()
            .and_then(|t| serde_json::from_str::<Value>(&t).ok())
            .and_then(|v| v.get("name").and_then(Value::as_str).map(str::to_string))
            .unwrap_or_else(|| folder.clone());
        fonts.push(json!({ "folder": folder, "name": display, "files": files }));
    }
    if fonts.is_empty() {
        return Err(format!("no complete font folders under {}", source.display()));
    }
    let count = fonts.len();
    fs::write(out.join("index.json"), serde_json::to_string_pretty(&json!({ "fonts": fonts })).map_err(|e| e.to_string())?)
        .map_err(|e| format!("cannot write the font index: {e}"))?;
    Ok(count)
}

/// A folder name that is safe to publish and to ask for by URL.
fn plain(name: &str) -> bool {
    !name.is_empty() && name.len() <= 80 && !name.starts_with('.') && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn complete_folders_are_copied_with_an_index_and_the_rest_are_left_out() {
        let dir = std::env::temp_dir().join(format!("xtask-sitefonts-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let repo = dir.join("repo");
        let site = dir.join("site");
        for (folder, files) in [
            ("good-font", &["font.json", "glyphs.stitchfoundry", "OFL.txt", "preview.png"][..]),
            ("no-glyphs", &["font.json"]),
            ("bad name!", &["font.json", "glyphs.stitchfoundry"]),
            (".hidden", &["font.json", "glyphs.stitchfoundry"]),
        ] {
            fs::create_dir_all(repo.join("fonts").join(folder)).unwrap();
            for f in files {
                let body = if *f == "font.json" { r#"{"name":"Good Font"}"#.to_string() } else { "x".to_string() };
                fs::write(repo.join("fonts").join(folder).join(f), body).unwrap();
            }
        }
        assert_eq!(build(&repo, &site).unwrap(), 1);
        let index: Value = serde_json::from_str(&fs::read_to_string(site.join("fonts/index.json")).unwrap()).unwrap();
        assert_eq!(index["fonts"][0]["folder"], "good-font");
        assert_eq!(index["fonts"][0]["name"], "Good Font");
        let paths: Vec<&str> = index["fonts"][0]["files"].as_array().unwrap().iter().filter_map(|f| f["path"].as_str()).collect();
        assert_eq!(paths, ["font.json", "glyphs.stitchfoundry", "OFL.txt"], "previews are not published");
        assert!(site.join("fonts/good-font/glyphs.stitchfoundry").exists());
        assert!(!site.join("fonts/no-glyphs").exists() && !site.join("fonts/bad name!").exists() && !site.join("fonts/.hidden").exists());

        // Nothing usable is an error (never publish an empty library), as is a missing repository.
        let empty = dir.join("empty");
        fs::create_dir_all(empty.join("fonts")).unwrap();
        assert!(build(&empty, &dir.join("site2")).is_err());
        assert!(build(&dir.join("nope"), &dir.join("site3")).is_err());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn folder_names_are_plain() {
        assert!(plain("foundry-block") && plain("a_b.c"));
        for bad in ["", ".x", "a b", "a/b", "é", &"x".repeat(100)] {
            assert!(!plain(bad), "{bad:?}");
        }
    }
}
