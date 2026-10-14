use egui::{Align2, Color32, FontId, PointerButton, Pos2, Sense, Shape, Stroke, vec2};
use lightfoundry_doc::Origin;
use lightfoundry_geom::{Affine, Point, Rect};
use serde_json::json;

use crate::snap::{self, SnapSettings, SnapTargets};
use crate::theme::{on_dark, parse_hex};
use crate::view::View;
use crate::{LightFoundryApp, Tool};
use crate::{crop_ui, head_ui, nodes_ui, register_ui, timeline_ui};

/// Pointer-distance (screen points) within which handles and edges are hit.
pub(crate) const HIT_PX: f32 = 6.0;
/// Smallest shape a drag may create, in mm.
const MIN_CREATE_MM: f64 = 0.05;

#[derive(Debug, Clone, Copy)]
pub enum Drag {
    None,
    Create {
        start: Point,
    },
    Move {
        start: Point,
    },
    Rubber {
        start: Point,
    },
    /// Resizing the selection; `anchor` is the corner that stays put.
    Resize {
        anchor: Point,
    },
    /// Pulling a new guide out of a ruler.
    NewGuide {
        vertical: bool,
    },
    /// Dragging an existing guide (by index).
    MoveGuide {
        index: usize,
    },
}

/// A guide line: a vertical one sits at `pos` mm along x, a horizontal one at `pos` along y. Guides
/// are an editing aid (they attract snapping) and are not part of the document.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Guide {
    pub vertical: bool,
    pub pos: f64,
}

/// Thickness of the rulers, in screen points.
pub const RULER_PX: f32 = 18.0;

/// Everything snapping needs for one frame.
pub(crate) struct Snapper {
    settings: SnapSettings,
    /// Targets from everything: used when creating and resizing.
    all: SnapTargets,
    /// Targets from everything except the selection: used when moving it.
    others: SnapTargets,
    /// Snap distance in mm.
    threshold: f64,
}

impl Snapper {
    fn new(app: &LightFoundryApp, view: &View) -> Snapper {
        let mut all = SnapTargets::default();
        let mut others = SnapTargets::default();
        for t in [&mut all, &mut others] {
            t.add_rect(Rect::new(0.0, 0.0, app.session.doc.machine.bed_width, app.session.doc.machine.bed_height));
            for g in &app.guides {
                if g.vertical { t.xs.push(g.pos) } else { t.ys.push(g.pos) }
            }
        }
        let mut vertices = 0;
        for item in &app.geometry.items {
            all.add_rect(item.bounds);
            let moving = app.session.selection.contains(&item.id);
            if !moving {
                others.add_rect(item.bounds);
            }
            // Corners and edge midpoints of drawn paths (not text, which has too many to be useful).
            if let Some(shape) = app.session.doc.shape(item.id)
                && let lightfoundry_doc::ShapeKind::Path { path } = &shape.kind
                && vertices < snap::MAX_VERTEX_TARGETS
            {
                for p in snap::path_points(path, shape.transform, snap::MAX_VERTEX_TARGETS_PER_SHAPE) {
                    all.add_point(p);
                    if !moving {
                        others.add_point(p);
                    }
                    vertices += 1;
                }
            }
        }
        Snapper { settings: app.snapping, all, others, threshold: snap::SNAP_RADIUS_PX / f64::from(view.zoom) }
    }

    pub(crate) fn point(&self, p: Point) -> snap::Snapped {
        snap::snap_point(p, &self.settings, &self.all, self.threshold)
    }
}

/// The far end of a drag as it will be applied: constrained by Shift, otherwise snapped.
fn drag_end(app: &LightFoundryApp, snapper: &Snapper, start: Point, raw: Point, shift: bool) -> (Point, (Option<f64>, Option<f64>)) {
    if shift {
        let p = match app.tool {
            Tool::Line => snap::constrain_angle(start, raw, 15.0),
            Tool::Rectangle | Tool::Ellipse => snap::constrain_square(start, raw),
            _ => raw,
        };
        return (p, (None, None));
    }
    let s = snapper.point(raw);
    (s.point, (s.x_line, s.y_line))
}

