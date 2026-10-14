//! LightFoundry in the browser: the same engine and egui interface, compiled to WebAssembly.
//!
//! Files open through the browser's file chooser and save as downloads. There is no machine link
//! yet (the browser's Web Serial API is the way to add one). Build it with `apps/lightfoundry-web/build.sh`.
//!
//! The wasm-bindgen glue is generated code, so the workspace's `unsafe_code` lint is relaxed here.
#![allow(unsafe_code)]

#[cfg(target_arch = "wasm32")]
mod web {
    use eframe::wasm_bindgen::JsCast;
    use lightfoundry_engine::Session;
    use std::cell::RefCell;
    use std::rc::Rc;

    use lightfoundry_ui_egui::{LightFoundryApp, Platform, Upload};
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
                Box::new(|_cc| {
                    Ok(Box::new(Host { app: LightFoundryApp::new(Session::new(), Box::new(WebPlatform { inbox: Rc::new(RefCell::new(None)) })) }))
                }),
            )
            .await
    }

    /// Files go through the browser's file chooser and downloads; there are no paths, serial ports
    /// or threads for a machine link.
    struct WebPlatform {
        /// The chosen file once it has been read (shared with the browser callbacks).
        inbox: Rc<RefCell<Option<Upload>>>,
    }

    impl Platform for WebPlatform {
        fn pick_open(&self, _: &str, _: &[&str]) -> Option<std::path::PathBuf> {
            None
        }

        fn pick_save(&self, _: &str, _: &str, _: &[&str]) -> Option<std::path::PathBuf> {
            None
        }

        fn browser_files(&self) -> bool {
            true
        }

        fn request_upload(&self, extensions: &[&str]) {
            if let Err(e) = choose_file(extensions, Rc::clone(&self.inbox)) {
                log::warn!("could not open the file chooser: {e:?}");
            }
        }

        fn take_upload(&self) -> Option<Upload> {
            self.inbox.borrow_mut().take()
        }

        fn download(&self, name: &str, bytes: &[u8], mime: &str) {
            if let Err(e) = start_download(name, bytes, mime) {
                log::warn!("could not start the download: {e:?}");
            }
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

    /// Opens the browser's file chooser; the chosen file lands in `inbox` once read.
    fn choose_file(extensions: &[&str], inbox: Rc<RefCell<Option<Upload>>>) -> Result<(), JsValue> {
        let document = web_sys::window().and_then(|w| w.document()).ok_or_else(|| JsValue::from_str("no document"))?;
        let input = document.create_element("input")?.dyn_into::<web_sys::HtmlInputElement>()?;
        input.set_type("file");
        input.set_accept(&extensions.iter().map(|e| format!(".{e}")).collect::<Vec<_>>().join(","));
        let chosen = input.clone();
        let on_change = Closure::once(move || {
            let Some(file) = chosen.files().and_then(|f| f.get(0)) else { return };
            let name = file.name();
            let Ok(reader) = web_sys::FileReader::new() else { return };
            let done = reader.clone();
            let on_load = Closure::once(move || {
                if let Ok(result) = done.result() {
                    let bytes = js_sys::Uint8Array::new(&result).to_vec();
                    *inbox.borrow_mut() = Some(Upload { name, bytes });
                }
            });
            reader.set_onload(Some(on_load.as_ref().unchecked_ref()));
            on_load.forget();
            if let Err(e) = reader.read_as_array_buffer(&file) {
                log::warn!("could not read the file: {e:?}");
            }
        });
        input.add_event_listener_with_callback("change", on_change.as_ref().unchecked_ref())?;
        on_change.forget();
        input.click();
        Ok(())
    }

    /// Saves `bytes` through a temporary link, which is how a page triggers a download.
    fn start_download(name: &str, bytes: &[u8], mime: &str) -> Result<(), JsValue> {
        let document = web_sys::window().and_then(|w| w.document()).ok_or_else(|| JsValue::from_str("no document"))?;
        let array = js_sys::Uint8Array::from(bytes);
        let parts = js_sys::Array::of1(&array);
        let options = web_sys::BlobPropertyBag::new();
        options.set_type(mime);
        let blob = web_sys::Blob::new_with_u8_array_sequence_and_options(&parts, &options)?;
        let url = web_sys::Url::create_object_url_with_blob(&blob)?;
        let link = document.create_element("a")?.dyn_into::<web_sys::HtmlAnchorElement>()?;
        link.set_href(&url);
        link.set_download(name);
        link.click();
        web_sys::Url::revoke_object_url(&url)
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
