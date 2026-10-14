use std::path::PathBuf;

use lightfoundry_device::Transport;

/// What the host operating system provides. Keeping this behind a trait keeps file dialogs and
/// serial ports out of the UI crate (and lets tests and the web build run without them).
pub trait Platform {
    fn pick_open(&self, title: &str, extensions: &[&str]) -> Option<PathBuf>;
    fn pick_save(&self, title: &str, default_name: &str, extensions: &[&str]) -> Option<PathBuf>;
    /// Where the autosave copy lives, if this platform has somewhere to keep one.
    fn recovery_file(&self) -> Option<PathBuf> {
        None
    }

    /// Removes the autosave copy (the work was saved or knowingly discarded).
    fn clear_recovery(&self) {}

    fn serial_ports(&self) -> Vec<String>;
    fn open_serial(&self, port: &str) -> Result<Box<dyn Transport + Send>, String>;
}

/// No dialogs, no serial ports: only the built-in simulator is available.
pub struct NoPlatform;

impl Platform for NoPlatform {
    fn pick_open(&self, _: &str, _: &[&str]) -> Option<PathBuf> {
        None
    }

    fn pick_save(&self, _: &str, _: &str, _: &[&str]) -> Option<PathBuf> {
        None
    }

    fn serial_ports(&self) -> Vec<String> {
        Vec::new()
    }

    fn open_serial(&self, _: &str) -> Result<Box<dyn Transport + Send>, String> {
        Err("serial ports are not available on this platform".into())
    }
}
