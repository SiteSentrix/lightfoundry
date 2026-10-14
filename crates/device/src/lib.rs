//! Talking to GRBL controllers: a transport abstraction, a character-counting streamer,
//! status parsing, and a simulated controller for tests and the in-app simulator.
//!
//! No UI and no OS serial code lives here; real serial ports implement [`Transport`] elsewhere
//! (see the `serial` feature).

mod link;
mod sim;
mod status;
mod stream;

#[cfg(feature = "serial")]
pub mod serial;

pub use link::{CONSOLE_LIMIT, JobState, Link, LinkError, MAX_FEED, MAX_JOG_MM, Snapshot, jog_line};
pub use sim::SimulatedGrbl;
pub use status::{MachineState, Status, parse_status};
pub use stream::{Progress, StreamOptions, StreamReport, stream_program};

use std::io;
use std::time::Duration;

use thiserror::Error;

/// GRBL's serial RX buffer, in bytes.
pub const GRBL_RX_BUFFER: usize = 128;

/// Real-time commands (single bytes, processed immediately, never buffered).
pub mod realtime {
    pub const STATUS_QUERY: u8 = b'?';
    pub const FEED_HOLD: u8 = b'!';
    pub const CYCLE_START: u8 = b'~';
    pub const SOFT_RESET: u8 = 0x18;
}

pub trait Transport {
    fn write_all(&mut self, bytes: &[u8]) -> io::Result<()>;
    /// Returns the next full line (without terminator), or `None` if nothing arrived within `timeout`.
    fn read_line(&mut self, timeout: Duration) -> io::Result<Option<String>>;
}

#[derive(Debug, Error)]
pub enum DeviceError {
    #[error("I/O error talking to the machine: {0}")]
    Io(#[from] io::Error),
    #[error("the controller rejected line {line} ({text:?}) with error:{code}")]
    Grbl { code: u32, line: usize, text: String },
    #[error("the controller raised ALARM:{0}; clear it (unlock/home) before running again")]
    Alarm(u32),
    #[error("line {line} is {len} bytes; GRBL accepts at most {} per line", stream::MAX_LINE_BYTES)]
    LineTooLong { line: usize, len: usize },
    #[error("no response from the controller for {0} s")]
    Timeout(u64),
    #[error("job cancelled")]
    Cancelled,
}
