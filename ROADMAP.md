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
- **2026-10-14 — path operations.** New `pathops` crate on `i_overlay`: boolean union / difference /
  intersect / xor with even-odd holes, and offset (grow, shrink, round/miter/bevel joins; open paths
  become an outline). `path.boolean` and `path.offset` commands, Arrange menu and Selection-panel
  controls. Found and fixed, by using the app, that ellipses and circles were never closed shapes
  (no fill, no booleans, no inner-first ordering); regression tests added at the geometry, planner
  and engine layers.
- **2026-10-14 — kerf.** Per-layer kerf offset (`layer.set kerf_offset`): closed outlines are offset
  before cutting so parts grow and holes shrink by the beam compensation; fills and open paths are
  untouched. `pathops` moved down to layer 1 so the planner can use it.
- **2026-10-14 — images.** `image.adjust` (brightness, contrast, gamma, negative; non-destructive,
  applied before dithering and never to the white around a rotated picture), real thumbnails on the
  canvas through the shape transform, an Image section in the inspector. Found by streaming a
  120k-pixel picture to the simulator: an image flush against the bed edge made the job unrunnable
  because of the 2 mm lead-in, so absolute placements now shorten the lead-in to the room available.
- **2026-10-14 — never lose work.** Autosave to a per-user recovery file (`file.autosave`), a
  startup offer to recover it (`file.recover`: reopens as untitled and unsaved), prompts before New,
  Open and closing discard changes (a save that does not complete keeps the question open), and a
  title with an unsaved marker. Found by killing the app with `kill -9`: egui only repaints on
  input, so an idle window never autosaved; a repaint is now scheduled while there are unsaved
  changes (with a regression test).
- **2026-10-14 — join.** `pathops::join` chains open polylines whose ends meet (grid-hashed, closes
  loops, bounded) and `path.join` applies it per layer. DXF import joins loose segments by default
  (`join: false` to keep them), so outlines drawn as separate LINEs become closed, fillable shapes.
- **2026-10-14 — arrays.** `arrange.grid` and `arrange.circular` (pitch = size + gap; arc or full
  circle; bounded to 10,000 copies, one undo step) and a corner radius on rectangles
  (`shape.rectangle radius`, Rectangle tool option); Array section in the Selection panel.
- **2026-10-14 — tabs and leads.** Per-layer tabs (`tab_count`, `tab_width`: laser-off gaps spread
  evenly on outer outlines only, never on holes, declined when they do not fit) and lead-in/out
  (`lead_in`, `lead_out`: approach from the scrap side — outside a part, inside a hole — starting
  mid-edge so the lead never runs along the cut itself; shortened to stay on the bed for absolute
  placement). Found by test: a lead at a corner ran along the adjacent edge.
- **2026-10-14 — overrides and test grids.** `Shape::cut` (`CutOverride`: speed, power, passes)
  replaces the layer's settings for one shape; the planner groups shapes by override and plans each
  group with the effective settings. `shape.cut_override` and `generate.test_grid` (speed across,
  power down, one square per cell with its own settings, labels on the next layer; bounded) with a
  Material test grid section in the Laser panel and an Own cut settings section in the Selection
  panel. Process note: a gate failure in the previous commit slipped through a shell pipeline; the
  gate script now stops on failure.
