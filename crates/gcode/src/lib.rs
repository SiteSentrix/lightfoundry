//! GRBL-flavoured G-code generation from a planned [`Job`].
//!
//! Workspace coordinates are y-down from the bed's top-left; the machine profile decides how they
//! map onto machine coordinates.

use std::fmt::Write;

use lightfoundry_doc::{Machine, Origin};
use lightfoundry_geom::{Point, Rect};
use lightfoundry_toolpath::{Job, Move};
use thiserror::Error;

/// Slack (mm) allowed outside the bed before a job is refused.
const BED_EPSILON: f64 = 1e-3;

#[derive(Debug, Error, PartialEq)]
pub enum GcodeError {
    #[error("point ({x:.2}, {y:.2}) mm is outside the {w:.0}×{h:.0} mm work area")]
    OutOfBed { x: f64, y: f64, w: f64, h: f64 },
    #[error("job contains a non-finite coordinate")]
    NonFinite,
    #[error("machine {0} must be a positive finite number")]
    BadMachine(&'static str),
}

/// Generates a complete program: preamble, every op × passes, shutdown.
pub fn generate(job: &Job, machine: &Machine) -> Result<String, GcodeError> {
    check_machine(machine)?;
    let mut w = Writer::new(machine);
    w.preamble("LightFoundry job");
    for op in &job.ops {
        if op.air_assist {
            w.line("M8");
        }
        for pass in 0..op.passes.max(1) {
            w.comment(&format!("layer {} pass {}", op.layer, pass + 1));
            for m in &op.moves {
                match m {
                    Move::Travel(p) => w.travel(*p)?,
                    Move::Cut { to, power } => w.cut(*to, *power, op.speed)?,
                }
            }
        }
        if op.air_assist {
            w.line("M9");
        }
    }
    w.shutdown();
    Ok(w.out)
}

/// A program that traces `bounds` at low power (`power` percent, 0 for a dry run), `loops` times.
pub fn frame(bounds: Rect, machine: &Machine, speed: f64, power: f64) -> Result<String, GcodeError> {
    check_machine(machine)?;
    if !(speed.is_finite() && speed > 0.0) {
        return Err(GcodeError::BadMachine("frame speed"));
    }
    let mut w = Writer::new(machine);
    w.preamble("LightFoundry frame");
    let corners = [(bounds.x0, bounds.y0), (bounds.x1, bounds.y0), (bounds.x1, bounds.y1), (bounds.x0, bounds.y1), (bounds.x0, bounds.y0)];
    for (i, (x, y)) in corners.iter().enumerate() {
        let p = Point::new(*x, *y);
        if i == 0 {
            w.travel(p)?;
        } else {
            w.cut(p, power, speed)?;
        }
    }
    w.shutdown();
    Ok(w.out)
}

fn check_machine(m: &Machine) -> Result<(), GcodeError> {
    for (name, v) in [("bed width", m.bed_width), ("bed height", m.bed_height)] {
        if !(v.is_finite() && v > 0.0) {
            return Err(GcodeError::BadMachine(name));
        }
    }
    if m.max_s == 0 {
        return Err(GcodeError::BadMachine("max S value"));
    }
    Ok(())
}

struct Writer<'a> {
    out: String,
    machine: &'a Machine,
    x: Option<f64>,
    y: Option<f64>,
    s: Option<u32>,
    f: Option<f64>,
}

impl<'a> Writer<'a> {
    fn new(machine: &'a Machine) -> Self {
        Writer { out: String::new(), machine, x: None, y: None, s: None, f: None }
    }

    fn preamble(&mut self, title: &str) {
        self.comment(title);
        self.line("G21");
        self.line("G90");
        self.line(if self.machine.dynamic_power { "M4 S0" } else { "M3 S0" });
        self.s = Some(0);
    }

    fn shutdown(&mut self) {
        self.line("M5");
        self.line("G0 X0 Y0");
    }

    fn travel(&mut self, p: Point) -> Result<(), GcodeError> {
        let (x, y) = self.to_machine(p)?;
        if self.x == Some(x) && self.y == Some(y) {
            return Ok(());
        }
        let mut s = String::from("G0");
        self.push_axes(&mut s, x, y);
        self.line(&s);
        Ok(())
    }

    fn cut(&mut self, p: Point, power: f64, speed: f64) -> Result<(), GcodeError> {
        let (x, y) = self.to_machine(p)?;
        let sv = ((power.clamp(0.0, 100.0) / 100.0) * self.machine.max_s as f64).round() as u32;
        if self.x == Some(x) && self.y == Some(y) && self.s == Some(sv) {
            return Ok(());
        }
        let mut s = String::from("G1");
        self.push_axes(&mut s, x, y);
        if self.s != Some(sv) {
            let _ = write!(s, " S{sv}");
            self.s = Some(sv);
        }
        if self.f != Some(speed) {
            let _ = write!(s, " F{}", num(speed));
            self.f = Some(speed);
        }
        self.line(&s);
        Ok(())
    }

    fn push_axes(&mut self, s: &mut String, x: f64, y: f64) {
        if self.x != Some(x) {
            let _ = write!(s, " X{}", num(x));
        }
        if self.y != Some(y) {
            let _ = write!(s, " Y{}", num(y));
        }
        self.x = Some(x);
        self.y = Some(y);
    }

