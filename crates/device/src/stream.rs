use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use web_time::Instant;

use crate::{DeviceError, GRBL_RX_BUFFER, Transport, realtime};

/// GRBL's maximum line length including the terminator is 80; we keep a margin.
pub const MAX_LINE_BYTES: usize = 79;

#[derive(Debug, Clone)]
pub struct StreamOptions {
    /// Give up if the controller says nothing for this long (long moves can take a while).
    pub idle_timeout: Duration,
    /// How long one poll waits for a response.
    pub poll: Duration,
}

impl Default for StreamOptions {
    fn default() -> Self {
        Self { idle_timeout: Duration::from_secs(60), poll: Duration::from_millis(20) }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Progress {
    pub sent: usize,
    pub acknowledged: usize,
    pub total: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamReport {
    pub lines: usize,
}

/// The protocol state of one program being streamed: which lines are in flight and how many
/// bytes of GRBL's RX buffer they occupy. Shared by [`stream_program`] and the persistent link.
pub(crate) struct JobStream {
    lines: Vec<String>,
    in_flight: VecDeque<usize>,
    in_flight_bytes: usize,
    next: usize,
    acked: usize,
}

impl JobStream {
    pub(crate) fn new(program: &str) -> Result<Self, DeviceError> {
        Ok(Self { lines: prepare(program)?, in_flight: VecDeque::new(), in_flight_bytes: 0, next: 0, acked: 0 })
    }

    pub(crate) fn progress(&self) -> Progress {
        Progress { sent: self.next, acknowledged: self.acked, total: self.lines.len() }
    }

    pub(crate) fn is_done(&self) -> bool {
        self.acked >= self.lines.len()
    }

    pub(crate) fn has_in_flight(&self) -> bool {
        !self.in_flight.is_empty()
    }

    /// Sends as many lines as fit in the controller's RX buffer.
    pub(crate) fn pump(&mut self, transport: &mut dyn Transport) -> Result<(), DeviceError> {
        while let Some(line) = self.lines.get(self.next) {
            let cost = line.len() + 1;
            if self.in_flight_bytes + cost > GRBL_RX_BUFFER - 1 {
                break;
            }
            transport.write_all(format!("{line}\n").as_bytes())?;
            self.in_flight.push_back(cost);
            self.in_flight_bytes += cost;
            self.next += 1;
        }
        Ok(())
    }

    /// Feeds one controller response. `Ok(true)` if it was an acknowledgement of a streamed line.
    pub(crate) fn on_response(&mut self, resp: &str) -> Result<bool, DeviceError> {
        let resp = resp.trim();
        if resp == "ok" {
            self.in_flight_bytes -= self.in_flight.pop_front().unwrap_or(0);
            self.acked += 1;
            Ok(true)
        } else if let Some(code) = resp.strip_prefix("error:") {
            let index = self.acked;
            Err(DeviceError::Grbl {
                code: code.trim().parse().unwrap_or(0),
                line: index + 1,
                text: self.lines.get(index).cloned().unwrap_or_default(),
            })
        } else if let Some(code) = resp.strip_prefix("ALARM:") {
            Err(DeviceError::Alarm(code.trim().parse().unwrap_or(0)))
        } else {
            // Status reports, "[MSG:...]" and banners are informational.
            Ok(false)
        }
    }
}

/// Streams a G-code program with GRBL's character-counting protocol: keep as many lines in flight
/// as fit in the 128-byte RX buffer and free space as `ok`/`error` responses arrive.
pub fn stream_program(
    transport: &mut dyn Transport,
    program: &str,
    options: &StreamOptions,
    cancel: &AtomicBool,
    mut on_progress: impl FnMut(Progress),
) -> Result<StreamReport, DeviceError> {
    let mut job = JobStream::new(program)?;
    let mut last_activity = Instant::now();

    while !job.is_done() {
        if cancel.load(Ordering::Relaxed) {
            // Reset flushes GRBL's buffers and stops motion; the laser is switched off by the reset.
            transport.write_all(&[realtime::SOFT_RESET])?;
            return Err(DeviceError::Cancelled);
        }
        let before = job.progress().sent;
        job.pump(transport)?;
        if job.progress().sent != before {
            on_progress(job.progress());
        }
        match transport.read_line(options.poll)? {
            Some(resp) => {
                last_activity = Instant::now();
                if job.on_response(&resp)? {
                    on_progress(job.progress());
                }
            }
            None if last_activity.elapsed() > options.idle_timeout => return Err(DeviceError::Timeout(options.idle_timeout.as_secs())),
            None => {}
        }
    }
    Ok(StreamReport { lines: job.progress().total })
}

/// A byte that must never be sent as part of a G-code line: GRBL treats `?`, `!`, `~` and the soft
/// reset (0x18) as real-time commands wherever they appear (so a stray one could pause, resume or
/// reset the machine mid-job), and control, DEL and non-ASCII bytes are not valid G-code.
pub(crate) fn forbidden_byte(line: &str) -> Option<u8> {
    line.bytes().find(|b| matches!(b, b'?' | b'!' | b'~') || (*b < 0x20 && *b != b'\t') || *b >= 0x7f)
}

/// Strips comments and blank lines; refuses lines GRBL would truncate.
pub(crate) fn prepare(program: &str) -> Result<Vec<String>, DeviceError> {
    let mut out = Vec::new();
    for (i, raw) in program.lines().enumerate() {
        // GRBL acts on these bytes wherever they appear, comments included.
        if let Some(byte) = forbidden_byte(raw) {
            return Err(DeviceError::ForbiddenByte { line: i + 1, byte });
        }
        let code = raw.split(';').next().unwrap_or("");
        let code = strip_paren_comments(code);
        let code = code.trim();
        if code.is_empty() {
            continue;
        }
        if code.len() > MAX_LINE_BYTES {
            return Err(DeviceError::LineTooLong { line: i + 1, len: code.len() });
        }
        out.push(code.to_string());
    }
    Ok(out)
}

fn strip_paren_comments(s: &str) -> String {
    let mut depth = 0u32;
    s.chars()
        .filter(|c| match c {
            '(' => {
                depth += 1;
                false
            }
            ')' => {
                depth = depth.saturating_sub(1);
                false
            }
            _ => depth == 0,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicBool;

    use crate::SimulatedGrbl;

    use super::*;

    fn program(n: usize) -> String {
        (0..n).map(|i| format!("G1 X{}.{:03} Y{} S500 F1000\n", i % 300, i % 1000, i % 200)).collect()
    }

    mod flow_tests {
        use super::*;

        #[test]
        fn streams_a_long_program_without_overrunning_the_rx_buffer() {
            let mut sim = SimulatedGrbl::new();
            let report = stream_program(&mut sim, &program(500), &StreamOptions::default(), &AtomicBool::new(false), |_| {}).unwrap();
            assert_eq!(report.lines, 500);
            assert_eq!(sim.received().len(), 500);
            assert!(sim.max_buffer_used() <= GRBL_RX_BUFFER, "buffer peaked at {}", sim.max_buffer_used());
            assert!(sim.max_buffer_used() > 80, "should actually pipeline, peaked at {}", sim.max_buffer_used());
        }

        #[test]
        fn comments_and_blank_lines_are_not_sent() {
            let mut sim = SimulatedGrbl::new();
            stream_program(&mut sim, "; hi\n\nG21 (units)\nG90 ; abs\n", &StreamOptions::default(), &AtomicBool::new(false), |_| {}).unwrap();
            assert_eq!(sim.received(), ["G21", "G90"]);
        }

        #[test]
        fn progress_reaches_total() {
            let mut sim = SimulatedGrbl::new();
            let mut last = None;
            stream_program(&mut sim, &program(20), &StreamOptions::default(), &AtomicBool::new(false), |p| last = Some(p)).unwrap();
            assert_eq!(last, Some(Progress { sent: 20, acknowledged: 20, total: 20 }));
        }
    }

    mod realtime_byte_tests {
        use super::*;

        #[test]
        fn programs_with_realtime_or_odd_bytes_are_refused_before_anything_is_sent() {
            for bad in
                ["G0 X1\n\u{18}\nG0 X2\n", "G0 X1 (pause !)\n", "G1 X1 ; why?\n", "G0 X1~\n", "G0 X1\u{7}\n", "G0 X1 \u{e9}\n", "G0 X\u{7f}1\n"]
            {
                assert!(matches!(prepare(bad), Err(DeviceError::ForbiddenByte { .. })), "{bad:?}");
                let mut sim = SimulatedGrbl::new();
                let r = stream_program(&mut sim, bad, &StreamOptions::default(), &AtomicBool::new(false), |_| {});
                assert!(matches!(r, Err(DeviceError::ForbiddenByte { .. })), "{bad:?}");
                assert!(sim.received().is_empty() && !sim.was_reset(), "nothing reached the controller for {bad:?}");
            }
            assert!(prepare("G0 X1\tY2 (note)\n; comment\n").is_ok(), "tabs and ordinary comments are fine");
        }
    }

    mod failure_tests {
        use super::*;

        #[test]
        fn controller_errors_name_the_offending_line() {
            let mut sim = SimulatedGrbl::new();
            sim.fail_on("G1 X999");
            let err = stream_program(&mut sim, "G21\nG1 X999 Y0\nG0 X0\n", &StreamOptions::default(), &AtomicBool::new(false), |_| {}).unwrap_err();
            assert!(matches!(err, DeviceError::Grbl { line: 2, .. }), "{err}");
        }

        #[test]
        fn alarm_aborts() {
            let mut sim = SimulatedGrbl::new();
            sim.alarm_on("G1");
            let err = stream_program(&mut sim, "G1 X1\n", &StreamOptions::default(), &AtomicBool::new(false), |_| {}).unwrap_err();
            assert!(matches!(err, DeviceError::Alarm(1)));
        }

        #[test]
        fn overlong_lines_are_refused_before_sending_anything() {
            let mut sim = SimulatedGrbl::new();
            let long = format!("G1 {}", "X1 ".repeat(40));
            let err = stream_program(&mut sim, &long, &StreamOptions::default(), &AtomicBool::new(false), |_| {}).unwrap_err();
            assert!(matches!(err, DeviceError::LineTooLong { .. }));
            assert!(sim.received().is_empty());
        }

        #[test]
        fn cancel_sends_soft_reset() {
            let mut sim = SimulatedGrbl::new();
            let cancel = AtomicBool::new(true);
            let err = stream_program(&mut sim, &program(10), &StreamOptions::default(), &cancel, |_| {}).unwrap_err();
            assert!(matches!(err, DeviceError::Cancelled));
            assert!(sim.was_reset());
        }

        #[test]
        fn silent_controller_times_out() {
            let mut sim = SimulatedGrbl::new();
            sim.go_silent();
            let opts = StreamOptions { idle_timeout: Duration::from_millis(50), poll: Duration::from_millis(5) };
            let err = stream_program(&mut sim, "G21\n", &opts, &AtomicBool::new(false), |_| {}).unwrap_err();
            assert!(matches!(err, DeviceError::Timeout(_)));
        }
    }
}
