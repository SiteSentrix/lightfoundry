//! Entry points for fuzzing every place untrusted bytes enter the program. Each function takes
//! arbitrary bytes and must return without panicking or hanging; errors are the expected outcome.
//!
//! Two drivers call them: the `cargo-fuzz` targets in `/fuzz` (coverage-guided, needs nightly), and
//! the deterministic mutation tests below, which run with `cargo test` on stable.

use lightfoundry_engine::{Session, commands};
use serde_json::Value;

/// Untrusted SVG text.
pub fn svg(data: &[u8]) {
    let _ = lightfoundry_svg::import_svg(data);
}

/// Untrusted DXF text.
pub fn dxf(data: &[u8]) {
    let _ = lightfoundry_dxf::import_dxf(data);
}

/// An untrusted `.lightfoundry` file.
pub fn project(data: &[u8]) {
    let _ = lightfoundry_format::from_bytes(data);
}

/// Lines a controller (or something pretending to be one) sends back.
pub fn controller_line(data: &[u8]) {
    let text = String::from_utf8_lossy(data);
    for line in text.lines() {
        let _ = lightfoundry_device::parse_status(line);
        let _ = lightfoundry_device::settings::parse_setting(line);
    }
}

/// A G-code program offered to the streamer.
pub fn gcode_program(data: &[u8]) {
    let text = String::from_utf8_lossy(data);
    let _ = lightfoundry_device::jog_to_line(f64::from(data.first().copied().unwrap_or(0)) * 1e3, 0.0, 1000.0);
    let mut sim = lightfoundry_device::SimulatedGrbl::new();
    let _ = lightfoundry_device::stream_program(
        &mut sim,
        &text,
        &lightfoundry_device::StreamOptions::default(),
        &std::sync::atomic::AtomicBool::new(false),
        |_| {},
    );
}

/// A message on the MCP channel.
pub fn mcp_message(data: &[u8]) {
    let mut session = Session::new();
    let text = String::from_utf8_lossy(data);
    for line in text.lines() {
        let _ = lightfoundry_mcp::handle_line(&mut session, line);
    }
}

/// Text to lay out in the stroke font.
pub fn text_layout(data: &[u8]) {
    let text = String::from_utf8_lossy(data);
    let _ = lightfoundry_text::layout(&text, &lightfoundry_text::TextStyle::default());
}

