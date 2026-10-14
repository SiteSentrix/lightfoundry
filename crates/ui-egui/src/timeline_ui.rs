//! The preview timeline: a bar under the canvas with play/pause, a scrubber and the time spent per
//! layer, and the maths that decides how much of the toolpath has been "burned" at a given moment.

use egui::{Color32, Stroke, pos2, vec2};
use lightfoundry_geom::Point;
use lightfoundry_toolpath::Moment;

use crate::LightFoundryApp;
use crate::geometry::PreviewSegment;
use crate::menus::format_duration;

/// Seconds a whole job takes to play back when the speed is left on automatic.
const AUTO_PLAY_SECONDS: f64 = 30.0;
/// Playback speeds offered, as multiples of real time (0 = automatic).
const SPEEDS: [f64; 5] = [0.0, 1.0, 10.0, 60.0, 600.0];

/// Playback state of the preview. `position == None` shows the whole job.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Player {
    pub position: Option<f64>,
    pub playing: bool,
    /// Index into [`SPEEDS`].
    pub speed: usize,
}

/// How much of a preview segment has happened at a given moment.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Reveal {
    Hidden,
    Full,
    /// This fraction (0..1) of the segment, counted from its start.
    Partial(f32),
}

pub fn reveal(seg: &PreviewSegment, now: Option<Moment>) -> Reveal {
    let Some(now) = now else { return Reveal::Full };
    let op = seg.op as usize;
    if op < now.op || (op == now.op && now.pass > 0) {
        return Reveal::Full;
    }
    if op > now.op {
        return Reveal::Hidden;
    }
    let into = now.into_pass as f32;
    if into >= seg.t1 {
        Reveal::Full
    } else if into <= seg.t0 {
        Reveal::Hidden
    } else {
        Reveal::Partial((into - seg.t0) / (seg.t1 - seg.t0))
    }
}

/// Where the head is at `now`: inside the segment being cut, else the end of the last one done.
pub fn head_position(segments: &[PreviewSegment], now: Moment) -> Option<Point> {
    let into = now.into_pass as f32;
    let mut last = None;
    for seg in segments.iter().filter(|s| s.op as usize == now.op) {
        if into >= seg.t0 && into < seg.t1 {
            let f = f64::from((into - seg.t0) / (seg.t1 - seg.t0));
            return Some(seg.a.lerp(seg.b, f));
        }
        if seg.t1 <= into {
            last = Some(seg.b);
        }
    }
    last
}

pub fn bar(app: &mut LightFoundryApp, ui: &mut egui::Ui) {
    let total = app.preview.timeline.total;
    if app.preview.error.is_some() || app.preview.timeline.ops.is_empty() {
        ui.label("Nothing to preview: no layer with output has any shapes.");
        return;
    }
    advance(app, ui, total);
    let speed = SPEEDS.get(app.player.speed).copied().unwrap_or(0.0);

    ui.horizontal(|ui| {
        let label = if app.player.playing { "Pause" } else { "Play" };
        if ui.button(label).clicked() {
            if !app.player.playing && app.player.position.is_none_or(|p| p >= total) {
                app.player.position = Some(0.0);
            }
            app.player.playing = !app.player.playing;
        }
        if ui.button("Show all").clicked() {
            app.player.position = None;
            app.player.playing = false;
        }
        let mut t = app.player.position.unwrap_or(total);
        let width = (ui.available_width() - 260.0).max(80.0);
        ui.spacing_mut().slider_width = width;
        if ui.add(egui::Slider::new(&mut t, 0.0..=total).show_value(false)).changed() {
            app.player.position = Some(t);
        }
        ui.label(format!("{} / {}", format_duration(t), format_duration(total)));
        egui::ComboBox::from_id_salt("preview_speed").width(70.0).selected_text(speed_label(speed)).show_ui(ui, |ui| {
            for (i, s) in SPEEDS.iter().enumerate() {
                ui.selectable_value(&mut app.player.speed, i, speed_label(*s));
            }
        });
    });
    ui.horizontal_wrapped(|ui| {
        for l in app.preview.timeline.layers() {
            let Some(layer) = app.session.doc.layer(l.layer) else { continue };
            let color = crate::theme::on_dark(crate::theme::parse_hex(&layer.color));
            let (rect, _) = ui.allocate_exact_size(vec2(10.0, 10.0), egui::Sense::hover());
            ui.painter().rect_filled(rect, 2.0, color);
            let share = if total > 0.0 { l.seconds / total * 100.0 } else { 0.0 };
            ui.label(format!("{} {} ({share:.0}%)", layer.name, format_duration(l.seconds)));
            ui.add_space(8.0);
        }
    });
}

/// Moves the playhead along while playing, and asks for repaints so it keeps moving.
fn advance(app: &mut LightFoundryApp, ui: &egui::Ui, total: f64) {
    if !app.player.playing {
        return;
    }
    let speed = SPEEDS.get(app.player.speed).copied().unwrap_or(0.0);
    let rate = if speed > 0.0 { speed } else { total / AUTO_PLAY_SECONDS };
    let dt = f64::from(ui.input(|i| i.stable_dt)).clamp(0.0, 0.25);
    let next = app.player.position.unwrap_or(0.0) + dt * rate;
    if next >= total || !next.is_finite() {
        app.player.position = Some(total);
        app.player.playing = false;
    } else {
        app.player.position = Some(next);
        ui.ctx().request_repaint();
    }
}

fn speed_label(speed: f64) -> String {
    if speed > 0.0 { format!("{speed}x") } else { "Auto".into() }
}

/// The head marker: a ring with a dot, in screen space.
pub fn draw_head(painter: &egui::Painter, at: egui::Pos2, color: Color32) {
    painter.circle_stroke(at, 6.0, Stroke::new(1.5, color));
    painter.circle_filled(at, 2.0, color);
    let arm = 10.0;
    for d in [vec2(arm, 0.0), vec2(0.0, arm)] {
        painter.line_segment([pos2(at.x - d.x, at.y - d.y), pos2(at.x + d.x, at.y + d.y)], Stroke::new(1.0, color));
    }
}
