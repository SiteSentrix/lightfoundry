use std::path::PathBuf;

use lightfoundry_device::Transport;
use lightfoundry_mcp::link::LinkServer;
use lightfoundry_ui_egui::{LightFoundryApp, Platform};

pub struct Host {
    pub app: LightFoundryApp,
    /// Present only when the person switched agent access on; closing the app closes it.
    pub agent_link: Option<LinkServer>,
}

impl eframe::App for Host {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if let Some(link) = &mut self.agent_link {
            link.serve_pending(&mut self.app.session);
        }
        self.app.ui(ui);
    }
}

/// Native file dialogs and serial ports.
pub struct DesktopPlatform;

impl Platform for DesktopPlatform {
    fn recovery_file(&self) -> Option<PathBuf> {
        Some(data_dir()?.join("LightFoundry").join("recovery.lightfoundry"))
    }

    fn job_position_file(&self) -> Option<PathBuf> {
        Some(data_dir()?.join("LightFoundry").join("job-position.json"))
    }

    fn translation_file(&self) -> Option<PathBuf> {
        lightfoundry_i18n::default_file()
    }

    fn preset_library_file(&self) -> Option<PathBuf> {
        Some(data_dir()?.join("LightFoundry").join("presets.json"))
    }

    fn templates_dir(&self) -> Option<PathBuf> {
        Some(data_dir()?.join("LightFoundry").join("templates"))
    }

    fn recent_file(&self) -> Option<PathBuf> {
        Some(data_dir()?.join("LightFoundry").join("recent.json"))
    }

    fn shortcuts_file(&self) -> Option<PathBuf> {
        Some(data_dir()?.join("LightFoundry").join("shortcuts.json"))
    }

    fn clear_recovery(&self) {
        if let Some(path) = self.recovery_file()
            && let Err(e) = std::fs::remove_file(&path)
            && e.kind() != std::io::ErrorKind::NotFound
        {
            log::warn!("could not remove {}: {e}", path.display());
        }
    }

    fn pick_open(&self, title: &str, extensions: &[&str]) -> Option<PathBuf> {
        rfd::FileDialog::new().set_title(title).add_filter("Supported files", extensions).pick_file()
    }

    fn pick_save(&self, title: &str, default_name: &str, extensions: &[&str]) -> Option<PathBuf> {
        rfd::FileDialog::new().set_title(title).set_file_name(default_name).add_filter("File", extensions).save_file()
    }

    #[cfg(feature = "serial")]
    fn serial_ports(&self) -> Vec<String> {
        lightfoundry_device::serial::list_ports()
    }

    #[cfg(not(feature = "serial"))]
    fn serial_ports(&self) -> Vec<String> {
        Vec::new()
    }

    #[cfg(feature = "serial")]
    fn open_serial(&self, port: &str) -> Result<Box<dyn Transport + Send>, String> {
        let t = lightfoundry_device::serial::SerialTransport::open(port, lightfoundry_device::serial::DEFAULT_BAUD).map_err(|e| e.to_string())?;
        Ok(Box::new(t))
    }

    #[cfg(not(feature = "serial"))]
    fn open_serial(&self, _port: &str) -> Result<Box<dyn Transport + Send>, String> {
        Err("this build has no serial support".into())
    }
}

/// The per-user data folder: `$XDG_DATA_HOME` or `~/.local/share` on Linux, `~/Library/Application
/// Support` on macOS, `%APPDATA%` on Windows.
fn data_dir() -> Option<PathBuf> {
    let var = |name: &str| std::env::var_os(name).filter(|v| !v.is_empty()).map(PathBuf::from);
    if cfg!(target_os = "windows") {
        var("APPDATA")
    } else if cfg!(target_os = "macos") {
        var("HOME").map(|h| h.join("Library").join("Application Support"))
    } else {
        var("XDG_DATA_HOME").or_else(|| var("HOME").map(|h| h.join(".local").join("share")))
    }
}