pub fn show(app: &mut LightFoundryApp, ui: &mut egui::Ui) {
    let (resp, painter) = ui.allocate_painter(ui.available_size(), Sense::click_and_drag());
    let canvas_rect = resp.rect;
    app.canvas_rect = Some(canvas_rect);
    let painter = painter.with_clip_rect(canvas_rect);
    app.geometry.refresh(&app.session);
    let doc = &app.session.doc;
    app.thumbs.retain(|id| doc.shape(id).is_some());
    if app.show_preview {
        app.preview.refresh(&app.session);
    }
    let (bed_w, bed_h) = (app.session.doc.machine.bed_width, app.session.doc.machine.bed_height);
    let mut view = app.view.unwrap_or_else(|| View::fit(canvas_rect, bed_w, bed_h));

    navigate(ui, &resp, &mut view);
    let hover = resp.hover_pos().map(|p| view.to_world(p));
    app.hover_world = hover;
    let pointer = resp.interact_pointer_pos().map(|p| view.to_world(p));
    let snapper = Snapper::new(app, &view);
    let shift = ui.input(|i| i.modifiers.shift);
    app.snap_lines = (None, None);
    let offset = interact(app, ui, &resp, &view, pointer, &snapper, canvas_rect);

    painter.rect_filled(canvas_rect, 0.0, app.tokens.workspace);
    draw_bed(app, &painter, &view, bed_w, bed_h);
    draw_shapes(app, &painter, &view, canvas_rect, offset);
    if app.show_preview {
        draw_preview(app, &painter, &view);
    }
    head_ui::draw(app, &painter, &view);
    register_ui::draw(app, &painter, &view);
    crop_ui::draw(app, &painter, &view);
    draw_guides(app, &painter, &view, canvas_rect);
    nodes_ui::draw(app, &painter, &view);
    draw_overlays(app, &painter, &view, pointer, offset, &snapper, shift);
    if app.show_rulers {
        draw_rulers(app, &painter, &view, canvas_rect, hover);
    }

    if let Some(p) = hover {
        painter.text(
            canvas_rect.left_bottom() + vec2(8.0, -8.0),
            Align2::LEFT_BOTTOM,
            format!("X {:.2}  Y {:.2} mm   ·   {:.0}%", p.x, p.y, view.zoom / (96.0 / 25.4) * 100.0),
            FontId::monospace(11.0),
            Color32::from_gray(170),
        );
    }
    app.view = Some(view);
}

fn navigate(ui: &egui::Ui, resp: &egui::Response, view: &mut View) {
    if resp.dragged_by(PointerButton::Middle) || resp.dragged_by(PointerButton::Secondary) {
        view.pan(resp.drag_delta());
    }
    if let Some(anchor) = resp.hover_pos() {
        let (scroll, pinch) = ui.input(|i| (i.smooth_scroll_delta.y, i.zoom_delta()));
        if scroll != 0.0 {
            view.zoom_at(anchor, (scroll * 0.0025).exp());
        }
        if pinch != 1.0 {
            view.zoom_at(anchor, pinch);
        }
    }
}

