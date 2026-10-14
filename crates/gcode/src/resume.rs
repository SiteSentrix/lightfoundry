//! Starting a program part-way through. The machine's modal state (position, feed, power, spindle
//! mode, air assist) is rebuilt from the lines that are skipped, so the laser moves to the right
//! place with the beam off and carries on exactly as the full program would have.

use std::fmt::Write;

use thiserror::Error;

/// Longest program that can be resumed (state tracking is linear, but there is no need for more).
const MAX_LINES: usize = 50_000_000;

#[derive(Debug, Error, PartialEq)]
pub enum ResumeError {
    #[error("the program has {total} lines; line {line} does not exist")]
    OutOfRange { line: usize, total: usize },
    #[error("line {0} is not a motion or setup line this program format can be resumed from")]
    Unsupported(usize),
    #[error("the program is too long to resume")]
    TooLong,
}

#[derive(Default)]
struct State {
    x: Option<f64>,
    y: Option<f64>,
    s: Option<f64>,
    f: Option<f64>,
    z: Option<f64>,
    /// `M3` or `M4`, as the program set it.
    spindle: Option<&'static str>,
    air: bool,
    relative: bool,
}

/// Rebuilds a program that begins at 1-based `line` of `program`. Only programs written by this
/// application's generator (absolute `G0`/`G1` moves with `X`, `Y`, `S`, `F`) are supported.
pub fn resume_from(program: &str, line: usize) -> Result<String, ResumeError> {
    let lines: Vec<&str> = program.lines().collect();
    if lines.len() > MAX_LINES {
        return Err(ResumeError::TooLong);
    }
    if line == 0 || line > lines.len() {
        return Err(ResumeError::OutOfRange { line, total: lines.len() });
    }
    let mut st = State::default();
    for (i, raw) in lines.iter().enumerate().take(line - 1) {
        track(&mut st, raw).ok_or(ResumeError::Unsupported(i + 1))?;
    }
    if st.relative {
        return Err(ResumeError::Unsupported(line));
    }
    let mut out = String::new();
    let _ = writeln!(out, "; LightFoundry resume from line {line}");
    out.push_str("G21\nG90\n");
    // Power stays at zero until the first cutting move says otherwise.
    out.push_str(st.spindle.map_or("M4 S0", |m| if m == "M3" { "M3 S0" } else { "M4 S0" }));
    out.push('\n');
    if st.air {
        out.push_str("M8\n");
    }
    if let Some(z) = st.z {
        let _ = writeln!(out, "G0 Z{}", num(z));
    }
    if let (Some(x), Some(y)) = (st.x, st.y) {
        let _ = writeln!(out, "G0 X{} Y{}", num(x), num(y));
    }
    let mut fix_first_cut = true;
    for raw in lines.iter().skip(line - 1) {
        let code = code_of(raw);
        // The skipped part set zero (G92) already; doing it again would move the origin.
        if code.starts_with("G92") {
            continue;
        }
        if fix_first_cut && code.starts_with("G1") {
            fix_first_cut = false;
            let mut l = (*raw).to_string();
            if st.s.is_some() && !has_word(code, 'S') {
                let _ = write!(l, " S{}", num(st.s.unwrap_or(0.0)));
            }
            if st.f.is_some() && !has_word(code, 'F') {
                let _ = write!(l, " F{}", num(st.f.unwrap_or(0.0)));
            }
            out.push_str(&l);
        } else {
            out.push_str(raw);
        }
        out.push('\n');
    }
    Ok(out)
}

fn code_of(raw: &str) -> &str {
    raw.split(';').next().unwrap_or("").trim()
}

fn has_word(code: &str, letter: char) -> bool {
    code.split_whitespace().any(|w| w.starts_with(letter))
}

