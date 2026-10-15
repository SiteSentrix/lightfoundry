use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Mutex, mpsc};

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
#[derive(Default)]
pub struct DesktopPlatform {
    /// The font library download in progress, if any: its result arrives here.
    font_job: Mutex<Option<mpsc::Receiver<Result<String, String>>>>,
}

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

    fn font_library_available(&self) -> bool {
        lightfoundry_fontlib::default_dir().is_some()
    }

    fn font_library_dir(&self) -> Option<PathBuf> {
        lightfoundry_fontlib::default_dir()
    }

    fn start_font_library_download(&self, everything: bool) -> bool {
        let (Some(dest), Ok(mut job)) = (lightfoundry_fontlib::default_dir(), self.font_job.lock()) else { return false };
        if job.is_some() {
            return false;
        }
        let (sender, receiver) = mpsc::channel();
        *job = Some(receiver);
        let set = if everything { lightfoundry_fontlib::Set::Everything } else { lightfoundry_fontlib::Set::Foundry };
        std::thread::spawn(move || {
            let cancel = AtomicBool::new(false);
            let result = lightfoundry_fontlib::download(&dest, &lightfoundry_fontlib::Source::default(), set, &cancel, &mut |_, _| {})
                .map(|s| {
                    format!(
                        "Font library: {} downloaded, {} already there, in {}. The Foundry fonts are under Library fonts in the text inspector.",
                        s.fetched,
                        s.kept,
                        dest.display()
                    )
                })
                .map_err(|e| e.to_string());
            let _ = sender.send(result);
        });
        true
    }

    fn take_font_library_result(&self) -> Option<Result<String, String>> {
        let mut job = self.font_job.lock().ok()?;
        match job.as_ref()?.try_recv() {
            Ok(result) => {
                *job = None;
                Some(result)
            }
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => {
                *job = None;
                Some(Err("the font download stopped unexpectedly".into()))
            }
        }
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
