//! `cargo xtask stats`: tests and lines of Rust per workspace crate, for the progress docs.

use std::fs;
use std::path::Path;

pub fn run(root: &Path) -> Result<(), String> {
    let mut rows = Vec::new();
    for group in ["crates", "apps", "xtask"] {
        let base = root.join(group);
        let crates: Vec<_> = if group == "xtask" {
            vec![base]
        } else {
            fs::read_dir(&base)
                .map_err(|e| format!("cannot read {}: {e}", base.display()))?
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.is_dir())
                .collect()
        };
        for dir in crates {
            let (lines, tests) = count(&dir);
            let name = dir.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned());
            rows.push((name, lines, tests));
        }
    }
    rows.sort();
    println!("{:<28} {:>8} {:>7}", "crate", "lines", "tests");
    let (mut total_lines, mut total_tests) = (0, 0);
    for (name, lines, tests) in &rows {
        println!("{name:<28} {lines:>8} {tests:>7}");
        total_lines += lines;
        total_tests += tests;
    }
    println!("{:<28} {total_lines:>8} {total_tests:>7}", "total");
    Ok(())
}

/// (lines of `.rs`, `#[test]` attributes) under a crate, skipping build output.
pub fn count(dir: &Path) -> (usize, usize) {
    let (mut lines, mut tests) = (0, 0);
    let Ok(entries) = fs::read_dir(dir) else { return (0, 0) };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if entry.file_name() != "target" {
                let (l, t) = count(&path);
                lines += l;
                tests += t;
            }
        } else if path.extension().is_some_and(|e| e == "rs")
            && let Ok(text) = fs::read_to_string(&path)
        {
            lines += text.lines().count();
            tests += text.lines().filter(|l| l.trim() == "#[test]").count();
        }
    }
    (lines, tests)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_and_tests_are_counted_and_target_is_skipped() {
        let dir = std::env::temp_dir().join(format!("xtask-stats-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("src")).unwrap();
        fs::create_dir_all(dir.join("target")).unwrap();
        fs::write(dir.join("src/lib.rs"), "fn a() {}\n#[test]\nfn t() {}\n#[test]\nfn u() {}\n").unwrap();
        fs::write(dir.join("target/x.rs"), "#[test]\n").unwrap();
        fs::write(dir.join("notes.txt"), "#[test]\n").unwrap();
        assert_eq!(count(&dir), (5, 2));
        let _ = fs::remove_dir_all(&dir);
        assert_eq!(count(&dir), (0, 0));
    }
}