/// Handles pointer interaction and returns the live move offset (mm) while dragging a selection.
fn interact(
    app: &mut LightFoundryApp,
    ui: &egui::Ui,
    resp: &egui::Response,
    view: &View,
    pointer: Option<Point>,
    snapper: &Snapper,
    canvas: egui::Rect,
) -> (f64, f64) {
    let shift = ui.input(|i| i.modifiers.shift);
    let tol = f64::from(HIT_PX / view.zoom);
    let mut offset = (0.0, 0.0);

    if app.crop_mode && crop_ui::interact(app, resp, view, pointer) {
        return offset;
    }
    if app.marks.pick.is_some() {
        if resp.clicked_by(PointerButton::Primary)
            && let Some(p) = pointer
        {
            register_ui::click(app, snapper.point(p).point);
        }
        return offset;
    }
    if app.move_head_mode {
        if resp.clicked_by(PointerButton::Primary)
            && let Some(p) = pointer
        {
            head_ui::click(app, p);
        }
        return offset;
    }

    // The node and pen tools have their own gestures, except for pulling guides out of rulers.
    if matches!(app.tool, Tool::Nodes | Tool::Pen) && !matches!(app.drag, Drag::NewGuide { .. } | Drag::MoveGuide { .. }) {
        let on_ruler =
            resp.drag_started_by(PointerButton::Primary) && ui.input(|i| i.pointer.press_origin()).is_some_and(|p| in_ruler(app, canvas, p));
        if !on_ruler {
            nodes_ui::interact(app, ui, resp, view, pointer, snapper);
            return offset;
        }
    }
    if resp.drag_started_by(PointerButton::Primary) {
        // egui reports the drag once the pointer has travelled past a threshold; the gesture began
        // where the button went down.
        let pressed = ui.input(|i| i.pointer.press_origin());
        let pressed_at = pressed.map(|p| view.to_world(p)).or(pointer);
        if let Some(p) = pressed_at {
            app.drag = begin_drag(app, p, pressed, canvas, tol, shift, snapper);
        }
    }
    if let (Drag::Move { start }, Some(p)) = (app.drag, pointer) {
        offset = move_offset(app, snapper, start, p, shift);
    }
    if let (Drag::MoveGuide { index }, Some(p)) = (app.drag, pointer)
        && let Some(g) = app.guides.get_mut(index)
    {
        g.pos = if g.vertical { p.x } else { p.y };
    }
    if resp.drag_stopped_by(PointerButton::Primary) {
        let released_on_ruler = ui.input(|i| i.pointer.interact_pos()).is_none_or(|p| in_ruler(app, canvas, p) || !canvas.contains(p));
        if let Some(p) = pointer {
            finish_drag(app, p, offset, shift, snapper, released_on_ruler);
        }
        app.drag = Drag::None;
        offset = (0.0, 0.0);
    }
    // The tool as it was when the click happened: placing text switches back to Select, and that
    // must not turn the same click into a selection click.
    let tool = app.tool;
    if resp.clicked_by(PointerButton::Primary)
        && tool == Tool::Text
        && let Some(p) = pointer
    {
        let p = snapper.point(p).point;
        let layer = app.active_layer;
        if let Some(v) = app.run("text.create", json!({ "text": "Text", "x": p.x, "y": p.y, "layer": layer })) {
            app.note_unsupported(&v);
        }
        app.tool = Tool::Select;
    }
    if resp.clicked_by(PointerButton::Primary)
        && tool == Tool::Select
        && let Some(p) = pointer
    {
        click_select(app, p, tol, shift);
    }
    offset
}

/// Whether a screen position is on a ruler strip.
pub(crate) fn in_ruler(app: &LightFoundryApp, canvas: egui::Rect, p: Pos2) -> bool {
    app.show_rulers && canvas.contains(p) && (p.x - canvas.min.x < RULER_PX || p.y - canvas.min.y < RULER_PX)
}

/// The selection's move offset: Shift locks to the dominant axis, otherwise alignment and grid
/// snapping adjust it (and remember which lines were hit, for drawing).
fn move_offset(app: &mut LightFoundryApp, snapper: &Snapper, start: Point, p: Point, shift: bool) -> (f64, f64) {
    let mut raw = (p.x - start.x, p.y - start.y);
    if shift {
        if raw.0.abs() >= raw.1.abs() {
            raw.1 = 0.0
        } else {
            raw.0 = 0.0
        }
        return raw;
    }
    let Some(b) = app.session.doc.bounds_of(&app.session.selection) else { return raw };
    let mut keys = SnapTargets::default();
    keys.add_rect(b);
    // `add_rect` lists edges before centres, so the grid lands the selection's top-left corner.
    let (delta, xl, yl) = snap::snap_translation(raw, &keys, &snapper.settings, &snapper.others, snapper.threshold);
    app.snap_lines = (xl, yl);
    delta
}

fn begin_drag(app: &mut LightFoundryApp, p: Point, screen: Option<Pos2>, canvas: egui::Rect, tol: f64, shift: bool, snapper: &Snapper) -> Drag {
    // Pulling a guide out of a ruler comes before any tool.
    if let Some(s) = screen
        && in_ruler(app, canvas, s)
    {
        let vertical = s.x - canvas.min.x < RULER_PX;
        return Drag::NewGuide { vertical };
    }
    if app.tool != Tool::Select {
        return Drag::Create { start: snapper.point(p).point };
    }
    if let Some(anchor) = handle_at(app, p, tol * 1.5) {
        return Drag::Resize { anchor };
    }
    match app.geometry.pick(p, tol) {
        Some(id) => {
            if !app.session.selection.contains(&id) {
                let ids = if shift { app.session.selection.iter().copied().chain([id]).collect() } else { vec![id] };
                app.run("edit.select", json!({ "ids": ids }));
            }
            Drag::Move { start: p }
        }
        None => {
            if let Some(index) = app.guides.iter().position(|g| (if g.vertical { p.x - g.pos } else { p.y - g.pos }).abs() <= tol) {
                return Drag::MoveGuide { index };
            }
            if !shift {
                app.run("edit.select", json!({ "ids": [] }));
            }
            Drag::Rubber { start: p }
        }
    }
}

