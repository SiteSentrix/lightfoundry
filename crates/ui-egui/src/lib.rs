//! The egui front end. Thin by design: panels read engine state and act only through
//! `Session::execute`, so everything the UI can do an agent or script can do too.

mod canvas;
mod geometry;
mod head_ui;
mod menus;
mod nodes_ui;
mod panels;
mod platform;
mod recent;
mod settings_ui;
pub(crate) mod snap;
mod theme;
mod timeline_ui;
mod view;

pub use platform::{NoPlatform, Platform};

use std::path::PathBuf;
use std::time::{Duration, Instant};

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
    /// Edit the nodes and handles of the selected path.
    Nodes,
    /// Draw Bézier paths node by node.
    Pen,
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
    thumbs: crate::geometry::ImageTextures,
    preview: PreviewCache,
    player: timeline_ui::Player,
    link: Option<Link>,
    /// Latest state of the machine link, refreshed once per frame.
    snap: Option<Snapshot>,
    last_job: Option<JobState>,
    console_input: String,
    /// The text being typed in the inspector (committed when the box loses focus).
    text_buffer: Option<(lightfoundry_doc::ShapeId, String)>,
    text_warning: Option<String>,
    jog_step: f64,
    offset_distance: f64,
    corner_radius: f64,
    array_columns: u32,
    array_rows: u32,
    array_gap: f64,
    array_count: u32,
    snapping: snap::SnapSettings,
    show_rulers: bool,
    guides: Vec<canvas::Guide>,
    node_sel: Option<nodes_ui::NodeSel>,
    node_drag: Option<nodes_ui::NodeDrag>,
    pen: Option<nodes_ui::Pen>,
    /// Cursor position on the canvas in mm (hover, not only while pressed).
    hover_world: Option<lightfoundry_geom::Point>,
    /// Where the canvas was drawn last frame (screen points).
    canvas_rect: Option<egui::Rect>,
    /// Alignment lines the last drag locked onto, for drawing: (x, y).
    snap_lines: (Option<f64>, Option<f64>),
    grid_cols: u32,
    grid_rows: u32,
    grid_speed: (f64, f64),
    grid_power: (f64, f64),
    grid_size: f64,
    array_radius: f64,
    offset_join: &'static str,
    offset_keep: bool,
    jog_feed: f64,
    target: Target,
    ports: Vec<String>,
    frame_power: f64,
    autosave_interval: Duration,
    autosave_revision: Option<u64>,
    last_autosave: Instant,
    /// A recovery file found at startup that the person has not yet decided about.
    pending_recovery: Option<PathBuf>,
    /// An action waiting for "save changes?" to be answered.
    pending_action: Option<PendingAction>,
    recent: recent::Recent,
    /// Percentages to cut off the left, top, right and bottom of the selected image.
    crop_pct: [f64; 4],
    sharpen_amount: f64,
    preset_name: String,
    /// While on, a click on the canvas sends the laser head there.
    move_head_mode: bool,
    /// x, y, width, height typed into the "create by numbers" fields.
    new_shape: [f64; 4],
    /// The project path last added to the recent list.
    recorded_path: Option<PathBuf>,
    allow_close: bool,
    last_title: String,
}

/// Actions that would discard unsaved changes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PendingAction {
    New,
    Open,
    OpenPath(PathBuf),
    Close,
}

/// How often a changed document is written to the recovery file.
pub const AUTOSAVE_INTERVAL: Duration = Duration::from_secs(20);

