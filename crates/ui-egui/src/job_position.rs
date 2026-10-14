//! Remembering how far a job got, so it can be restarted after a power cut or a crash.
//!
//! While a job runs, its progress is written to a small file every few seconds. A job that finishes
//! cleanly removes it; anything else leaves it for the next start to offer.

use std::path::Path;

use serde_json::{Value, json};

/// Seconds between writes while a job runs.
pub const WRITE_EVERY_SECS: u64 = 5;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobPosition {
    /// Fingerprint of the program that was running, to tell whether it is still the same job.
    pub hash: u64,
    /// The program line to restart from (already backed up a little).
    pub line: usize,
    pub total_lines: usize,
}

/// FNV-1a over the program text; stable between runs, unlike the standard hasher.
pub fn fingerprint(program: &str) -> u64 {
    program.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3))
}

pub fn save(path: &Path, pos: &JobPosition) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let text = json!({ "hash": pos.hash.to_string(), "line": pos.line, "total_lines": pos.total_lines }).to_string();
    if let Err(e) = std::fs::write(path, text) {
        log::warn!("could not save the job position to {}: {e}", path.display());
    }
}

pub fn clear(path: &Path) {
    if let Err(e) = std::fs::remove_file(path)
        && e.kind() != std::io::ErrorKind::NotFound
    {
        log::warn!("could not remove {}: {e}", path.display());
    }
}

/// Reads a saved position; a missing, damaged or nonsensical file is no position.
pub fn load(path: &Path) -> Option<JobPosition> {
    let text = std::fs::read_to_string(path).ok()?;
    let v: Value = serde_json::from_str(&text).ok()?;
    let hash = v.get("hash")?.as_str()?.parse().ok()?;
    let line = usize::try_from(v.get("line")?.as_u64()?).ok()?;
    let total_lines = usize::try_from(v.get("total_lines")?.as_u64()?).ok()?;
    (line >= 1 && line <= total_lines).then_some(JobPosition { hash, line, total_lines })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_saved_position_reads_back_and_clearing_removes_it() {
        let dir = std::env::temp_dir().join(format!("lf-jobpos-{}", std::process::id()));
        let file = dir.join("pos.json");
        let pos = JobPosition { hash: fingerprint("G1 X1"), line: 12, total_lines: 40 };
        save(&file, &pos);
        assert_eq!(load(&file), Some(pos));
        clear(&file);
        assert_eq!(load(&file), None);
        clear(&file);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn damaged_or_impossible_files_are_ignored() {
        let dir = std::env::temp_dir().join(format!("lf-jobpos-bad-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("pos.json");
        for text in [
            "",
            "nope",
            "{}",
            r#"{"hash":"x","line":1,"total_lines":2}"#,
            r#"{"hash":"1","line":0,"total_lines":2}"#,
            r#"{"hash":"1","line":5,"total_lines":2}"#,
            r#"{"hash":"1","line":-3,"total_lines":2}"#,
        ] {
            std::fs::write(&file, text).unwrap();
            assert_eq!(load(&file), None, "{text}");
        }
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn the_fingerprint_tells_programs_apart() {
        assert_eq!(fingerprint("G1 X1"), fingerprint("G1 X1"));
        assert_ne!(fingerprint("G1 X1"), fingerprint("G1 X2"));
    }
}
