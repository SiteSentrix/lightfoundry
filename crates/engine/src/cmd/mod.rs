//! The command registry. One `CommandSpec` per user-visible action; modules group them by menu.

mod edit;
mod file;
mod job;
mod params;
mod setup;
mod shapes;

use serde_json::Value;

use crate::{EngineError, Session};

pub(crate) use params::{id_list, parse};

/// A user-visible action.
pub struct CommandSpec {
    /// Stable id, e.g. `shape.rectangle`.
    pub id: &'static str,
    pub label: &'static str,
    /// Menu path, e.g. `Edit/Align`.
    pub menu: &'static str,
    pub shortcut: Option<&'static str>,
    /// Human/agent-readable parameter description (JSON object keys).
    pub params_doc: &'static str,
    /// Mutating commands are one undo step and roll back on failure.
    pub mutates: bool,
    pub run: fn(&mut Session, &Value) -> Result<Value, EngineError>,
}

pub fn commands() -> impl Iterator<Item = &'static CommandSpec> {
    shapes::COMMANDS.iter().chain(edit::COMMANDS).chain(setup::COMMANDS).chain(file::COMMANDS).chain(job::COMMANDS)
}

pub fn find_command(id: &str) -> Option<&'static CommandSpec> {
    commands().find(|c| c.id == id)
}
