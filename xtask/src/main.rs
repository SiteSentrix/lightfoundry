//! Workspace tooling: `cargo xtask <command>`. Pure Rust (std + serde_json).

mod assets;
mod cleanroom;
mod layers;
mod messages;
mod site_fonts;

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

const USAGE: &str = "\
usage: cargo xtask <command>

commands:
  layers     enforce the crate dependency layering
  cleanroom  fail on LightBurn-authored files, text or fixtures
  assets     check that every file under assets/ has a row in ATTRIBUTION.md
  messages   rewrite the list of engine messages for translators (--check: fail if out of date)
  site-fonts <fonts-repo> <site-dir>  copy the drawable library fonts into the hosted site with an index
  ci         fmt --check, clippy -D warnings, test, layers, cleanroom, assets (stops at the first failure)
";

fn main() -> ExitCode {
    let result = match std::env::args().nth(1).as_deref() {
        Some("layers") => layers::run(&root()),
        Some("cleanroom") => cleanroom::run(&root()),
        Some("assets") => assets::run(&root()),
        Some("messages") => messages::run(&root(), std::env::args().nth(2).as_deref() == Some("--check")),
        Some("site-fonts") => site_fonts::run(&std::env::args().skip(2).collect::<Vec<_>>()),
        Some("ci") => ci(),
        _ => Err(USAGE.to_string()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}

fn ci() -> Result<(), String> {
    let steps: [(&str, &[&str]); 3] = [
        ("fmt", &["fmt", "--all", "--check"]),
        ("clippy", &["clippy", "--workspace", "--all-targets", "--", "-D", "warnings"]),
        ("test", &["test", "--workspace"]),
    ];
    for (name, args) in steps {
        eprintln!("==> {name}");
        let status = Command::new("cargo").args(args).current_dir(root()).status().map_err(|e| format!("could not run cargo: {e}"))?;
        if !status.success() {
            return Err(format!("{name} failed"));
        }
    }
    eprintln!("==> layers");
    layers::run(&root())?;
    eprintln!("==> cleanroom");
    cleanroom::run(&root())?;
    eprintln!("==> assets");
    assets::run(&root())?;
    eprintln!("==> messages");
    messages::run(&root(), true)?;
    eprintln!("ci: all checks passed");
    Ok(())
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("."))
}
