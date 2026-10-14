# Gaps

Every known shortfall, ranked. This is the work list; pick from the top. Scores and evidence are in
[`target-app-parity.md`](target-app-parity.md).

| #  | Gap                                                                                  | Why it matters                                                      |
|----|--------------------------------------------------------------------------------------|---------------------------------------------------------------------|
| 1  | Verify on real GRBL hardware (jog, frame, short cut), fix what it finds              | Everything machine-side is only simulator-tested                    |
| 2  | Machine settings ($$) editor; run-from-line; auto-reconnect                          | Day-to-day machine ownership                                        |
| 3  | Jog by dragging/clicking on the canvas; "move laser here" and show head position on canvas | Faster alignment                                              |
| 4  | Outline fonts for text (Fill-mode text, accented/Unicode characters) via an optional font source (`storytold/craft-fonts` through a build option, never in-repo); today text is an ASCII single-line stroke font | Fill-engraved lettering and non-English text |
| 5  | Node editing and a pen/Bézier tool                                                   | Cleaning up imports, drawing curves                                 |
| 6  | Offset/kerf, boolean ops (union, difference, intersect), weld                        | Fit and assembly work (boxes, joints)                               |
| 7  | Rulers, snapping, guides, numeric create dialogs                                     | Precision layout                                                    |
| 8  | Cut-order options (by layer/group, remove duplicate lines, choose start point)       | Quality and time                                                    |
| 9  | Image adjustments (contrast, gamma, negative, crop, trace to vectors)                | Photo engraving results                                             |
| 10 | Lead-in/out, tabs/bridges, perforation, ramp, per-layer overscan                     | Clean cuts, holding parts                                           |
| 11 | Timeline preview with scrubbing and per-layer time breakdown                         | Catch mistakes before burning                                       |
| 12 | Groups, layers list with reorder/rename, per-shape cut settings overrides            | Larger projects                                                     |
| 13 | Autosave and data recovery; recent files                                             | Never lose work                                                     |
| 14 | Material library and test grids (power/speed matrix generator)                       | Finding settings for a new material (original library only)         |
| 15 | Other controllers (Marlin/Smoothieware dialects first; Ruida is a large project)     | Reach beyond GRBL                                                   |
| 16 | Camera overlay and print-and-cut alignment                                           | Placing work on printed or irregular material                       |
| 17 | MCP server and JSON control channel on top of `Session::execute`                     | Agent-driven design, like the sibling apps                          |
| 18 | WASM/web build                                                                       | Same engine in a browser (Web Serial for the machine link)          |
| 19 | Fuzz harnesses (`cargo-fuzz`) for SVG/DXF/format/G-code line paths                   | The tests are hostile-input unit tests, not a fuzzer                |
| 20 | Rotary, Z-axis, galvo                                                                | Hardware variants                                                   |
