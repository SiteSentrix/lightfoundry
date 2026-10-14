# Gaps

Every known shortfall, ranked. This is the work list; pick from the top. Scores and evidence are in
[`target-app-parity.md`](target-app-parity.md).

| #  | Gap                                                                                  | Why it matters                                                      |
|----|--------------------------------------------------------------------------------------|---------------------------------------------------------------------|
| 1  | Verify on real GRBL hardware (jog, frame, short cut), fix what it finds              | Everything machine-side is only simulator-tested                    |
| 2  | Resume after a power loss, checked on a real machine: the saved position is offered at next start (run-from-line, auto-reconnect, the $$ editor and the saved job position are done); the head must be re-homed by the person, and a real-machine check is still owed | Day-to-day machine ownership                                        |
| 4  | Mixed left-to-right/right-to-left lines (bidi) (kerning, font fallback, ligatures, shaping, single-direction right-to-left runs and vertical stacking are done) | Typography beyond plain filled text |
| 8  | Per-shape cut order is done (priority within a layer, order by group, a chosen start point and overlapping-edge removal); remaining: ordering across layers | Quality and time |
| 10 | Ramp along fills and images (tabs, lead-in/out, perforation, outline ramp, overscan and speed-dependent overscan are done) | Clean cuts of special materials                                     |
| 14 | A real material library needs measured values per machine (generic starters, per-project presets, a shared preset library and test grids are done) | Reusing settings for a known material                               |
| 15 | Other controllers: verify the Marlin/Smoothieware dialects on hardware and add a link for them; Ruida is a large project | Reach beyond GRBL                                                   |
| 16 | Live camera capture and lens calibration (two-point print-and-cut alignment is done; a photo of the bed can serve as a backdrop on a non-output layer) | Placing work on printed or irregular material                       |
| 17 | MCP: a live link to the running desktop app (the stdio server edits its own session); resource subscriptions | Agent-driven design, like the sibling apps                          |
| 18 | Browser build: try Web Serial on real hardware; installing as a PWA needs a service worker (JavaScript, which the Rust-only rule forbids unless generated from Rust) and an icon drawn in code; remember recent files |  Web users can design and generate G-code today |
| 20 | Galvo/fibre/CO2-RF controllers (proprietary protocols); a tilted or 3D Z map; chuck rotary with steps-per-rotation |  Hardware variants |
| 21 | Translations: the CLI, durations and a few composed labels are still English; the browser build cannot load a catalog; no translation ships (see `translations.md`) | Users outside English-speaking markets |
