# Gaps

Every known shortfall, ranked. This is the work list; pick from the top. Scores and evidence are in
[`target-app-parity.md`](target-app-parity.md).

| #  | Gap                                                                                  | Why it matters                                                      |
|----|--------------------------------------------------------------------------------------|---------------------------------------------------------------------|
| 1  | Verify on real GRBL hardware (jog, frame, short cut), fix what it finds              | Everything machine-side is only simulator-tested                    |
| 2  | Resume after a power loss with a saved job position (run-from-line, auto-reconnect and the $$ editor are done) | Day-to-day machine ownership                                        |
| 4  | Text shaping (kerning, ligatures, complex scripts), text on a path, a bundled-free font picker listing installed fonts | Typography beyond plain filled text |
| 8  | Cut-order: by group, choose the start point, remove overlapping segments (not only identical outlines) | Quality and time                                                    |
| 9  | Smoother (curve-fitted) tracing and colour-aware tracing; on-canvas crop handles; resample/resize (tone, crop, sharpen, seven dither modes and basic threshold tracing are done) | Photo engraving results                                  |
| 10 | Ramp along fills and images; speed-dependent overscan (tabs, lead-in/out, perforation, outline ramp and overscan are done) | Clean cuts of special materials                                     |
| 13 | Keep several autosave generations; periodic recovery-health check (recent files are done) | Never lose work                                                     |
| 14 | Ship an original starter material library; share presets between projects (per-project presets and test grids are done) | Reusing settings for a known material                               |
| 15 | Other controllers: verify the Marlin/Smoothieware dialects on hardware and add a link for them; Ruida is a large project | Reach beyond GRBL                                                   |
| 16 | Live camera capture and lens calibration (two-point print-and-cut alignment is done; a photo of the bed can serve as a backdrop on a non-output layer) | Placing work on printed or irregular material                       |
| 17 | MCP: resources (documents as files), prompts, a live link to the running desktop app (the stdio server edits its own session) | Agent-driven design, like the sibling apps                          |
| 18 | Browser build: try Web Serial on real hardware, install as a PWA, remember recent files |  Web users can design and generate G-code today |
| 20 | Galvo/fibre/CO2-RF controllers (proprietary protocols); a tilted or 3D Z map; chuck rotary with steps-per-rotation |  Hardware variants |
