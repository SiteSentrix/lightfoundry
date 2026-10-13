# Architecture

> **Last updated:** 2026-10-13 · **Stage:** M0, the vertical slice (import → layers → toolpath → G-code → stream)

How LightFoundry is built today. Rules (clean-room, never-crash, quality gates) are in
[`AGENTS.md`](../AGENTS.md).

## Shape

A Cargo workspace of 10 library crates under `crates/`, two apps under `apps/`, and `xtask`. Pure
Rust. The same engine runs the desktop app and the headless CLI.

## Layers

`cargo xtask layers` (part of `cargo xtask ci`) enforces that a crate depends only on lower layers
and that nothing below L6 depends on a UI toolkit. The table lives in `xtask/src/layers.rs`.

| Layer | Crates     | Role                                                                                             |
|-------|------------|--------------------------------------------------------------------------------------------------|
| L0    | `geom`     | polylines, flattening, bounds and hit-testing on `kurbo`                                         |
| L1    | `doc`      | the document: cut layers, shapes (paths and rasters), the machine profile; data + serde only     |
| L2    | `toolpath` | document to `Job`: ordering, scan fill, raster dithering, time estimate                          |
| L3    | `svg`      | SVG import (`usvg` resolves shapes, groups, transforms, units)                                   |
| L3    | `dxf`      | ASCII DXF import written from the public reference                                               |
| L3    | `gcode`    | `Job` to GRBL G-code; bed-bounds and finiteness checks; framing                                  |
| L3    | `format`   | the native `.lightfoundry` file: versioned JSON, atomic save, validation on load                 |
| L3    | `device`   | `Transport` trait, GRBL character-counting streamer, status parsing, background `JobRunner`, simulated controller, optional serial port (`serial` feature) |
| L5    | `engine`   | `Session` (document, selection, undo/redo), the command registry, the panic guard                |
| L6    | `ui-egui`  | menus, tools, canvas, panels; talks to the engine only through `execute`                         |
| L7    | `apps/lightfoundry`, `apps/lightfoundry-cli`, `xtask` | the eframe desktop app (native dialogs, serial), the CLI, build tooling |

## Everything is a command

Every user-visible action is a `CommandSpec` in `crates/engine/src/cmd/*`: id, label, menu path,
shortcut, a parameter description, whether it mutates, and `run`. Mutating commands are one undo
step; if they return an error or panic, the document is restored. `command.batch` runs several as
one atomic undo step. The UI, the CLI's `run` script and any future control channel or MCP server
reach the same registry, so anything a person can do, a script or agent can do.

`cargo run -p lightfoundry-cli -- commands` prints the live list.

## Coordinates and units

Everything is millimetres, **y down from the bed's top-left** in the document and in the UI. The
machine profile (`Origin::BottomLeft` or `TopLeft`) is applied only when G-code is written. Shapes
store local geometry plus an affine transform; world-space outlines are flattened on demand
(tolerance 0.02 mm).

## Planning and output

`toolpath::plan` walks output-enabled layers in id order. Per layer: scan fills first, then outlines
(inner shapes first, then nearest neighbour), then images (sampled on a world grid at the layer's
interval, dithered, emitted as scan rows with zero-power overscan). The result is a `Job` of
machine-independent `Move`s (`Travel`, `Cut { power % }`). `gcode::generate` maps power to
`S` (0–`max_s`), flips Y for a bottom-left origin, drops redundant words, repeats each op `passes`
times, and refuses out-of-bed or non-finite coordinates.

## Talking to a machine

`device::stream_program` implements GRBL's character-counting protocol: keep as many lines in
flight as fit in the 128-byte RX buffer and free space as `ok` responses arrive; `error:N` and
`ALARM:N` abort with the offending line; cancel sends a soft reset (0x18) so the laser stops.
`JobRunner` runs it on a thread; dropping a runner cancels the job. `SimulatedGrbl` models the RX
buffer so tests can prove the streamer never overruns it.

## Robustness

No panics in shipped code (workspace clippy lints). `engine::guard` catches a panic at every
command and rolls the document back. Importers and parsers cap input sizes and counts; the engine's
`command_sweep_never_panics` test fires hostile parameters at every registered command and checks
document invariants afterwards.

## UI

`ui-egui` is a thin front end: tools turn gestures into commands (`shape.*`, `edit.move`,
`command.batch` for corner-handle resize), panels read the session and call `layer.set`,
`machine.set`, `job.*`. Derived data (flattened outlines, the preview) is cached per
`Session::revision`. File dialogs and serial ports come in through the `Platform` trait, so the UI
crate has no OS dependencies beyond egui. Headless frame tests in `crates/ui-egui/src/tests.rs`
feed real pointer events through the real layout.
