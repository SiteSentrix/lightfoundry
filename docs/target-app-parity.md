# Parity with the target application

> **Last updated:** 2026-10-13 · **Target:** LightBurn-class laser software (GRBL first)
> **Method:** self-assessed, deliberately low, all numbers *estimated* (nothing measured against the
> target; the clean-room rule forbids running it). Behaviour reference so far: general laser and GRBL
> conventions plus the GRBL project's public documentation. Before closing a gap, cite the public
> documentation or specification the behaviour comes from.

| Area                  | Score | Shipped                                                                 | Missing (highlights)                                                                                     |
|-----------------------|------:|-------------------------------------------------------------------------|----------------------------------------------------------------------------------------------------------|
| Drawing tools         |   35% | rectangle, ellipse, line, polygon, editable text (stroke font), boolean ops, offset; move, resize, rotate, flip, align | pen/Bézier editing, node edit, outline fonts (Fill-mode text), rounded rect, array/grid, groups, tabs/bridges, weld-by-selection of open paths |
| Import                |   40% | SVG, DXF (basic entities), raster images with thumbnails and tone adjustments (brightness, contrast, gamma, negative) | AI/PDF/EPS, DXF splines/ellipses/text/blocks, image trace, crop/resize-resample, sharpen, SVG text/images |
| Layers & cut settings |   40% | 16 layers; line, fill, fill+line, image; speed, power, passes, interval, kerf offset | sub-layers, lead-in/out, perforation, dot width, ramp, Z offset, per-layer overscan        |
| Toolpath planning     |   30% | inner-first, nearest neighbour, scan fill, dithered raster, estimate    | cut-order optimiser options, remove overlaps, start-point choice, cut selected only, job origin modes    |
| Output                |   35% | GRBL G-code (M3/M4, origin flip, bounds refusal), frame, job origin (absolute / user origin / current position, 9-point anchor) | GRBL 0.9 variants, Smoothieware/Marlin, Ruida/Trocen/TopWisdom, galvo, rotary |
| Machine control       |   45% | persistent link: status/position, jog, home, unlock, set/go to origin, pause/resume, stop, console, streaming with progress, simulator | machine settings ($$) editor, jog by dragging on canvas, camera, auto-reconnect, run-from-line |
| Preview               |   20% | static toolpath preview with travel toggle                              | timeline simulation, scrubbing, speed/power colouring, 3D, print-time breakdown per layer                |
| Files & workflow      |   40% | native JSON project, atomic save, undo/redo (200), autosave and crash recovery, unsaved-changes prompts, dirty title | recent files, templates, material library, print-and-cut, camera alignment, scripting |
| UI                    |   25% | menus, tool bar, canvas pan/zoom/grid, layer palette, inspector         | rulers, snapping, guides, layers list reorder, keyboard map, i18n, theming, accessibility                |
| Robustness            |   60% | no-panic lints, rollback on failure, hostile-input tests, safe streaming | fuzzing harnesses, data recovery, soak tests on real hardware                                            |

**Not yet verified on real hardware.** Everything on the machine side has been exercised against the
built-in simulator and unit tests only. Treat the first run on a real machine as a test: use Frame,
low power, and stay present.

## Evidence of what ships

- 214 tests across the workspace (`cargo test --workspace`); gates in `cargo xtask ci`.
- End-to-end: `lightfoundry-cli gcode demo.svg` → G-code; the desktop app was driven under Xvfb
  (draw with each tool, generate, start on the simulator, progress and completion shown; jog pad
  clicks moved the simulated head the expected way and the console logged each `$J=` line).
