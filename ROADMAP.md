# Roadmap

> **Stage:** M0, the vertical slice, complete · **Next:** M1, drive a real machine

| Milestone | Goal                                                                                       | Status |
|-----------|--------------------------------------------------------------------------------------------|--------|
| M0        | Import, layers, toolpath, G-code, simulated streaming, desktop app, CLI, quality gates     | done   |
| M1        | Real-hardware verification; jog/home/position; job origin; pause/resume (gaps 1–3)         | next   |
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
