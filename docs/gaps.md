# Gaps

Every known shortfall, ranked. This is the work list; pick from the top. Scores and evidence are in
[`target-app-parity.md`](target-app-parity.md).

| #  | Gap                                                                                  | Why it matters                                                      |
|----|--------------------------------------------------------------------------------------|---------------------------------------------------------------------|
| 1  | Verify on real GRBL hardware (jog, frame, short cut), fix what it finds              | Everything machine-side is only simulator-tested                    |
| 2  | Run-from-line; auto-reconnect (the $$ settings editor is done)                       | Day-to-day machine ownership                                        |
| 3  | Jog by dragging/clicking on the canvas; "move laser here" and show head position on canvas | Faster alignment                                              |
| 4  | Outline fonts for text (Fill-mode text, accented/Unicode characters) via an optional font source (`storytold/craft-fonts` through a build option, never in-repo); today text is an ASCII single-line stroke font | Fill-engraved lettering and non-English text |
| 7  | Numeric create dialogs; snapping to shape vertices and path midpoints (edges, centres, grid, bed and guides are done) | Precision layout                                                    |
| 8  | Cut-order: by group, choose the start point, remove overlapping segments (not only identical outlines) | Quality and time                                                    |
| 9  | Image crop, sharpen, halftone/newsprint dithers, trace to vectors (tone adjustments are done) | Photo engraving results                                  |
| 10 | Perforation/dashed cuts, ramp, per-layer overscan setting (tabs and lead-in/out are done) | Clean cuts of special materials                                     |
| 13 | Keep several autosave generations; periodic recovery-health check (recent files are done) | Never lose work                                                     |
| 14 | Material library (named presets; original entries only). Test grids are done        | Reusing settings for a known material                               |
| 15 | Other controllers (Marlin/Smoothieware dialects first; Ruida is a large project)     | Reach beyond GRBL                                                   |
| 16 | Camera overlay and print-and-cut alignment                                           | Placing work on printed or irregular material                       |
| 17 | MCP server and JSON control channel on top of `Session::execute`                     | Agent-driven design, like the sibling apps                          |
| 18 | WASM/web build                                                                       | Same engine in a browser (Web Serial for the machine link)          |
| 19 | Fuzz harnesses (`cargo-fuzz`) for SVG/DXF/format/G-code line paths                   | The tests are hostile-input unit tests, not a fuzzer                |
| 20 | Rotary, Z-axis, galvo                                                                | Hardware variants                                                   |
