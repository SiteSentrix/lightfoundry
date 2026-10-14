use std::path::PathBuf;

use lightfoundry_device::Transport;

/// A file the person picked in a browser-style host: its name and contents.
#[derive(Debug, Clone, PartialEq)]
pub struct Upload {
    pub name: String,
    pub bytes: Vec<u8>,
}

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

    /// Where the list of recent projects is kept, if this platform has somewhere to keep it.
    fn recent_file(&self) -> Option<PathBuf> {
        None
    }

    /// Browser-style hosts cannot hand out file paths: files arrive as bytes after the person has
    /// chosen them ([`request_upload`](Platform::request_upload), then
    /// [`take_upload`](Platform::take_upload)) and leave as downloads.
    fn browser_files(&self) -> bool {
        false
    }

    /// Asks the person to choose a file with one of these extensions.
    fn request_upload(&self, _extensions: &[&str]) {}

    /// The file chosen after a request, once it has been read.
    fn take_upload(&self) -> Option<Upload> {
        None
    }

    /// Offers `bytes` to the person as a downloaded file called `name`.
    fn download(&self, _name: &str, _bytes: &[u8], _mime: &str) {}

    /// Whether this platform can run a machine link at all (it needs threads, which the browser
    /// build does not have).
    fn machine_available(&self) -> bool {
        true
    }

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
