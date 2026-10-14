//! The list of recently opened or saved projects, kept in a small JSON file the platform provides.

use std::path::{Path, PathBuf};

/// Entries kept.
pub const MAX_RECENT: usize = 10;
/// A recent-files file larger than this is not read (it is not ours).
const MAX_FILE_BYTES: u64 = 256 * 1024;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Recent {
    /// Most recent first.
    pub files: Vec<PathBuf>,
}

impl Recent {
    /// Reads the list; a missing, oversized or damaged file gives an empty one.
    pub fn load(path: &Path) -> Recent {
        let Ok(meta) = std::fs::metadata(path) else { return Recent::default() };
        if meta.len() > MAX_FILE_BYTES {
            return Recent::default();
        }
        let Ok(text) = std::fs::read_to_string(path) else { return Recent::default() };
        let Ok(list) = serde_json::from_str::<Vec<serde_json::Value>>(&text) else { return Recent::default() };
        let mut recent = Recent::default();
        for entry in list.iter().rev() {
            if let Some(p) = entry.as_str().filter(|p| !p.is_empty()) {
                recent.push(PathBuf::from(p));
            }
        }
        recent
    }

    pub fn save(&self, path: &Path) {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent); // a failure shows up as the write error below
        }
        let list: Vec<&str> = self.files.iter().filter_map(|p| p.to_str()).collect();
        match serde_json::to_string(&list) {
            Ok(text) => {
                if let Err(e) = std::fs::write(path, text) {
                    log::warn!("could not write {}: {e}", path.display());
                }
            }
            Err(e) => log::warn!("could not encode the recent files: {e}"),
        }
    }

    /// Moves `path` to the front, dropping duplicates and anything beyond [`MAX_RECENT`].
    pub fn push(&mut self, path: PathBuf) {
        self.files.retain(|p| *p != path);
        self.files.insert(0, path);
        self.files.truncate(MAX_RECENT);
    }

    pub fn remove(&mut self, path: &Path) {
        self.files.retain(|p| p != path);
    }
}
