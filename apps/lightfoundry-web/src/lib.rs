//! LightFoundry in the browser: the same engine and egui interface, compiled to WebAssembly.
//!
//! This build has no file dialogs and no serial ports yet (the browser's File System Access and Web
//! Serial APIs are the way to add them), so it is for designing, previewing and generating G-code,
//! and for driving the built-in simulated controller. Build it with `apps/lightfoundry-web/build.sh`.
//!
//! The wasm-bindgen glue is generated code, so the workspace's `unsafe_code` lint is relaxed here.
#![allow(unsafe_code)]

#[cfg(target_arch = "wasm32")]
mod web {
    use eframe::wasm_bindgen::JsCast;
    use lightfoundry_engine::Session;
    use lightfoundry_ui_egui::{LightFoundryApp, Platform};
    use wasm_bindgen::prelude::*;

    /// Starts the app on the `<canvas>` with the given id.
    #[wasm_bindgen]
    pub async fn start(canvas_id: String) -> Result<(), JsValue> {
        let document = web_sys::window().and_then(|w| w.document()).ok_or_else(|| JsValue::from_str("no document"))?;
        let canvas = document
            .get_element_by_id(&canvas_id)
            .ok_or_else(|| JsValue::from_str("no canvas with that id"))?
            .dyn_into::<web_sys::HtmlCanvasElement>()
            .map_err(|_| JsValue::from_str("that element is not a canvas"))?;
        eframe::WebRunner::new()
            .start(
                canvas,
                eframe::WebOptions::default(),
                Box::new(|_cc| Ok(Box::new(Host { app: LightFoundryApp::new(Session::new(), Box::new(WebPlatform)) }))),
            )
            .await
    }

    /// No dialogs, no serial ports, and no threads for a machine link.
    struct WebPlatform;

    impl Platform for WebPlatform {
        fn pick_open(&self, _: &str, _: &[&str]) -> Option<std::path::PathBuf> {
            None
        }

        fn pick_save(&self, _: &str, _: &str, _: &[&str]) -> Option<std::path::PathBuf> {
            None
        }

        fn machine_available(&self) -> bool {
            false
        }

        fn serial_ports(&self) -> Vec<String> {
            Vec::new()
        }

        fn open_serial(&self, _: &str) -> Result<Box<dyn lightfoundry_device::Transport + Send>, String> {
            Err("serial ports are not available in the browser build".into())
        }
    }

    struct Host {
        app: LightFoundryApp,
    }

    impl eframe::App for Host {
        fn ui(&mut self, ui: &mut eframe::egui::Ui, _frame: &mut eframe::Frame) {
            self.app.ui(ui);
        }
    }
}
