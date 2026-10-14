# LightFoundry

**An open-source, clean-room laser engraver/cutter application in pure Rust, aiming at a
LightBurn-class workflow.** Lay out artwork, give each colour layer its own speed, power, passes and
fill settings, preview the toolpath, and stream GRBL G-code to your machine, or to the built-in
simulator when nothing is plugged in.

> **Status: early.** The core pipeline works end to end and is well tested, but this is a small
> fraction of what LightBurn does. See [`ROADMAP.md`](ROADMAP.md) and
> [`docs/target-app-parity.md`](docs/target-app-parity.md) for an honest list of what exists and what
> doesn't. LightFoundry is independent of and unaffiliated with LightBurn Software.

## What works today

- **Create:** rectangle, ellipse, line, regular-polygon and text tools (editable multi-line text in an original single-line stroke font: size, letter/line gap, alignment; click anywhere in the block to select); drag, rubber-band select, corner-handle resize, nudge, align, flip, rotate, duplicate, undo/redo (200 steps).
- **Import:** SVG (shapes, groups, transforms, units; stroke/fill colours map to cut layers), DXF (LINE, LWPOLYLINE with bulges, POLYLINE, CIRCLE, ARC; `$INSUNITS` honoured; DXF layers become cut layers), and raster images (PNG/JPEG/BMP/GIF/WebP).
- **Cut layers:** 16 layers, each with Line / Fill / Fill + Line mode, speed, min/max power, passes, fill interval and angle, bi-directional scanning, air assist, and image dithering (threshold, Floyd–Steinberg, grayscale).
- **Toolpath:** inner shapes cut before outer ones, nearest-neighbour ordering, serpentine scan fill with holes, raster engraving with overscan, and a toolpath preview with time estimate.
- **Output:** GRBL G-code (`M4` dynamic power or `M3`, configurable `$30` S range, bottom-left or top-left origin, bed-bounds refusal), frame-the-job, save G-code.
- **Job origin:** Absolute coordinates, User origin, or Current position, with a 9-point anchor on the job bounds, so work can be placed on material wherever the head is (`G92` for current position).
- **Machine control:** a persistent GRBL connection with live status and position, jog pad (direction follows the machine's origin corner), home, unlock, set origin here, go to origin, pause/resume (feed hold), stop (soft reset, laser off), a console with a command line, and character-counting job streaming with progress. Serial ports are behind the `serial` feature (on by default in the desktop app); a built-in simulated controller needs no hardware and checks the 128-byte RX buffer is never overrun.
- **Files:** native `.lightfoundry` (versioned JSON, atomic saves, validated on load).
- **Headless:** `lightfoundry-cli` converts files to G-code and runs JSON command scripts.

## Quick start

```sh
git clone https://github.com/SiteSentrix/lightfoundry
cd lightfoundry

cargo run --release -p lightfoundry                         # desktop app
cargo run --release -p lightfoundry -- design.svg           # open an SVG
cargo run -p lightfoundry-cli -- gcode design.svg -o job.gcode --bed 300x200 --origin top_left \
    --set 2:speed=900,max_power=40                          # headless conversion
cargo xtask ci                                              # fmt, clippy, tests, layers, clean-room
```

Linux needs the usual windowing libraries for eframe (X11 or Wayland, `libxkbcommon`, a GL driver).
Serial ports use the `serialport` crate without `libudev`; you may need to be in the `dialout` group.

> **Not yet verified on real hardware.** Everything machine-side has run against the built-in
> simulator only. Use Frame and low power first, and stay present.

> **Safety:** a laser is a fire and eye hazard. Test with the simulator and the Frame button first,
> never leave a running machine unattended, and wear appropriate eye protection. This software is
> provided without warranty (see the licences).

## Design

A layered Rust workspace, "everything is a command", no panics in shipped code, and a thin UI.
See [`docs/architecture.md`](docs/architecture.md). Contributor and agent rules are in
[`AGENTS.md`](AGENTS.md).

## Licence

MIT OR Apache-2.0 (`LICENSE-MIT`, `LICENSE-APACHE`, `NOTICE`).
