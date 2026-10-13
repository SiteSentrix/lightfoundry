//! Real serial-port transport (feature `serial`).

use std::io::{self, Read, Write};
use std::time::{Duration, Instant};

use crate::Transport;

/// Typical GRBL laser baud rate.
pub const DEFAULT_BAUD: u32 = 115_200;

pub struct SerialTransport {
    port: Box<dyn serialport::SerialPort>,
    buffer: Vec<u8>,
}

impl SerialTransport {
    pub fn open(path: &str, baud: u32) -> io::Result<Self> {
        let port = serialport::new(path, baud).timeout(Duration::from_millis(10)).open().map_err(|e| io::Error::other(e.to_string()))?;
        Ok(Self { port, buffer: Vec::new() })
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
            if let Some(pos) = self.buffer.iter().position(|b| *b == b'\n') {
                let line: Vec<u8> = self.buffer.drain(..=pos).collect();
                return Ok(Some(String::from_utf8_lossy(&line).trim().to_string()));
            }
            match self.port.read(&mut byte) {
                Ok(1) => {
                    // A misbehaving device must not make us buffer without bound.
                    if self.buffer.len() < 4096 {
                        self.buffer.push(byte[0]);
                    }
                }
                Ok(_) => {}
                Err(e) if e.kind() == io::ErrorKind::TimedOut => {}
                Err(e) => return Err(e),
            }
            if Instant::now() >= deadline {
                return Ok(None);
            }
        }
    }
}
