//! LightFoundry desktop app: hosts the egui front end in an eframe window.

mod host;

use std::path::PathBuf;

use lightfoundry_engine::Session;
use lightfoundry_ui_egui::LightFoundryApp;
use serde_json::json;

fn main() -> eframe::Result {
    env_logger::init();
    let mut session = Session::new();
    if let Some(path) = std::env::args().nth(1).map(PathBuf::from) {
        let (id, key) = match path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).as_deref() {
            Some("svg") => ("file.import_svg", "path"),
            Some("dxf") => ("file.import_dxf", "path"),
            Some("png" | "jpg" | "jpeg" | "bmp" | "gif" | "webp") => ("file.import_image", "path"),
            _ => ("file.open", "path"),
        };
        if let Err(e) = session.execute(id, &json!({ key: path })) {
            log::error!("could not open {}: {e}", path.display());
        }
    }

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1280.0, 800.0]).with_min_inner_size([900.0, 560.0]).with_title("LightFoundry"),
        ..Default::default()
    };
    eframe::run_native(
        "LightFoundry",
        options,
        Box::new(|_cc| Ok(Box::new(host::Host { app: LightFoundryApp::new(session, Box::new(host::DesktopPlatform)) }))),
    )
}
