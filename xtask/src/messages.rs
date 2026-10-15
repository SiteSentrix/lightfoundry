//! Keeps `crates/i18n/engine-messages.txt` (every message the program's own errors can show, one
//! template per line) in step with the source, so translators know what to translate.
//!
//! Sources of messages: `#[error("...")]` attributes, and the text inside `EngineError::Variant(...)`
//! constructors. Test code (`tests.rs` files and anything after `#[cfg(test)]`) is skipped.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

const OUTPUT: &str = "crates/i18n/engine-messages.txt";
const VARIANTS: [&str; 4] = ["BadParams", "NotFound", "Import", "Internal"];

pub fn run(root: &Path, check: bool) -> Result<(), String> {
    let mut files = Vec::new();
    collect(&root.join("crates"), &mut files);
    files.sort();
    let mut found = BTreeSet::new();
    for file in files {
        let text = std::fs::read_to_string(&file).map_err(|e| format!("cannot read {}: {e}", file.display()))?;
        let text = text.split("#[cfg(test)]").next().unwrap_or("");
        for lit in literals_in_errors(text) {
            if let Some(t) = normalise(&lit) {
                found.insert(t);
            }
        }
    }
    let body: String = found.iter().map(|t| format!("{t}\n")).collect();
    let path = root.join(OUTPUT);
    if check {
        let current = std::fs::read_to_string(&path).unwrap_or_default();
        if current != body {
            return Err(format!("{OUTPUT} is out of date: run `cargo xtask messages`"));
        }
        eprintln!("messages: ok ({} templates)", found.len());
        return Ok(());
    }
    std::fs::write(&path, body).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
    eprintln!("messages: wrote {} templates to {OUTPUT}", found.len());
    Ok(())
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if path.is_dir() {
            if name != "target" && name != "tests" && name != "fuzz" {
                collect(&path, out);
            }
        } else if name.ends_with(".rs") && name != "tests.rs" && path.components().any(|c| c.as_os_str() == "src") {
            out.push(path);
        }
    }
}

/// String literals in `#[error(...)]` attributes and in `EngineError::Variant(...)` calls.
fn literals_in_errors(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for marker in ["#[error(".to_string()].into_iter().chain(VARIANTS.iter().map(|v| format!("EngineError::{v}("))) {
        let mut from = 0;
        while let Some(at) = text[from..].find(&marker) {
            let start = from + at + marker.len();
            let end = matching_paren(text, start).unwrap_or(start);
            out.extend(string_literals(&text[start..end]));
            from = start;
        }
    }
    out
}

/// Index of the `)` closing a call whose arguments start at `start`, skipping string literals.
fn matching_paren(text: &str, start: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    let (mut depth, mut i) = (1usize, start);
    while i < bytes.len() {
        match bytes[i] {
            b'"' => {
                i += 1;
                while i < bytes.len() && bytes[i] != b'"' {
                    i += if bytes[i] == b'\\' { 2 } else { 1 };
                }
            }
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

fn string_literals(text: &str) -> Vec<String> {
    let (mut out, bytes) = (Vec::new(), text.as_bytes());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'"' {
            let mut lit = String::new();
            i += 1;
            while i < bytes.len() && bytes[i] != b'"' {
                if bytes[i] == b'\\' && i + 1 < bytes.len() {
                    lit.push(match bytes[i + 1] {
                        b'n' => '\n',
                        other => other as char,
                    });
                    i += 2;
                } else {
                    let ch = text[i..].chars().next().unwrap_or(' ');
                    lit.push(ch);
                    i += ch.len_utf8();
                }
            }
            out.push(lit);
        }
        i += 1;
    }
    out
}

/// A message template as translators see it: places keep their names, bare `{}` become `{1}`, `{2}`,
/// and format specs are dropped. Literals that are not sentences (paths, identifiers) are skipped.
fn normalise(lit: &str) -> Option<String> {
    if lit.contains('\n') || !(lit.contains(' ') || lit.len() > 12) || !lit.chars().any(|c| c.is_ascii_alphabetic()) {
        return None;
    }
    let (mut out, mut n) = (String::new(), 0);
    let mut rest = lit;
    while let Some(open) = rest.find(['{', '}']) {
        out.push_str(&rest[..open]);
        let tail = &rest[open..];
        if tail.starts_with("{{") || tail.starts_with("}}") {
            out.push_str(&tail[..2]);
            rest = &tail[2..];
            continue;
        }
        match tail.strip_prefix('{').and_then(|t| t.find('}')) {
            Some(close) => {
                let name = tail[1..=close].split(':').next().unwrap_or("");
                n += 1;
                out.push('{');
                out.push_str(&if name.is_empty() { n.to_string() } else { name.to_string() });
                out.push('}');
                rest = &tail[close + 2..];
            }
            None => {
                out.push_str(&tail[..1]);
                rest = &tail[1..];
            }
        }
    }
    out.push_str(rest);
    Some(out)
}
