# Roadmap

> **Stage:** M1 software complete, hardware verification pending · **Next:** run on a real GRBL machine (gap 1)

| Milestone | Goal                                                                                       | Status |
|-----------|--------------------------------------------------------------------------------------------|--------|
| M0        | Import, layers, toolpath, G-code, simulated streaming, desktop app, CLI, quality gates     | done   |
| M1        | Jog/home/position, job origin, pause/resume (done, simulator-tested); real-hardware verification (gap 1) | in progress |
| M2        | Design tools: text, node edit, offset, booleans, snapping (gaps 4–7)                       |        |
| M3        | Planning quality: cut-order options, lead-ins, tabs, image adjustments, timeline preview   |        |
| M4        | Workflow: groups, autosave/recovery, material test grids, agent control (MCP)              |        |

See [`docs/gaps.md`](docs/gaps.md) for the ranked work list and
[`docs/target-app-parity.md`](docs/target-app-parity.md) for the self-assessment.

## Progress log

- **2026-10-13 — M0.** Workspace scaffolded on the VectorCraft/CADCraft conventions. Geometry,
  document model, toolpath planner (ordering, scan fill, dithered raster), GRBL G-code generator,
  SVG/DXF/image import, native file format, GRBL streamer with simulator and optional serial
  transport, engine with 31 commands, undo/redo and panic guard, egui desktop app, CLI, `xtask`
  (fmt, clippy, tests, layers, clean-room, assets). 118 tests. Not yet run on real hardware.
- **2026-10-14 — M1.** Persistent GRBL `Link` (status polling, jog, home, unlock, set/go to origin,
  pause/resume/stop, console) with a shared `JobStream` protocol core; simulator learned `$J=`, `$H`,
  `G92`, G90/G91 and feed hold. Job origin modes (`job.set_origin`: absolute / user origin / current
  position, 9-point anchor) through the document, G-code placement, file format, CLI and UI. UI jog pad
  and anchor picker drawn in code (the default font has no arrow glyphs). 143 tests. Still not run on
  real hardware.
- **2026-10-14 — text.** New `text` crate: an original single-line stroke font (ASCII, upper and
  lower case, digits, punctuation) drawn from a glyph table with corner-cutting smoothing, plus
  layout (multi-line, alignment, letter/line gap). `ShapeKind::Text` keeps content and style next to
  the generated path; `text.create` / `text.edit` commands; Text tool and inspector editor in the UI.
  Clicking anywhere inside a text block selects it. Fixed a drag-select bug found while testing (a
  click that switched tools was re-handled as a selection click).
