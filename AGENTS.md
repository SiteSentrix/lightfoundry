# LightFoundry — instructions for agents

LightFoundry is a clean-room, open-source, pure-Rust laser engraver/cutter application targeting
LightBurn-class workflow parity: lay out vector and raster artwork, assign cut settings per layer,
preview the toolpath, and send GRBL G-code to the machine. It runs natively (eframe/egui) and is
built so the same engine can drive a headless CLI and, later, the web.

The conventions follow the `storytold` crafting apps (VectorCraft, CADCraft). Their shared standards
repo (`craftrules`) was not accessible when this project was started, so the rules below are
transcribed from those two repos' `AGENTS.md`; reconcile with `craftrules` when it is available.

## Start every session here
1. Read [`ROADMAP.md`](ROADMAP.md) (stage, what is next) and the ranked work list in [`docs/gaps.md`](docs/gaps.md). Unless the user gives you a task, pick work from that list.
2. Read [`docs/architecture.md`](docs/architecture.md) for the layers and the command model.
3. Orient → plan → implement + test → verify → record → commit. One task per commit.

## Non-negotiables
- **Clean-room.** LightBurn is proprietary software; its licence forbids reverse engineering.
  - Never run, script, automate, screenshot or measure LightBurn (or any other vendor's laser software) and never compare our output with theirs.
  - Never read, disassemble or copy anything from an installation of it (binaries, resources, device profiles, material libraries, fonts, help files).
  - Never copy their code, text, icons, artwork, `.lbrn`/`.lbrn2` project files, `.lbdev` device files, `.clb` libraries or any sample files, and never commit files produced by them.
  - Behaviour comes from **public documentation and public specifications**, cited by URL in the code or docs: the GRBL documentation and wiki, the G-code/RS-274 conventions, the SVG and DXF references, and files users send us that they own (used locally, never committed). Write test inputs from scratch.
  - If a task seems to need any of this, stop and say so. `cargo xtask cleanroom` (part of `cargo xtask ci`) fails on those file types and on LightBurn copyright text.
  - Never copy GPL/AGPL/LGPL code (LaserGRBL, LightBurn-adjacent GPL tools, Inkscape/lib2geom, LibreCAD…); don't even read their sources. Permissively licensed crates from crates.io are fine.
- **Assets.** No third-party icons, images, fonts, presets, material libraries or screenshots. Icons and defaults are drawn in code. Anything file-based must be original or openly licensed and have a row in [`ATTRIBUTION.md`](ATTRIBUTION.md) (`cargo xtask assets` enforces it). Fonts, if ever needed, live in `Stitchwright/stitchfoundry-fonts`, never here.
- **Never crash: no panics in shipped code.** A crash loses the user's work and can leave a laser firing.
  - Fallible work returns `Result`/`Option`. Workspace clippy lints deny `unwrap`, `expect`, `panic!`, `todo!`, `unimplemented!`, `unreachable!` outside tests; tests may use them (`clippy.toml`). `unsafe_code` is denied.
  - Files, pasted data, command parameters and anything from a serial port are untrusted: use `.get()` instead of indexing with data, cap sizes and counts read from input, check floats are finite before using them as loop bounds or casting, never divide by something that can be zero.
  - `lightfoundry_engine::guard` is the last line of defence around every command (a panic restores the document); it does not replace `Result`.
  - Every new importer, parser or protocol path ships with hostile-input tests (see `dxf`, `svg`, `format`, `device`, and `command_sweep_never_panics` in the engine).
- **Laser safety is a correctness property.** G-code generation must refuse (not clamp silently) out-of-bed coordinates and non-finite numbers, comments must not be able to inject commands, cancelling a job must soft-reset the controller (laser off), and dropping a job runner cancels it. Never add a path that can leave the laser on.
- **Everything is a command.** User-visible behaviour is a `CommandSpec` in `crates/engine/src/cmd/*` (id, label, menu, params doc, `mutates`, `run`) plus tests. The UI, CLI (and later MCP/control channel) all go through `Session::execute`; mutating commands are one undo step and roll back on failure. Programmatic calls never open dialogs.
- **Layering** is enforced by `cargo xtask layers` (table in `xtask/src/layers.rs`; add new crates there). Nothing below the UI layer depends on egui/eframe/winit/rfd. **The UI is thin**: panels read engine state and act through `execute`; colours come from `theme::Tokens`.
- **Rust only** (no handwritten JS/TS).
- **Quality gate** before every commit: `cargo xtask ci` (fmt, clippy `-D warnings`, tests, layers, cleanroom, assets).

## Code style
- Edition 2024, `rustfmt.toml` as committed (`max_width = 150`); `log` macros, never `println!`/`eprintln!` in library code (the CLI and xtask print by design).
- Top-to-bottom reading order: constants, primary type, supporting types, impls (constructors, public methods, private helpers); callers above callees; in test modules, test cases first (grouped into sub-modules when there are 2+), helpers last.
- Optional fields use a `maybe_`-style name only where ambiguity would otherwise arise; prefer `Option<T>` fields with clear names.
- Tables in Markdown are space-padded so they read in plain text.

## Running and looking at the app
- `cargo run --release -p lightfoundry [file.svg|file.dxf|image.png|project.lightfoundry]` — desktop app. On a headless box: `xvfb-run` plus `LIBGL_ALWAYS_SOFTWARE=1`, and `libxkbcommon-x11` must be installed.
- `cargo run -p lightfoundry-cli -- commands` lists every command; `... gcode in.svg -o out.gcode --bed 300x200` converts headlessly; `... run script.json` runs a JSON command script.
- **For UI work, look at the result**: take a screenshot (Xvfb + `import -window root`) and drive it with `xdotool`, and cover interactions with the headless egui frame tests in `crates/ui-egui/src/tests.rs` (they feed real pointer events).