fn finish_drag(app: &mut LightFoundryApp, end: Point, offset: (f64, f64), shift: bool, snapper: &Snapper, released_on_ruler: bool) {
    match app.drag {
        Drag::None => {}
        Drag::Create { start } => {
            let (end, _) = drag_end(app, snapper, start, end, shift);
            create(app, start, end);
        }
        Drag::Move { .. } => {
            if offset.0.hypot(offset.1) > 0.01 {
                app.run("edit.move", json!({ "dx": offset.0, "dy": offset.1 }));
            }
        }
        Drag::Rubber { start } => {
            let r = Rect::new(start.x.min(end.x), start.y.min(end.y), start.x.max(end.x), start.y.max(end.y));
            let ids: Vec<_> = app
                .geometry
                .items
                .iter()
                .filter(|it| r.contains(Point::new(it.bounds.x0, it.bounds.y0)) && r.contains(Point::new(it.bounds.x1, it.bounds.y1)))
                .map(|it| it.id)
                .collect();
            app.run("edit.select", json!({ "ids": ids }));
        }
        Drag::Resize { anchor } => {
            let end = snapper.point(end).point;
            let (w, h) = ((end.x - anchor.x).abs(), (end.y - anchor.y).abs());
            if w > MIN_CREATE_MM && h > MIN_CREATE_MM {
                let (x, y) = (anchor.x.min(end.x), anchor.y.min(end.y));
                app.run(
                    "command.batch",
                    json!({ "commands": [
                        { "id": "edit.resize", "params": { "width": w, "height": h } },
                        { "id": "edit.move_to", "params": { "x": x, "y": y } },
                    ]}),
                );
            }
        }
        Drag::NewGuide { vertical } => {
            if !released_on_ruler {
                app.guides.push(Guide { vertical, pos: if vertical { end.x } else { end.y } });
            }
        }
        Drag::MoveGuide { index } => {
            // Dropping a guide back on a ruler (or off the canvas) removes it.
            if released_on_ruler && index < app.guides.len() {
                app.guides.remove(index);
            }
        }
    }
}

fn create(app: &mut LightFoundryApp, a: Point, b: Point) {
    let (w, h) = ((b.x - a.x).abs(), (b.y - a.y).abs());
    let layer = app.active_layer;
    match app.tool {
        Tool::Select | Tool::Text | Tool::Nodes | Tool::Pen => {}
        Tool::Rectangle if w > MIN_CREATE_MM && h > MIN_CREATE_MM => {
            app.run(
                "shape.rectangle",
                json!({ "x": a.x.min(b.x), "y": a.y.min(b.y), "width": w, "height": h, "radius": app.corner_radius, "layer": layer }),
            );
        }
        Tool::Ellipse if w > MIN_CREATE_MM && h > MIN_CREATE_MM => {
            app.run("shape.ellipse", json!({ "cx": (a.x + b.x) / 2.0, "cy": (a.y + b.y) / 2.0, "rx": w / 2.0, "ry": h / 2.0, "layer": layer }));
        }
        Tool::Line if a.distance(b) > MIN_CREATE_MM => {
            app.run("shape.line", json!({ "x1": a.x, "y1": a.y, "x2": b.x, "y2": b.y, "layer": layer }));
        }
        Tool::Polygon if a.distance(b) > MIN_CREATE_MM => {
            app.run("shape.polygon", json!({ "cx": a.x, "cy": a.y, "radius": a.distance(b), "sides": app.polygon_sides, "layer": layer }));
        }
        _ => {}
    }
}

fn click_select(app: &mut LightFoundryApp, p: Point, tol: f64, shift: bool) {
    let hit = app.geometry.pick(p, tol);
    let mut ids = if shift { app.session.selection.clone() } else { Vec::new() };
    if let Some(id) = hit {
        if let Some(pos) = ids.iter().position(|i| *i == id).filter(|_| shift) {
            ids.remove(pos);
        } else {
            ids.push(id);
        }
    }
    app.run("edit.select", json!({ "ids": ids }));
}