/// A command and its parameters: the first byte picks the command, the rest is JSON.
pub fn command(data: &[u8]) {
    let Some((first, rest)) = data.split_first() else { return };
    let specs: Vec<_> = commands().collect();
    let Some(spec) = specs.get(usize::from(*first) % specs.len().max(1)) else { return };
    // Commands that touch the file system are exercised by their own tests, not blindly.
    if spec.id.starts_with("file.") {
        return;
    }
    let params: Value = serde_json::from_slice(rest).unwrap_or(Value::Null);
    let mut session = Session::new();
    // Give the command something to work on.
    let _ = session.execute("shape.rectangle", &serde_json::json!({"x": 10, "y": 10, "width": 20, "height": 10}));
    let _ = session.execute(spec.id, &params);
    let _ = session.execute("job.gcode", &Value::Null);
    let _ = session.execute("edit.undo", &Value::Null);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A small deterministic generator, so a failing input can be reproduced from its seed.
    struct Rng(u64);

    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }

        fn below(&mut self, n: usize) -> usize {
            (self.next() % n.max(1) as u64) as usize
        }
    }

    const SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="50mm" height="30mm" viewBox="0 0 50 30"><g transform="translate(5 5)"><rect x="1" y="1" width="20" height="10" rx="2"/><circle cx="30" cy="10" r="6"/><path d="M0 0 C 5 20 15 20 20 0 Z"/></g></svg>"##;
    const DXF: &str = "0\nSECTION\n2\nENTITIES\n0\nLINE\n8\n0\n10\n0\n20\n0\n11\n10\n21\n10\n0\nCIRCLE\n8\n0\n10\n5\n20\n5\n40\n3\n0\nLWPOLYLINE\n8\n0\n90\n3\n70\n1\n10\n0\n20\n0\n10\n4\n20\n0\n10\n4\n20\n4\n0\nENDSEC\n0\nEOF\n";
    /// Blocks, inserts, an ellipse and a spline: the parts of the DXF reader with the most arithmetic.
    const DXF_RICH: &str = "0\nSECTION\n2\nBLOCKS\n0\nBLOCK\n2\nB\n10\n0\n20\n0\n0\nLINE\n10\n0\n20\n0\n11\n5\n21\n5\n0\nINSERT\n2\nB\n0\nENDBLK\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n0\nINSERT\n2\nB\n10\n3\n20\n4\n41\n2\n42\n2\n50\n30\n0\nELLIPSE\n10\n0\n20\n0\n11\n9\n21\n1\n40\n0.4\n41\n0\n42\n3\n0\nSPLINE\n70\n8\n71\n3\n40\n0\n40\n0\n40\n0\n40\n0\n40\n1\n40\n1\n40\n1\n40\n1\n41\n1\n41\n2\n41\n1\n41\n1\n10\n0\n20\n0\n10\n1\n20\n3\n10\n2\n20\n3\n10\n3\n20\n0\n0\nENDSEC\n0\nEOF\n";
    const GCODE: &str = "G21\nG90\nM4 S0\nG0 X10 Y10\nG1 X20 Y10 S500 F1000\nG1 X20 Y20\nM5\nG0 X0 Y0\n";
    const STATUS: &str = "<Idle|MPos:1.000,2.000,0.000|FS:0,0>\n$130=410.000\n$32=1\nok\nerror:20\nALARM:1\n";
    const MCP: &str = "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/call\",\"params\":{\"name\":\"shape_rectangle\",\"arguments\":{\"width\":5,\"height\":5}}}\n{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/list\"}\n";

    /// Random bytes, plus the seed with a few bytes flipped, dropped, duplicated or truncated.
    fn mutations(seed: &[u8], rng: &mut Rng, count: usize) -> Vec<Vec<u8>> {
        let mut out = vec![Vec::new(), seed.to_vec()];
        for _ in 0..count {
            let mut v = seed.to_vec();
            for _ in 0..=rng.below(6) {
                if v.is_empty() {
                    break;
                }
                let i = rng.below(v.len());
                match rng.below(5) {
                    0 => v[i] = rng.next() as u8,
                    1 => {
                        v.remove(i);
                    }
                    2 => {
                        let b = v[i];
                        v.insert(i, b);
                    }
                    3 => v.truncate(i),
                    _ => {
                        let n = rng.below(40);
                        let chunk: Vec<u8> = (0..n).map(|_| rng.next() as u8).collect();
                        v.splice(i..i, chunk);
                    }
                }
            }
            out.push(v);
        }
        for _ in 0..count / 4 {
            let n = rng.below(300);
            out.push((0..n).map(|_| rng.next() as u8).collect());
        }
        out
    }

    fn drive(seed: &str, f: fn(&[u8]), count: usize) {
        let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
        for input in mutations(seed.as_bytes(), &mut rng, count) {
            f(&input);
        }
    }

    #[test]
    fn svg_survives_mutation() {
        drive(SVG, svg, 1500);
    }

    #[test]
    fn dxf_survives_mutation() {
        drive(DXF, dxf, 3000);
        drive(DXF_RICH, dxf, 4000);
    }

    #[test]
    fn project_files_survive_mutation() {
        let mut s = Session::new();
        s.execute("shape.rectangle", &serde_json::json!({"width": 10, "height": 5})).unwrap();
        s.execute("preset.save", &serde_json::json!({"name": "p", "layer": 0})).unwrap();
        let bytes = lightfoundry_format::to_bytes(&s.doc).unwrap();
        let text = String::from_utf8(bytes).unwrap();
        drive(&text, project, 1500);
    }

    #[test]
    fn controller_lines_and_programs_survive_mutation() {
        drive(STATUS, controller_line, 3000);
        drive(GCODE, gcode_program, 1500);
    }

    #[test]
    fn mcp_messages_survive_mutation() {
        drive(MCP, mcp_message, 1500);
    }

    #[test]
    fn text_and_commands_survive_mutation() {
        drive("Hello, wörld!\nLine 2 \u{1F600} ~}{", text_layout, 1500);
        let mut rng = Rng(42);
        for _ in 0..3000 {
            let n = rng.below(120);
            let mut data: Vec<u8> = vec![rng.next() as u8];
            let junk = [
                "{\"id\":1}",
                "{\"width\":-5,\"height\":1e300}",
                "{\"x\":null}",
                "{\"ids\":[1,2,3],\"to\":\"left\"}",
                "[1,2]",
                "{\"text\":\"a\",\"height\":0}",
            ];
            data.extend_from_slice(junk[rng.below(junk.len())].as_bytes());
            data.extend((0..n % 8).map(|_| rng.next() as u8));
            command(&data);
        }
    }
}