impl LightFoundryApp {
    pub fn new(session: Session, platform: Box<dyn Platform>) -> Self {
        let ports = platform.serial_ports();
        let pending_recovery = platform.recovery_file().filter(|p| p.exists());
        let recent = platform.recent_file().map(|p| recent::Recent::load(&p)).unwrap_or_default();
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
            thumbs: Default::default(),
            preview: PreviewCache::default(),
            player: timeline_ui::Player::default(),
            link: None,
            snap: None,
            last_job: None,
            console_input: String::new(),
            text_buffer: None,
            text_warning: None,
            jog_step: 1.0,
            offset_distance: 1.0,
            corner_radius: 0.0,
            array_columns: 3,
            array_rows: 3,
            array_gap: 5.0,
            array_count: 6,
            snapping: snap::SnapSettings::default(),
            show_rulers: true,
            guides: Vec::new(),
            node_sel: None,
            node_drag: None,
            pen: None,
            hover_world: None,
            canvas_rect: None,
            snap_lines: (None, None),
            grid_cols: 5,
            grid_rows: 5,
            grid_speed: (200.0, 1200.0),
            grid_power: (10.0, 60.0),
            grid_size: 10.0,
            array_radius: 40.0,
            offset_join: "round",
            offset_keep: true,
            jog_feed: 3000.0,
            target: Target::Simulator,
            ports,
            frame_power: 0.0,
            autosave_interval: AUTOSAVE_INTERVAL,
            autosave_revision: None,
            last_autosave: Instant::now(),
            pending_recovery,
            pending_action: None,
            recent,
            crop_pct: [0.0; 4],
            sharpen_amount: 1.0,
            preset_name: String::new(),
            move_head_mode: false,
            new_shape: [10.0, 10.0, 50.0, 30.0],
            recorded_path: None,
            allow_close: false,
            last_title: String::new(),
        }
    }

    /// Draws one frame. Called from the host's `eframe::App::ui`.
    pub fn ui(&mut self, ui: &mut egui::Ui) {
        self.poll_link(ui.ctx());
        self.autosave_tick(ui.ctx());
        self.watch_close_and_title(ui.ctx());
        self.track_recent();
        if !self.has_modal() {
            self.handle_shortcuts(ui.ctx());
        }
        menus::menu_bar(self, ui);
        egui::Panel::bottom("status").show(ui, |ui| self.status_bar(ui));
        egui::Panel::left("tools").resizable(false).exact_size(84.0).show(ui, |ui| panels::toolbar(self, ui));
        egui::Panel::right("inspector").default_size(300.0).show(ui, |ui| panels::inspector(self, ui));
        if self.show_preview {
            egui::Panel::bottom("timeline").show(ui, |ui| timeline_ui::bar(self, ui));
        }
        egui::CentralPanel::default().show(ui, |ui| canvas::show(self, ui));
        self.dialogs(ui.ctx());
    }

    fn has_modal(&self) -> bool {
        self.pending_recovery.is_some() || self.pending_action.is_some()
    }

    /// Writes the recovery copy while there are unsaved changes, and removes it once they are saved.
    fn autosave_tick(&mut self, ctx: &egui::Context) {
        let Some(path) = self.platform.recovery_file() else { return };
        // Until the person has decided about the previous session's copy, do not overwrite it.
        if self.pending_recovery.is_some() {
            return;
        }
        if !self.session.dirty {
            if self.autosave_revision.take().is_some() {
                self.platform.clear_recovery();
            }
            return;
        }
        if self.autosave_revision == Some(self.session.revision) {
            return;
        }
        // egui only repaints on input; without this an idle window would never get to autosave.
        let wait = self.autosave_interval.saturating_sub(self.last_autosave.elapsed());
        if !wait.is_zero() {
            ctx.request_repaint_after(wait);
            return;
        }
        self.last_autosave = Instant::now();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent); // a missing folder surfaces as the save error below
        }
        match self.session.execute("file.autosave", &serde_json::json!({ "path": path })) {
            Ok(_) => self.autosave_revision = Some(self.session.revision),
            Err(e) => log::warn!("autosave failed: {e}"),
        }
    }

    /// Keeps the window title current and turns a close request on unsaved work into a question.
    fn watch_close_and_title(&mut self, ctx: &egui::Context) {
        let title = window_title(&self.session);
        if title != self.last_title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.last_title = title;
        }
        if ctx.input(|i| i.viewport().close_requested()) && self.session.dirty && !self.allow_close {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.pending_action = Some(PendingAction::Close);
        }
    }

    /// Adds the project to the recent list whenever the document's path changes (open, save as).
    fn track_recent(&mut self) {
        if self.session.path == self.recorded_path {
            return;
        }
        self.recorded_path = self.session.path.clone();
        if let Some(path) = self.recorded_path.clone() {
            self.recent.push(path);
            self.save_recent();
        }
    }

    fn forget_recent(&mut self, path: &std::path::Path) {
        self.recent.remove(path);
        self.save_recent();
    }

    fn save_recent(&self) {
        if let Some(file) = self.platform.recent_file() {
            self.recent.save(&file);
        }
    }

    /// Asks before discarding unsaved work, then runs `action`.
    pub(crate) fn request(&mut self, action: PendingAction) {
        if self.session.dirty {
            self.pending_action = Some(action);
        } else {
            self.proceed(action);
        }
    }

    fn proceed(&mut self, action: PendingAction) {
        match action {
            PendingAction::New => {
                self.run("file.new", Value::Null);
                self.platform.clear_recovery();
                self.autosave_revision = None;
                self.view = None;
            }
            PendingAction::Open => {
                if let Some(path) = self.platform.pick_open("Open project", &["lightfoundry"]) {
                    self.run("file.open", serde_json::json!({ "path": path }));
                    self.platform.clear_recovery();
                    self.autosave_revision = None;
                    self.view = None;
                }
            }
            PendingAction::OpenPath(path) => {
                if self.run("file.open", serde_json::json!({ "path": path })).is_some() {
                    self.platform.clear_recovery();
                    self.autosave_revision = None;
                    self.view = None;
                } else {
                    // A file that is gone or unreadable should not stay on the menu.
                    self.forget_recent(&path);
                }
            }
            PendingAction::Close => {
                self.allow_close = true;
                self.platform.clear_recovery();
            }
        }
    }

    /// The person's answer to "recover unsaved work?".
    fn answer_recovery(&mut self, recover: bool) {
        let Some(path) = self.pending_recovery.take() else { return };
        if recover {
            self.run("file.recover", serde_json::json!({ "path": path }));
            self.view = None;
        } else {
            self.platform.clear_recovery();
        }
    }

    /// The person's answer to "save changes first?". Saving that does not complete (cancelled file
    /// dialog, failed write) keeps the question open: the work is never dropped by accident.
    fn answer_unsaved(&mut self, answer: Answer) {
        let Some(action) = self.pending_action.clone() else { return };
        match answer {
            Answer::Save => {
                if menus::save(self, false) {
                    self.pending_action = None;
                    self.proceed(action);
                }
            }
            Answer::Discard => {
                self.pending_action = None;
                self.proceed(action);
            }
            Answer::Cancel => self.pending_action = None,
        }
    }

    fn dialogs(&mut self, ctx: &egui::Context) {
        if self.pending_recovery.is_some() {
            let mut choice = None;
            egui::Modal::new(egui::Id::new("recovery")).show(ctx, |ui| {
                ui.heading("Recover unsaved work?");
                ui.label("LightFoundry found work from a previous session that was never saved.");
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("Recover").clicked() {
                        choice = Some(true);
                    }
                    if ui.button("Discard").clicked() {
                        choice = Some(false);
                    }
                });
            });
            if let Some(recover) = choice {
                self.answer_recovery(recover);
            }
            return;
        }
        let Some(action) = self.pending_action.clone() else { return };
        let name = self.session.path.as_ref().and_then(|p| p.file_name()).map_or("Untitled".to_string(), |n| n.to_string_lossy().into_owned());
        let mut answer = None;
        egui::Modal::new(egui::Id::new("unsaved")).show(ctx, |ui| {
            ui.heading("Unsaved changes");
            ui.label(format!("Save changes to {name} first?"));
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button("Save").clicked() {
                    answer = Some(Answer::Save);
                }
                if ui.button("Discard").clicked() {
                    answer = Some(Answer::Discard);
                }
                if ui.button("Cancel").clicked() {
                    answer = Some(Answer::Cancel);
                }
            });
        });
        if let Some(answer) = answer {
            self.answer_unsaved(answer);
        }
        if action == PendingAction::Close && self.allow_close {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
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
        self.move_head_mode = false;
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
        // Ungroup (Ctrl+Shift+G) before group (Ctrl+G), like redo before undo.
        let (ungroup, group) =
            ctx.input_mut(|i| (i.consume_key(Modifiers::COMMAND | Modifiers::SHIFT, Key::G), i.consume_key(Modifiers::COMMAND, Key::G)));
        if ungroup {
            self.run("edit.ungroup", Value::Null);
        } else if group {
            self.run("edit.group", Value::Null);
        }
        if all {
            self.run("edit.select_all", Value::Null);
        }
        if dup {
            self.run("edit.duplicate", Value::Null);
        }
        if delete && self.tool == Tool::Nodes && self.node_sel.is_some() {
            nodes_ui::delete_selected(self);
        } else if delete && !self.session.selection.is_empty() {
            self.run("edit.delete", Value::Null);
        }
        let (enter, escape) = ctx.input_mut(|i| (i.consume_key(Modifiers::NONE, Key::Enter), i.consume_key(Modifiers::NONE, Key::Escape)));
        if enter && self.pen.is_some() {
            nodes_ui::finish(self, false);
        }
        if escape {
            if self.move_head_mode {
                self.move_head_mode = false;
            } else if self.pen.is_some() || self.node_drag.is_some() {
                nodes_ui::cancel(self);
            } else if self.tool != Tool::Select {
                self.tool = Tool::Select;
            }
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

pub(crate) enum Answer {
    Save,
    Discard,
    Cancel,
}

/// `name — LightFoundry`, with `*` while there are unsaved changes.
pub(crate) fn window_title(session: &Session) -> String {
    let name = session.path.as_ref().and_then(|p| p.file_name()).map_or("Untitled".to_string(), |n| n.to_string_lossy().into_owned());
    format!("{}{name} — LightFoundry", if session.dirty { "*" } else { "" })
}