/// The corner opposite to a selection-bounds corner under `p`, if any.
fn handle_at(app: &LightFoundryApp, p: Point, tol: f64) -> Option<Point> {
    let b = app.session.doc.bounds_of(&app.session.selection)?;
    let corners = [(b.x0, b.y0, b.x1, b.y1), (b.x1, b.y0, b.x0, b.y1), (b.x1, b.y1, b.x0, b.y0), (b.x0, b.y1, b.x1, b.y0)];
    corners.iter().find(|(cx, cy, _, _)| p.distance(Point::new(*cx, *cy)) <= tol).map(|(_, _, ax, ay)| Point::new(*ax, *ay))
}

fn draw_bed(app: &LightFoundryApp, painter: &egui::Painter, view: &View, w: f64, h: f64) {
    let t = &app.tokens;
    let bed = egui::Rect::from_two_pos(view.to_screen(Point::new(0.0, 0.0)), view.to_screen(Point::new(w, h)));
    painter.rect_filled(bed, 0.0, t.bed);
    let step = if view.zoom * 10.0 >= 6.0 { 10.0 } else { 50.0 };
    let mut x = 0.0;
    while x <= w {
        let major = (x / 50.0).fract().abs() < 1e-9;
        let (a, b) = (view.to_screen(Point::new(x, 0.0)), view.to_screen(Point::new(x, h)));
        painter.line_segment([a, b], Stroke::new(1.0, if major { t.grid_major } else { t.grid_minor }));
        x += step;
    }
    let mut y = 0.0;
    while y <= h {
        let major = (y / 50.0).fract().abs() < 1e-9;
        let (a, b) = (view.to_screen(Point::new(0.0, y)), view.to_screen(Point::new(w, y)));
        painter.line_segment([a, b], Stroke::new(1.0, if major { t.grid_major } else { t.grid_minor }));
        y += step;
    }
    painter.rect_stroke(bed, 0.0, Stroke::new(1.5, t.grid_major), egui::StrokeKind::Outside);
    // Machine origin marker, so it is obvious which corner is (0, 0) on the controller.
    let o = match app.session.doc.machine.origin {
        Origin::BottomLeft => view.to_screen(Point::new(0.0, h)),
        Origin::TopLeft => view.to_screen(Point::new(0.0, 0.0)),
    };
    let dy = if app.session.doc.machine.origin == Origin::BottomLeft { -1.0 } else { 1.0 };
    painter.line_segment([o, o + vec2(28.0, 0.0)], Stroke::new(2.0, t.origin));
    painter.line_segment([o, o + vec2(0.0, 28.0 * dy)], Stroke::new(2.0, t.origin));
    painter.circle_filled(o, 3.0, t.origin);
}

fn draw_shapes(app: &mut LightFoundryApp, painter: &egui::Painter, view: &View, clip: egui::Rect, offset: (f64, f64)) {
    let shift = Affine::translate(offset);
    let dim = app.show_preview;
    for item in &app.geometry.items {
        let selected = app.session.selection.contains(&item.id);
        let base = app.session.doc.layer(item.layer).map_or(Color32::WHITE, |l| on_dark(parse_hex(&l.color)));
        let color = if selected {
            app.tokens.selection
        } else if dim {
            base.gamma_multiply(0.35)
        } else {
            base
        };
        let m = if selected { shift } else { Affine::IDENTITY };
        for line in &item.lines {
            let pts: Vec<Pos2> = line.points.iter().map(|p| view.to_screen(m * *p)).collect();
            if pts.len() < 2 || !pts.iter().any(|p| clip.expand(50.0).contains(*p)) && !crosses(&pts, clip) {
                continue;
            }
            let stroke = Stroke::new(if selected { 2.0 } else { 1.5 }, color);
            painter.add(if line.closed { Shape::closed_line(pts, stroke) } else { Shape::line(pts, stroke) });
        }
        if item.is_image {
            draw_image(&mut app.thumbs, &app.session.doc, painter, view, item.id, m);
        }
    }
}