    fn to_machine(&self, p: Point) -> Result<(f64, f64), GcodeError> {
        if !(p.x.is_finite() && p.y.is_finite()) {
            return Err(GcodeError::NonFinite);
        }
        let m = self.machine;
        if p.x < -BED_EPSILON || p.y < -BED_EPSILON || p.x > m.bed_width + BED_EPSILON || p.y > m.bed_height + BED_EPSILON {
            return Err(GcodeError::OutOfBed { x: p.x, y: p.y, w: m.bed_width, h: m.bed_height });
        }
        let y = match m.origin {
            Origin::TopLeft => p.y,
            Origin::BottomLeft => m.bed_height - p.y,
        };
        Ok((round3(p.x.clamp(0.0, m.bed_width)), round3(y.clamp(0.0, m.bed_height))))
    }

    fn line(&mut self, s: &str) {
        self.out.push_str(s);
        self.out.push('\n');
    }

    fn comment(&mut self, s: &str) {
        let clean: String = s.chars().filter(|c| !matches!(c, '(' | ')' | ';' | '\n' | '\r')).collect();
        self.out.push_str("; ");
        self.out.push_str(&clean);
        self.out.push('\n');
    }
}

fn round3(v: f64) -> f64 {
    (v * 1000.0).round() / 1000.0
}

/// Formats with up to 3 decimals, no trailing zeros, and never `-0`.
fn num(v: f64) -> String {
    let s = format!("{:.3}", round3(v));
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" || s.is_empty() { "0".into() } else { s.into() }
}

#[cfg(test)]
mod tests {
    use lightfoundry_doc::{Document, Shape};
    use lightfoundry_geom::rect_path;
    use lightfoundry_toolpath::plan;

    use super::*;

    fn rect_program(origin: Origin) -> String {
        let mut doc = Document::default();
        doc.machine.origin = origin;
        doc.machine.bed_height = 100.0;
        if let Some(l) = doc.layer_mut(0) {
            l.speed = 1200.0;
            l.max_power = 50.0;
        }
        doc.add_shape(Shape::path("r", 0, rect_path(10.0, 10.0, 20.0, 10.0)));
        generate(&plan(&doc).unwrap(), &doc.machine).unwrap()
    }

    mod program_tests {
        use super::*;

        #[test]
        fn rectangle_program_matches_expected_text() {
            let g = rect_program(Origin::TopLeft);
            let expected = "\
; LightFoundry job
G21
G90
M4 S0
; layer 0 pass 1
G0 X10 Y10
G1 X30 S500 F1200
G1 Y20
G1 X10
G1 Y10
M5
G0 X0 Y0
";
            assert_eq!(g, expected);
        }

        #[test]
        fn bottom_left_origin_flips_y() {
            let g = rect_program(Origin::BottomLeft);
            assert!(g.contains("G0 X10 Y90"), "{g}");
            assert!(g.contains("G1 Y80"), "{g}");
        }

        #[test]
        fn passes_repeat_and_air_assist_wraps() {
            let mut doc = Document::default();
            if let Some(l) = doc.layer_mut(0) {
                l.passes = 2;
                l.air_assist = true;
            }
            doc.add_shape(Shape::path("r", 0, rect_path(0.0, 0.0, 5.0, 5.0)));
            let g = generate(&plan(&doc).unwrap(), &doc.machine).unwrap();
            assert_eq!(g.matches("pass").count(), 2);
            assert!(g.contains("M8") && g.contains("M9"));
        }

        #[test]
        fn constant_power_mode_uses_m3() {
            let mut doc = Document::default();
            doc.machine.dynamic_power = false;
            assert!(generate(&Job::default(), &doc.machine).unwrap().contains("M3 S0"));
        }
    }

    mod safety_tests {
        use super::*;

        #[test]
        fn out_of_bed_is_refused() {
            let mut doc = Document::default();
            doc.add_shape(Shape::path("r", 0, rect_path(390.0, 0.0, 50.0, 5.0)));
            let err = generate(&plan(&doc).unwrap(), &doc.machine).unwrap_err();
            assert!(matches!(err, GcodeError::OutOfBed { .. }), "{err}");
        }

        #[test]
        fn non_finite_coordinates_are_refused() {
            let job = Job {
                ops: vec![lightfoundry_toolpath::Op {
                    layer: 0,
                    kind: lightfoundry_toolpath::OpKind::Line,
                    speed: 100.0,
                    passes: 1,
                    air_assist: false,
                    moves: vec![Move::Travel(Point::new(f64::NAN, 0.0))],
                }],
            };
            assert_eq!(generate(&job, &Machine::default()), Err(GcodeError::NonFinite));
        }

        #[test]
        fn broken_machine_profiles_are_refused() {
            let m = Machine { bed_width: 0.0, ..Machine::default() };
            assert!(generate(&Job::default(), &m).is_err());
            let m = Machine { max_s: 0, ..Machine::default() };
            assert!(generate(&Job::default(), &m).is_err());
        }

        #[test]
        fn layer_names_cannot_inject_gcode_through_comments() {
            let machine = Machine::default();
            let mut w = Writer::new(&machine);
            w.comment("evil\nM3 S1000");
            assert!(!w.out.contains("\nM3"));
        }

        #[test]
        fn zero_is_never_negative_zero() {
            assert_eq!(num(-0.0001), "0");
            assert_eq!(num(12.5), "12.5");
            assert_eq!(num(3.0), "3");
        }
    }

    mod frame_tests {
        use super::*;

        #[test]
        fn frame_traces_bounds_with_requested_power() {
            let g = frame(Rect::new(10.0, 10.0, 30.0, 20.0), &Machine { origin: Origin::TopLeft, ..Machine::default() }, 3000.0, 1.0).unwrap();
            assert!(g.contains("S10"), "{g}");
            assert_eq!(g.matches("G1").count(), 4);
        }
    }
}