/// Applies one line to the tracked state. `None` means the line is something we cannot follow.
fn track(st: &mut State, raw: &str) -> Option<()> {
    let code = code_of(raw);
    if code.is_empty() {
        return Some(());
    }
    let mut words = code.split_whitespace();
    let first = words.next()?;
    match first {
        "G21" | "G92" | "M5" | "G0" | "G1" | "M3" | "M4" | "M8" | "M9" | "G90" | "G91" => {}
        _ => return None,
    }
    match first {
        "G90" => st.relative = false,
        "G91" => st.relative = true,
        "M3" => st.spindle = Some("M3"),
        "M4" => st.spindle = Some("M4"),
        "M8" => st.air = true,
        "M9" => st.air = false,
        "M5" => st.s = Some(0.0),
        _ => {}
    }
    if first == "G92" {
        return Some(()); // work zero is set once, before the part that is skipped
    }
    let motion = matches!(first, "G0" | "G1");
    for w in code.split_whitespace().skip(usize::from(!motion && !matches!(first, "M3" | "M4"))) {
        let (letter, value) = w.split_at(w.char_indices().nth(1).map_or(w.len(), |(i, _)| i));
        let v = value.parse::<f64>().ok().filter(|v| v.is_finite());
        match (letter, v) {
            ("X", Some(v)) if motion => st.x = Some(v),
            ("Y", Some(v)) if motion => st.y = Some(v),
            ("Z", Some(v)) if motion => st.z = Some(v),
            ("S", Some(v)) => st.s = Some(v),
            ("F", Some(v)) if motion => st.f = Some(v),
            ("X" | "Y" | "Z" | "F", _) if motion => return None,
            _ => {}
        }
    }
    Some(())
}

fn num(v: f64) -> String {
    let s = format!("{v:.3}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" || s.is_empty() { "0".into() } else { s.into() }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PROGRAM: &str = "; LightFoundry job\nG21\nG90\nM4 S0\nG92 X0 Y0\nM8\nG0 X10 Y10\nG1 X20 Y10 S500 F1200\nG1 Y20\nG0 X50 Y50\nG1 X60 S250\nG1 Y60\nM9\nM5\nG0 X0 Y0\n";

    #[test]
    fn resuming_rebuilds_position_feed_power_and_air() {
        // Line 11 is `G1 X60 S250`; the skipped lines left the head at (50, 50), air on, F1200.
        let r = resume_from(PROGRAM, 11).unwrap();
        assert!(r.starts_with("; LightFoundry resume from line 11\nG21\nG90\nM4 S0\nM8\nG0 X50 Y50\n"), "{r}");
        assert!(r.contains("G1 X60 S250 F1200\n"), "feed is restored on the first cut: {r}");
        assert!(r.ends_with("M9\nM5\nG0 X0 Y0\n"));
        assert!(!r.contains("G92"), "the origin is not set a second time");
    }

    #[test]
    fn the_z_height_is_restored_before_the_head_moves() {
        let p = "G21\nG90\nM4 S0\nG0 Z-1.5\nG0 X5 Y5\nG1 X9 S100 F500\nG1 Y9\n";
        let r = resume_from(p, 7).unwrap();
        assert!(r.contains("G0 Z-1.5\nG0 X9 Y5\n"), "{r}");
    }

    #[test]
    fn a_cut_without_its_own_power_inherits_the_last_one() {
        let r = resume_from(PROGRAM, 9).unwrap(); // `G1 Y20` follows `G1 X20 Y10 S500 F1200`
        assert!(r.contains("G0 X20 Y10\n"), "{r}");
        assert!(r.contains("G1 Y20 S500 F1200\n"), "{r}");
    }

    #[test]
    fn resuming_from_the_first_line_is_the_whole_program_minus_the_origin_command() {
        let r = resume_from(PROGRAM, 1).unwrap();
        assert!(r.contains("G0 X10 Y10") && r.contains("M8") && !r.contains("G92"));
        assert!(!r.contains("G0 X50 Y50\nM4"), "{r}");
    }

    #[test]
    fn constant_power_programs_keep_their_spindle_mode_and_start_with_the_beam_off() {
        let p = PROGRAM.replace("M4 S0", "M3 S0");
        let r = resume_from(&p, 11).unwrap();
        assert!(r.contains("\nM3 S0\n") && !r.contains("M4"), "{r}");
    }

    #[test]
    fn bad_lines_and_unsupported_programs_are_refused() {
        assert_eq!(resume_from(PROGRAM, 0), Err(ResumeError::OutOfRange { line: 0, total: 15 }));
        assert!(matches!(resume_from(PROGRAM, 99), Err(ResumeError::OutOfRange { .. })));
        assert_eq!(resume_from("G21\nG2 X1 Y1 I1 J0\nG1 X2\n", 3), Err(ResumeError::Unsupported(2)));
        assert!(matches!(resume_from("G91\nG1 X1\n", 2), Err(ResumeError::Unsupported(_))));
        assert!(resume_from("G1 Xabc\nG1 X1\n", 2).is_err());
        assert!(resume_from("", 1).is_err());
        assert!(resume_from("G1 X1 Y1 S5 F10", 1).is_ok());
    }
}
