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
- **2026-10-14 — groups and layers.** Shapes carry an optional group id: selecting a member selects
  the group, copies and array copies form groups of their own (never sharing the original's),
  `edit.group` / `edit.ungroup` (Ctrl+G, Ctrl+Shift+G, merging existing groups). Layers can be
  renamed, recoloured (#rrggbb) and reordered (`layer.move`; list order is cut order).
- **2026-10-14 — snapping and rulers.** Pure snap maths (`snap.rs`: axis snapping to edges, centres,
  bed and guides with grid fallback; translation snapping that locks the closest selection edge;
  Shift constraints for squares/circles and 15-degree lines; 1-2-5 ruler steps) wired into creation,
  resize, move and text placement, with alignment lines while dragging. Rulers on the canvas edges
  with a cursor marker, guides pulled out of the rulers (drag back to delete), View menu options.
  Egui 0.36 delivers modifier keys as events, which the test harness now emits.
- **2026-10-14 — node editing and pen tool.** `geom::nodes` is an editable node model (corner/smooth
  nodes with handles, shape-preserving insert, delete, line/curve segments, open/close) with
  commands `node.move`, `node.move_handle`, `node.insert`, `node.delete`, `node.set_smooth`,
  `node.set_segment`, `node.set_closed`, `path.create` and `edit.convert_to_path` (text to path).
  Nodes tool (drag nodes and handles, double-click a segment to insert, Delete) and Pen tool
  (click corners, drag out smooth nodes, click the first node to close, Enter/double-click/right
  click to finish, Escape to cancel), both snapping. 319 tests.
- **2026-10-14 — timeline preview.** `toolpath::Timeline` (per-operation start, per-pass time,
  per-layer totals, `at(t)` to find operation/pass/offset; `estimate` is now derived from it and
  carries the head position across operations). `job.estimate` also returns per-layer times. The
  preview has a bottom bar with Play/Pause, Show all, a scrubber, playback speed (auto = 30 s per
  job) and a per-layer time breakdown; done moves are drawn bright, the rest faintly, with a head
  marker. Tests: timeline maths, reveal/head logic, playback to the end.
- **2026-10-14 — recent files.** File > Open Recent (ten entries, most recent first, kept in
  `recent.json` in the app data folder through `Platform::recent_file`); any change of the
  document's path (open, save as) records it, picking one asks about unsaved changes first, and an
  unreadable entry is dropped from the list.
- **2026-10-14 — controller settings.** `device::settings` (parser for `$n=value` reports, validated
  `$n=v` writer, table of GRBL 1.1 settings with source URL); the link remembers what the controller
  reports (bounded) and offers `request_settings` / `write_setting` (refused while a job runs);
  the simulator answers `$$` and stores writes. The Laser panel has a Controller settings editor
  (flags as checkboxes, numbers with units) and warns when laser mode ($32) is off or $30 / $130-131
  differ from the document, with one-click fixes.
- **2026-10-14 — cut-order options.** `Document::optimize` (inner shapes first, reduce travel,
  remove duplicate lines; older files load with all three on) and `job.set_optimize`. Duplicate
  removal compares outlines after rounding to 0.1 µm, either direction and any start vertex, and
  applies to fills too (two identical shapes would otherwise cancel under even-odd). Laser panel
  gets a Cut order section.
- **2026-10-14 — image crop, sharpen and more dithers.** `image.crop` (fractions per side; the
  remaining picture stays where it was) and `image.sharpen` (3x3 unsharp mask) edit the stored
  pixels and are undoable; Atkinson, Jarvis, ordered (8x8 Bayer) and halftone (45 degree dot
  screen mapped through the spot function's quantiles so tone is preserved) join threshold,
  Floyd-Steinberg and grayscale. Image inspector gets a Crop and sharpen section.
- **2026-10-14 — G-code dialects.** `Machine::dialect` (GRBL, Marlin laser with `M3 I` inline
  power, Smoothieware with 0-1 `S` on each G1 and no spindle commands), chosen in the Device
  section and `machine.set`; written from the public firmware docs, flagged as untested on
  hardware; out-of-bed refusal applies to all. The serial link features stay GRBL-only.


- **2026-10-14 — presets.** `Document::presets` (named snapshots of a layer's cut settings, capped at
  200, saved with the project) and `preset.save` / `preset.apply` / `preset.delete`; applying keeps
  the layer's name, colour and output switch. Presets section in the layer panel.
- **2026-10-14 — head on the canvas.** With absolute start the connected machine's position is drawn
  on the bed (a ring and cross), and "Click the canvas to move the laser there" sends a bounded
  absolute `$J=G90 G21` jog (`Link::jog_to`); points off the bed are refused, Escape or
  disconnecting leaves the mode, and nothing is sent while a job runs.
- **2026-10-14 — precision layout.** Snapping now also locks onto path corners, segment ends and
  the midpoints of straight segments (capped at 200 per shape, 5000 overall; text excluded), and
  the Selection panel offers "Create by numbers" (x, y, width, height) for rectangles and ellipses
  on the active layer.
- **2026-10-14 — perforation.** Layers have `perf_cut` / `perf_gap` (mm; both > 0): outlines, open
  or closed, are cut as dashes with rapid moves over the gaps (laser off), replacing tabs when
  both are set. Lengths below 0.05 mm, non-finite values and absurd piece counts fall back to a
  solid line (or a plan error for bad settings).
- **2026-10-14 — MCP server.** New crate `lightfoundry-mcp` (layer 6; the UI layer moved to 7): a
  newline-delimited JSON-RPC 2.0 server implementing initialize, ping, tools/list and tools/call,
  where every engine command is a tool (`shape.rectangle` becomes `shape_rectangle`) plus a
  read-only `document_summary`. Engine failures are tool errors; messages over 64 MB are skipped
  without being buffered. `lightfoundry-cli mcp` serves it on stdin/stdout. Note for operators: the
  file.* tools read and write paths the agent names, so only attach it to agents you trust.
- **2026-10-14 — trace to vectors.** `toolpath::trace` follows the boundaries of dark pixels along
  pixel edges (outer loops clockwise, holes the other way, diagonal neighbours joined), then
  simplifies with Douglas-Peucker (bounded: 16 Mpixel, 4 M edges, 50 000 loops). `image.trace`
  applies the picture's tone adjustments, scales to the picture's size and position, and adds the
  outlines as a path (optionally replacing the picture). Image inspector gets a Trace section.
- **2026-10-14 — cut selected shapes only.** `Session::job_doc()` (the document, or just the
  selection) feeds generate, estimate, frame and the preview, so job origins anchor to the selection
  too; `job.set_scope` and a Laser-panel checkbox switch it (a session setting, not saved).
- **2026-10-14 — distribute and star.** `edit.distribute` (even gaps between three or more shapes
  along an axis, or a fixed gap from the first) with Arrange menu entries, and `shape.star`
  (3-100 points, inner and outer radius).
- **2026-10-14 — cross-hatch fill and a Star button.** `crosshatch` on a layer plans the fill a
  second time at right angles (older files load with it off); the Create by numbers section gets a
  Star button.
- **2026-10-14 — fuzz harnesses, and two bugs they found.** `crates/fuzzkit` has one entry point
  per untrusted-input path (SVG, DXF, project files, controller lines, G-code programs, MCP
  messages, text, commands with junk parameters), driven by deterministic mutation tests on stable
  and by `cargo-fuzz` targets in `/fuzz` (nightly; not run here). Found and fixed: the simulated
  controller panicked on a non-ASCII G-code word, and a program containing GRBL's real-time bytes
  (0x18 soft reset, `?`, `!`, `~`) would have been streamed as is; the streamer and the console
  now refuse such lines, comments included, before anything is sent.
- **2026-10-14 — auto-reconnect and run-from-line.** A lost serial connection is retried with
  doubling waits (up to 8 tries, 30 s apart at most); Disconnect cancels it and nothing is ever
  resumed by itself. `gcode::resume_from` rebuilds position, feed, power, spindle mode and air from
  the skipped lines and never repeats `G92`; `job.resume` and a "Run from line" control in the
  G-code section start a job part-way through, with the beam off for the approach move.

