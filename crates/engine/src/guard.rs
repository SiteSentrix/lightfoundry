//! Last line of defence: a panic inside a command must never take the app (and unsaved work) down.

use std::panic::{AssertUnwindSafe, catch_unwind};

use crate::EngineError;

/// Runs `f`, converting a panic into [`EngineError::Internal`]. The caller restores its snapshot.
pub fn guarded<T>(f: impl FnOnce() -> Result<T, EngineError>) -> Result<T, EngineError> {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(result) => result,
        Err(payload) => {
            let msg = payload
                .downcast_ref::<&str>()
                .map(|s| (*s).to_string())
                .or_else(|| payload.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "panic".into());
            log::error!("command panicked: {msg}");
            Err(EngineError::Internal(msg))
        }
    }
}
