use std::collections::VecDeque;
use std::io;
use std::time::Duration;

use crate::{GRBL_RX_BUFFER, Transport, realtime};

/// An in-memory GRBL. It models the 128-byte RX buffer (so over-sending is detectable) and answers
/// one buffered line per poll, which makes the streamer actually pipeline lines.
#[derive(Debug, Default)]
pub struct SimulatedGrbl {
    pending: VecDeque<String>,
    pending_bytes: usize,
    responses: VecDeque<String>,
    received: Vec<String>,
    partial: String,
    max_used: usize,
    overrun: bool,
    reset: bool,
    silent: bool,
    fail_prefix: Option<String>,
    alarm_prefix: Option<String>,
    position: (f64, f64),
    latency: Duration,
}

impl SimulatedGrbl {
    pub fn new() -> Self {
        Self::default()
    }

    /// Lines the "controller" has accepted, in order.
    pub fn received(&self) -> &[String] {
        &self.received
    }

    /// Peak RX buffer occupancy in bytes.
    pub fn max_buffer_used(&self) -> usize {
        self.max_used
    }

    pub fn overran_buffer(&self) -> bool {
        self.overrun
    }

    pub fn was_reset(&self) -> bool {
        self.reset
    }

    pub fn position(&self) -> (f64, f64) {
        self.position
    }

    /// Lines starting with `prefix` answer `error:20`.
    pub fn fail_on(&mut self, prefix: &str) {
        self.fail_prefix = Some(prefix.into());
    }

    /// Lines starting with `prefix` answer `ALARM:1`.
    pub fn alarm_on(&mut self, prefix: &str) {
        self.alarm_prefix = Some(prefix.into());
    }

    /// Delay per response, so progress is visible in the UI.
    pub fn set_latency(&mut self, latency: Duration) {
        self.latency = latency;
    }

    pub fn go_silent(&mut self) {
        self.silent = true;
    }

    fn process_one(&mut self) {
        let Some(line) = self.pending.pop_front() else { return };
        self.pending_bytes -= line.len() + 1;
        if self.alarm_prefix.as_deref().is_some_and(|p| line.starts_with(p)) {
            self.responses.push_back("ALARM:1".into());
        } else if self.fail_prefix.as_deref().is_some_and(|p| line.starts_with(p)) {
            self.responses.push_back("error:20".into());
        } else {
            self.apply(&line);
            self.responses.push_back("ok".into());
        }
        self.received.push(line);
    }

    fn apply(&mut self, line: &str) {
        for word in line.split_whitespace() {
            let (axis, value) = word.split_at(1);
            if let Ok(v) = value.parse::<f64>() {
                match axis {
                    "X" => self.position.0 = v,
                    "Y" => self.position.1 = v,
                    _ => {}
                }
            }
        }
    }
}

impl Transport for SimulatedGrbl {
    fn write_all(&mut self, bytes: &[u8]) -> io::Result<()> {
        for &b in bytes {
            if b == realtime::SOFT_RESET {
                self.reset = true;
                self.pending.clear();
                self.pending_bytes = 0;
                continue;
            }
            if b == realtime::STATUS_QUERY {
                self.responses.push_back(format!("<Idle|MPos:{:.3},{:.3},0.000|FS:0,0>", self.position.0, self.position.1));
                continue;
            }
            if b == b'\n' {
                let line = std::mem::take(&mut self.partial);
                self.pending_bytes += line.len() + 1;
                self.pending.push_back(line);
            } else {
                self.partial.push(char::from(b));
            }
            self.max_used = self.max_used.max(self.pending_bytes + self.partial.len());
            if self.pending_bytes + self.partial.len() > GRBL_RX_BUFFER {
                self.overrun = true;
            }
        }
        Ok(())
    }

    fn read_line(&mut self, timeout: Duration) -> io::Result<Option<String>> {
        if self.silent {
            std::thread::sleep(timeout);
            return Ok(None);
        }
        if self.responses.is_empty() {
            if !self.latency.is_zero() && !self.pending.is_empty() {
                std::thread::sleep(self.latency);
            }
            self.process_one();
        }
        Ok(self.responses.pop_front())
    }
}
