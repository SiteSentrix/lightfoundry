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

use lightfoundry_device::{JobState, Link, SimulatedGrbl, Snapshot};
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
    Text,
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
    link: Option<Link>,
    /// Latest state of the machine link, refreshed once per frame.
    snap: Option<Snapshot>,
    last_job: Option<JobState>,
    console_input: String,
    /// The text being typed in the inspector (committed when the box loses focus).
    text_buffer: Option<(lightfoundry_doc::ShapeId, String)>,
    text_warning: Option<String>,
    jog_step: f64,
    jog_feed: f64,
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
            link: None,
            snap: None,
            last_job: None,
            console_input: String::new(),
            text_buffer: None,
            text_warning: None,
            jog_step: 1.0,
            jog_feed: 3000.0,
            target: Target::Simulator,
            ports,
            frame_power: 0.0,
        }
    }

    /// Draws one frame. Called from the host's `eframe::App::ui`.
    pub fn ui(&mut self, ui: &mut egui::Ui) {
        self.poll_link(ui.ctx());
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

    /// Remembers characters the stroke font cannot draw so the inspector can say so.
    pub(crate) fn note_unsupported(&mut self, result: &Value) {
        let chars: Vec<String> =
            result["unsupported"].as_array().map(|a| a.iter().filter_map(|c| c.as_str().map(String::from)).collect()).unwrap_or_default();
        self.text_warning = (!chars.is_empty()).then(|| format!("No glyph for: {} (drawn as a box)", chars.join(" ")));
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

    /// Opens the selected machine (or the simulator) and keeps the connection until `disconnect`.
    pub(crate) fn connect(&mut self) {
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
                self.link = Some(Link::connect(t));
                self.say("Connected");
            }
            Err(e) => self.status = Some((format!("Could not open the machine: {e}"), true)),
        }
    }

    /// Dropping the link cancels any running job (soft reset) before the connection closes.
    pub(crate) fn disconnect(&mut self) {
        self.link = None;
        self.snap = None;
        self.last_job = None;
        self.say("Disconnected");
    }

    pub(crate) fn start_job(&mut self, program: String) {
        let Some(link) = &self.link else {
            self.status = Some(("Connect to a machine (or the simulator) first".into(), true));
            return;
        };
        if let Err(e) = link.start_job(program) {
            self.status = Some((e.to_string(), true));
        }
    }

    /// Runs a link command and shows a refusal in the status bar.
    pub(crate) fn link_do(&mut self, f: impl FnOnce(&Link) -> Result<(), lightfoundry_device::LinkError>) {
        let Some(link) = &self.link else {
            self.status = Some(("Not connected".into(), true));
            return;
        };
        if let Err(e) = f(link) {
            self.status = Some((e.to_string(), true));
        }
    }

    pub(crate) fn job_active(&self) -> bool {
        self.snap.as_ref().is_some_and(|s| matches!(s.job, JobState::Running | JobState::Paused))
    }

    fn poll_link(&mut self, ctx: &egui::Context) {
        let Some(link) = &self.link else { return };
        // The worker updates continuously; poll at a modest rate rather than every frame forever.
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
        let snap = link.snapshot();
        if !snap.connected {
            self.status = Some(("The machine connection was lost".into(), true));
            self.link = None;
            self.snap = None;
            return;
        }
        if self.last_job.as_ref() != Some(&snap.job) {
            if let JobState::Finished(result) = &snap.job {
                self.status = Some(match result {
                    Ok(lines) => (format!("Job finished: {lines} lines"), false),
                    Err(e) => (format!("Job stopped: {e}"), true),
                });
            }
            self.last_job = Some(snap.job.clone());
        }
        self.snap = Some(snap);
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
