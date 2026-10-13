//! Every file under `assets/` must have a row in ATTRIBUTION.md (path, author, source, licence).
//! Icons and default art are drawn in code, so a clean tree has no `assets/` directory at all.

use std::fs;
use std::path::Path;

pub fn run(root: &Path) -> Result<(), String> {
    let dir = root.join("assets");
    if !dir.exists() {
        println!("assets: ok (no assets/ directory)");
        return Ok(());
    }
    let attribution = fs::read_to_string(root.join("ATTRIBUTION.md")).unwrap_or_default();
    let mut missing = Vec::new();
    collect(root, &dir, &attribution, &mut missing);
    if missing.is_empty() {
        println!("assets: ok");
        Ok(())
    } else {
        Err(format!("assets without an ATTRIBUTION.md row:\n  {}", missing.join("\n  ")))
    }
}

fn collect(root: &Path, dir: &Path, attribution: &str, missing: &mut Vec<String>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect(root, &path, attribution, missing);
        } else {
            let rel = path.strip_prefix(root).unwrap_or(&path).display().to_string();
            if !attribution.contains(&rel) {
                missing.push(rel);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unattributed_assets_fail_and_attributed_ones_pass() {
        let dir = std::env::temp_dir().join(format!("lf-assets-{}", std::process::id()));
        fs::create_dir_all(dir.join("assets")).unwrap();
        fs::write(dir.join("assets/a.png"), "x").unwrap();
        assert!(run(&dir).is_err());
        fs::write(dir.join("ATTRIBUTION.md"), "| assets/a.png | me | original | MIT OR Apache-2.0 |").unwrap();
        assert!(run(&dir).is_ok());
        let _ = fs::remove_dir_all(&dir);
    }
}
