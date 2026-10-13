//! The egui front end. Thin by design: panels read engine state and act only through
//! `Session::execute`, so everything the UI can do an agent or script can do too.

mod canvas;
mod geometry;
mod menus;
mod panels;
mod platform;
mod theme;
mod view;

pub use platform::{NoPlatform, Platform};

use std::path::PathBuf;

use lightfoundry_device::{JobRunner, SimulatedGrbl};
use lightfoundry_doc::LayerId;
use lightfoundry_engine::{EngineError, Session};
use serde_json::Value;

use crate::geometry::{GeometryCache, PreviewCache};
use crate::theme::Tokens;
use crate::view::View;

/// Interactive tools on the left toolbar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Select,
    Rectangle,
    Ellipse,
    Line,
    Polygon,
}

/// Where Start sends the program.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    Simulator,
    Serial(String),
}

pub struct LightFoundryApp {
    pub session: Session,
    platform: Box<dyn Platform>,
    tokens: Tokens,
    view: Option<View>,
    tool: Tool,
    drag: canvas::Drag,
    active_layer: LayerId,
    show_preview: bool,
    show_travel: bool,
    polygon_sides: u32,
    status: Option<(String, bool)>,
    geometry: GeometryCache,
    preview: PreviewCache,
    runner: Option<JobRunner>,
    run_result: Option<Result<String, String>>,
    target: Target,
    ports: Vec<String>,
    frame_power: f64,
}

impl LightFoundryApp {
    pub fn new(session: Session, platform: Box<dyn Platform>) -> Self {
        let ports = platform.serial_ports();
        Self {
            session,
            platform,
            tokens: Tokens::DARK,
            view: None,
            tool: Tool::Select,
            drag: canvas::Drag::None,
            active_layer: 0,
            show_preview: false,
            show_travel: false,
            polygon_sides: 6,
            status: None,
            geometry: GeometryCache::default(),
            preview: PreviewCache::default(),
            runner: None,
            run_result: None,
            target: Target::Simulator,
            ports,
            frame_power: 0.0,
        }
    }

    /// Draws one frame. Called from the host's `eframe::App::ui`.
    pub fn ui(&mut self, ui: &mut egui::Ui) {
        self.poll_runner(ui.ctx());
        self.handle_shortcuts(ui.ctx());
        menus::menu_bar(self, ui);
        egui::Panel::bottom("status").show(ui, |ui| self.status_bar(ui));
        egui::Panel::left("tools").resizable(false).exact_size(84.0).show(ui, |ui| panels::toolbar(self, ui));
        egui::Panel::right("inspector").default_size(300.0).show(ui, |ui| panels::inspector(self, ui));
        egui::CentralPanel::default().show(ui, |ui| canvas::show(self, ui));
    }

    /// Runs a command, reporting failures in the status bar instead of dropping them.
    pub(crate) fn run(&mut self, id: &str, params: Value) -> Option<Value> {
        match self.session.execute(id, &params) {
            Ok(v) => Some(v),
            Err(e) => {
                self.report_error(&e);
                None
            }
        }
    }

    pub(crate) fn say(&mut self, text: impl Into<String>) {
        self.status = Some((text.into(), false));
    }

    pub(crate) fn report_error(&mut self, e: &EngineError) {
        self.status = Some((e.to_string(), true));
    }

    pub(crate) fn pick_open(&self, title: &str, exts: &[&str]) -> Option<PathBuf> {
        self.platform.pick_open(title, exts)
    }

    pub(crate) fn pick_save(&self, title: &str, name: &str, exts: &[&str]) -> Option<PathBuf> {
        self.platform.pick_save(title, name, exts)
    }

    pub(crate) fn start_job(&mut self, program: String) {
        if self.runner.as_ref().is_some_and(JobRunner::is_running) {
            self.status = Some(("A job is already running".into(), true));
            return;
        }
        let transport: Result<Box<dyn lightfoundry_device::Transport + Send>, String> = match &self.target {
            Target::Simulator => {
                let mut sim = SimulatedGrbl::new();
                sim.set_latency(std::time::Duration::from_millis(3));
                Ok(Box::new(sim))
            }
            Target::Serial(port) => self.platform.open_serial(port),
        };
        match transport {
            Ok(t) => {
                self.run_result = None;
                self.runner = Some(JobRunner::start(t, program));
            }
            Err(e) => self.status = Some((format!("Could not open the machine: {e}"), true)),
        }
    }

    fn poll_runner(&mut self, ctx: &egui::Context) {
        let Some(runner) = self.runner.as_mut() else { return };
        if runner.is_running() {
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        }
        if let Some(outcome) = runner.poll() {
            let (text, is_err) = match &outcome {
                Ok(r) => (format!("Job finished: {} lines sent", r.lines), false),
                Err(e) => (e.to_string(), true),
            };
            self.status = Some((text.clone(), is_err));
            self.run_result = Some(outcome.map(|_| text).map_err(|e| e.to_string()));
        }
    }

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        use egui::{Key, Modifiers};
        // Redo (Ctrl+Shift+Z) is consumed before undo (Ctrl+Z) so the shifted chord never counts as undo.
        let (redo, undo, all, dup, delete) = ctx.input_mut(|i| {
            (
                i.consume_key(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z) || i.consume_key(Modifiers::COMMAND, Key::Y),
                i.consume_key(Modifiers::COMMAND, Key::Z),
                i.consume_key(Modifiers::COMMAND, Key::A),
                i.consume_key(Modifiers::COMMAND, Key::D),
                i.consume_key(Modifiers::NONE, Key::Delete) || i.consume_key(Modifiers::NONE, Key::Backspace),
            )
        });
        if redo {
            self.run("edit.redo", Value::Null);
        } else if undo {
            self.run("edit.undo", Value::Null);
        }
        if all {
            self.run("edit.select_all", Value::Null);
        }
        if dup {
            self.run("edit.duplicate", Value::Null);
        }
        if delete && !self.session.selection.is_empty() {
            self.run("edit.delete", Value::Null);
        }
        let nudge = ctx.input_mut(|i| {
            let step = if i.modifiers.shift { 10.0 } else { 1.0 };
            let mut d = (0.0, 0.0);
            for (key, dx, dy) in [(Key::ArrowLeft, -step, 0.0), (Key::ArrowRight, step, 0.0), (Key::ArrowUp, 0.0, -step), (Key::ArrowDown, 0.0, step)]
            {
                if i.consume_key(Modifiers::NONE, key) || i.consume_key(Modifiers::SHIFT, key) {
                    d = (d.0 + dx, d.1 + dy);
                }
            }
            d
        });
        if nudge != (0.0, 0.0) && !self.session.selection.is_empty() {
            self.run("edit.move", serde_json::json!({ "dx": nudge.0, "dy": nudge.1 }));
        }
    }

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| match &self.status {
            Some((text, true)) => {
                ui.colored_label(self.tokens.error, text);
            }
            Some((text, false)) => {
                ui.colored_label(self.tokens.ok, text);
            }
            None => {
                ui.label("Ready");
            }
        });
    }
}

#[cfg(test)]
mod tests;