/// Draws an image shape as a textured quad through its transform (so rotation and flips work),
/// falling back to a labelled box if no texture can be made.
fn draw_image(
    thumbs: &mut crate::geometry::ImageTextures,
    doc: &lightfoundry_doc::Document,
    painter: &egui::Painter,
    view: &View,
    id: lightfoundry_doc::ShapeId,
    extra: Affine,
) {
    let Some(shape) = doc.shape(id).cloned() else { return };
    let lightfoundry_doc::ShapeKind::Image { width_mm, height_mm, .. } = &shape.kind else { return };
    let (w, h) = (*width_mm, *height_mm);
    let Some(texture) = thumbs.get(painter.ctx(), &shape) else { return };
    let corner = |x: f64, y: f64| view.to_screen(extra * shape.transform * Point::new(x, y));
    let corners = [
        (corner(0.0, 0.0), egui::pos2(0.0, 0.0)),
        (corner(w, 0.0), egui::pos2(1.0, 0.0)),
        (corner(w, h), egui::pos2(1.0, 1.0)),
        (corner(0.0, h), egui::pos2(0.0, 1.0)),
    ];
    let mut mesh = egui::Mesh::with_texture(texture);
    for (pos, uv) in corners {
        mesh.vertices.push(egui::epaint::Vertex { pos, uv, color: Color32::WHITE });
    }
    mesh.indices.extend_from_slice(&[0, 1, 2, 0, 2, 3]);
    painter.add(Shape::mesh(mesh));
}

/// Cheap bounding test so long shapes spanning the whole canvas are not culled.
fn crosses(pts: &[Pos2], clip: egui::Rect) -> bool {
    let bounds = egui::Rect::from_points(pts);
    bounds.intersects(clip)
}

fn draw_preview(app: &LightFoundryApp, painter: &egui::Painter, view: &View) {
    let now = app.player.position.and_then(|t| app.preview.timeline.at(t));
    for seg in &app.preview.segments {
        let travel = seg.power <= 0.0;
        if travel && !app.show_travel {
            continue;
        }
        let (stroke, end) = match timeline_ui::reveal(seg, now) {
            timeline_ui::Reveal::Hidden => {
                // The part of the job still to come stays visible, faintly.
                (Stroke::new(1.0, app.tokens.travel.gamma_multiply(0.25)), seg.b)
            }
            timeline_ui::Reveal::Full => (preview_stroke(app, seg), seg.b),
            timeline_ui::Reveal::Partial(f) => (preview_stroke(app, seg), seg.a.lerp(seg.b, f64::from(f))),
        };
        painter.line_segment([view.to_screen(seg.a), view.to_screen(end)], stroke);
    }
    if let Some(now) = now
        && let Some(head) = timeline_ui::head_position(&app.preview.segments, now)
    {
        timeline_ui::draw_head(painter, view.to_screen(head), app.tokens.selection);
    }
    if let Some(err) = &app.preview.error {
        painter.text(
            painter.clip_rect().left_top() + vec2(10.0, 10.0),
            Align2::LEFT_TOP,
            format!("Preview unavailable: {err}"),
            FontId::proportional(13.0),
            app.tokens.error,
        );
    } else if app.preview.truncated {
        painter.text(
            painter.clip_rect().left_top() + vec2(10.0, 10.0),
            Align2::LEFT_TOP,
            "Preview truncated (very large job)",
            FontId::proportional(13.0),
            app.tokens.travel,
        );
    }
}

fn preview_stroke(app: &LightFoundryApp, seg: &crate::geometry::PreviewSegment) -> Stroke {
    if seg.power <= 0.0 {
        return Stroke::new(1.0, app.tokens.travel);
    }
    let base = app.session.doc.layer(seg.layer).map_or(Color32::WHITE, |l| on_dark(parse_hex(&l.color)));
    Stroke::new(1.0, base.gamma_multiply(0.4 + 0.6 * (seg.power / 100.0).clamp(0.0, 1.0)))
}

