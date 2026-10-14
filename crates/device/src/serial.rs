//! Real serial-port transport (feature `serial`).

use std::io::{self, Read, Write};
use std::time::{Duration, Instant};

use crate::Transport;
use crate::linebuf::LineBuffer;

/// Typical GRBL laser baud rate.
pub const DEFAULT_BAUD: u32 = 115_200;

pub struct SerialTransport {
    port: Box<dyn serialport::SerialPort>,
    lines: LineBuffer,
}

impl SerialTransport {
    pub fn open(path: &str, baud: u32) -> io::Result<Self> {
        let port = serialport::new(path, baud).timeout(Duration::from_millis(10)).open().map_err(|e| io::Error::other(e.to_string()))?;
        Ok(Self { port, lines: LineBuffer::default() })
    }
}

/// Names of serial ports currently present.
pub fn list_ports() -> Vec<String> {
    serialport::available_ports().map(|ports| ports.into_iter().map(|p| p.port_name).collect()).unwrap_or_default()
}

impl Transport for SerialTransport {
    fn write_all(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.port.write_all(bytes)?;
        self.port.flush()
    }

    fn read_line(&mut self, timeout: Duration) -> io::Result<Option<String>> {
        let deadline = Instant::now() + timeout;
        let mut byte = [0u8; 1];
        loop {
            match self.port.read(&mut byte) {
                Ok(1) => {
                    if let Some(line) = self.lines.push(byte[0]) {
                        return Ok(Some(line));
                    }
                }
                Ok(_) => {}
                Err(e) if matches!(e.kind(), io::ErrorKind::TimedOut | io::ErrorKind::Interrupted | io::ErrorKind::WouldBlock) => {}
                Err(e) => return Err(e),
            }
            if Instant::now() >= deadline {
                return Ok(None);
            }
        }
    }
}