fn draw_overlays(
    app: &LightFoundryApp,
    painter: &egui::Painter,
    view: &View,
    pointer: Option<Point>,
    offset: (f64, f64),
    snapper: &Snapper,
    shift: bool,
) {
    let t = &app.tokens;
    if let Some(b) = app.session.doc.bounds_of(&app.session.selection) {
        let moved = Rect::new(b.x0 + offset.0, b.y0 + offset.1, b.x1 + offset.0, b.y1 + offset.1);
        let r = egui::Rect::from_two_pos(view.to_screen(Point::new(moved.x0, moved.y0)), view.to_screen(Point::new(moved.x1, moved.y1)));
        let corners = [r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom(), r.left_top()];
        painter.extend(Shape::dashed_line(&corners, Stroke::new(1.0, t.selection), 5.0, 3.0));
        for c in &corners[..4] {
            let h = egui::Rect::from_center_size(*c, vec2(7.0, 7.0));
            painter.rect_filled(h, 1.0, t.handle);
            painter.rect_stroke(h, 1.0, Stroke::new(1.0, t.selection), egui::StrokeKind::Outside);
        }
    }
    let Some(p) = pointer else { return };
    let ghost = Stroke::new(1.0, t.selection);
    let mut lines = app.snap_lines;
    match app.drag {
        Drag::Create { start } => {
            let (end, hit) = drag_end(app, snapper, start, p, shift);
            lines = hit;
            draw_create_ghost(app, painter, view, start, end, ghost);
        }
        Drag::Rubber { start } => {
            let r = egui::Rect::from_two_pos(view.to_screen(start), view.to_screen(p));
            painter.rect_filled(r, 0.0, Color32::from_rgba_unmultiplied(90, 170, 255, 30));
            painter.rect_stroke(r, 0.0, ghost, egui::StrokeKind::Inside);
        }
        Drag::Resize { anchor } => {
            let s = snapper.point(p);
            lines = (s.x_line, s.y_line);
            let r = egui::Rect::from_two_pos(view.to_screen(anchor), view.to_screen(s.point));
            painter.rect_stroke(r, 0.0, ghost, egui::StrokeKind::Inside);
        }
        Drag::NewGuide { vertical } => {
            draw_guide_line(painter, view, painter.clip_rect(), Guide { vertical, pos: if vertical { p.x } else { p.y } }, t.guide)
        }
        Drag::None | Drag::Move { .. } | Drag::MoveGuide { .. } => {}
    }
    // Alignment lines the cursor or selection is locked onto.
    let clip = painter.clip_rect();
    let guide_stroke = Stroke::new(1.0, t.guide);
    if let Some(x) = lines.0 {
        let sx = view.to_screen(Point::new(x, 0.0)).x;
        painter.line_segment([Pos2::new(sx, clip.min.y), Pos2::new(sx, clip.max.y)], guide_stroke);
    }
    if let Some(y) = lines.1 {
        let sy = view.to_screen(Point::new(0.0, y)).y;
        painter.line_segment([Pos2::new(clip.min.x, sy), Pos2::new(clip.max.x, sy)], guide_stroke);
    }
}

fn draw_guides(app: &LightFoundryApp, painter: &egui::Painter, view: &View, clip: egui::Rect) {
    for g in &app.guides {
        draw_guide_line(painter, view, clip, *g, app.tokens.guide.gamma_multiply(0.7));
    }
}

fn draw_guide_line(painter: &egui::Painter, view: &View, clip: egui::Rect, g: Guide, color: Color32) {
    let stroke = Stroke::new(1.0, color);
    if g.vertical {
        let x = view.to_screen(Point::new(g.pos, 0.0)).x;
        painter.extend(Shape::dashed_line(&[Pos2::new(x, clip.min.y), Pos2::new(x, clip.max.y)], stroke, 6.0, 4.0));
    } else {
        let y = view.to_screen(Point::new(0.0, g.pos)).y;
        painter.extend(Shape::dashed_line(&[Pos2::new(clip.min.x, y), Pos2::new(clip.max.x, y)], stroke, 6.0, 4.0));
    }
}

/// Rulers along the top and left edges, in mm, with the cursor position marked.
fn draw_rulers(app: &LightFoundryApp, painter: &egui::Painter, view: &View, canvas: egui::Rect, hover: Option<Point>) {
    let t = &app.tokens;
    let (top, left) = (
        egui::Rect::from_min_size(canvas.min, vec2(canvas.width(), RULER_PX)),
        egui::Rect::from_min_size(canvas.min, vec2(RULER_PX, canvas.height())),
    );
    painter.rect_filled(top, 0.0, t.ruler);
    painter.rect_filled(left, 0.0, t.ruler);
    let (major, minor) = snap::ruler_steps(f64::from(view.zoom));
    let font = FontId::monospace(9.0);
    let stroke = Stroke::new(1.0, t.ruler_text);

    // World range visible along each axis.
    let (w0, w1) = (view.to_world(canvas.min), view.to_world(canvas.max));
    let ticks = |lo: f64, hi: f64, step: f64| -> Vec<f64> {
        let first = (lo / step).floor() as i64;
        let last = (hi / step).ceil() as i64;
        // Bounded: the step is chosen from the zoom, so this is a screenful of ticks.
        (first..=last).take(4000).map(|k| k as f64 * step).collect()
    };
    for x in ticks(w0.x, w1.x, minor) {
        let sx = view.to_screen(Point::new(x, 0.0)).x;
        if sx < canvas.min.x + RULER_PX || sx > canvas.max.x {
            continue;
        }
        let is_major = ((x / major).round() * major - x).abs() < minor * 1e-3;
        let len = if is_major { RULER_PX * 0.7 } else { RULER_PX * 0.35 };
        painter.line_segment([Pos2::new(sx, top.max.y - len), Pos2::new(sx, top.max.y)], stroke);
        if is_major {
            painter.text(Pos2::new(sx + 2.0, top.min.y + 1.0), Align2::LEFT_TOP, trim_mm(x), font.clone(), t.ruler_text);
        }
    }
    for y in ticks(w0.y, w1.y, minor) {
        let sy = view.to_screen(Point::new(0.0, y)).y;
        if sy < canvas.min.y + RULER_PX || sy > canvas.max.y {
            continue;
        }
        let is_major = ((y / major).round() * major - y).abs() < minor * 1e-3;
        let len = if is_major { RULER_PX * 0.7 } else { RULER_PX * 0.35 };
        painter.line_segment([Pos2::new(left.max.x - len, sy), Pos2::new(left.max.x, sy)], stroke);
        if is_major {
            painter.text(Pos2::new(left.min.x + 1.0, sy + 2.0), Align2::LEFT_TOP, trim_mm(y), font.clone(), t.ruler_text);
        }
    }
    if let Some(p) = hover {
        let (sx, sy) = (view.to_screen(p).x, view.to_screen(p).y);
        let mark = Stroke::new(1.0, t.origin);
        if sx >= canvas.min.x + RULER_PX && sx <= canvas.max.x {
            painter.line_segment([Pos2::new(sx, top.min.y), Pos2::new(sx, top.max.y)], mark);
        }
        if sy >= canvas.min.y + RULER_PX && sy <= canvas.max.y {
            painter.line_segment([Pos2::new(left.min.x, sy), Pos2::new(left.max.x, sy)], mark);
        }
    }
    painter.rect_filled(egui::Rect::from_min_size(canvas.min, vec2(RULER_PX, RULER_PX)), 0.0, t.ruler);
}

fn trim_mm(v: f64) -> String {
    let s = format!("{v:.3}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s.is_empty() || s == "-0" { "0".into() } else { s.into() }
}

fn draw_create_ghost(app: &LightFoundryApp, painter: &egui::Painter, view: &View, a: Point, b: Point, stroke: Stroke) {
    let (sa, sb) = (view.to_screen(a), view.to_screen(b));
    match app.tool {
        Tool::Rectangle => {
            painter.rect_stroke(egui::Rect::from_two_pos(sa, sb), 0.0, stroke, egui::StrokeKind::Inside);
        }
        Tool::Ellipse => {
            let r = egui::Rect::from_two_pos(sa, sb);
            let pts: Vec<Pos2> = (0..64)
                .map(|i| {
                    let t = std::f32::consts::TAU * i as f32 / 64.0;
                    r.center() + vec2(r.width() / 2.0 * t.cos(), r.height() / 2.0 * t.sin())
                })
                .collect();
            painter.add(Shape::closed_line(pts, stroke));
        }
        Tool::Line => {
            painter.line_segment([sa, sb], stroke);
        }
        Tool::Polygon => {
            let radius = sa.distance(sb);
            let n = app.polygon_sides.max(3);
            let pts: Vec<Pos2> = (0..n)
                .map(|i| {
                    let t = std::f32::consts::TAU * i as f32 / n as f32 - std::f32::consts::FRAC_PI_2;
                    sa + vec2(radius * t.cos(), radius * t.sin())
                })
                .collect();
            painter.add(Shape::closed_line(pts, stroke));
        }
        Tool::Select | Tool::Text | Tool::Nodes | Tool::Pen => {}
    }
}
