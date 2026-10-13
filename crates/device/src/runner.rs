use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use crate::{DeviceError, Progress, StreamOptions, StreamReport, Transport, stream_program};

/// Streams a program on a background thread so the UI stays responsive.
pub struct JobRunner {
    handle: Option<JoinHandle<Result<StreamReport, DeviceError>>>,
    progress: Arc<Mutex<Progress>>,
    cancel: Arc<AtomicBool>,
}

impl JobRunner {
    pub fn start(mut transport: Box<dyn Transport + Send>, program: String) -> Self {
        let progress = Arc::new(Mutex::new(Progress { sent: 0, acknowledged: 0, total: 0 }));
        let cancel = Arc::new(AtomicBool::new(false));
        let (p, c) = (Arc::clone(&progress), Arc::clone(&cancel));
        let handle = std::thread::spawn(move || {
            stream_program(transport.as_mut(), &program, &StreamOptions::default(), &c, |pr| {
                if let Ok(mut guard) = p.lock() {
                    *guard = pr;
                }
            })
        });
        Self { handle: Some(handle), progress, cancel }
    }

    pub fn progress(&self) -> Progress {
        self.progress.lock().map(|p| *p).unwrap_or(Progress { sent: 0, acknowledged: 0, total: 0 })
    }

    /// Asks the job to stop; the streamer soft-resets the controller (laser off).
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    /// `None` while running; the outcome once, after the thread has finished.
    pub fn poll(&mut self) -> Option<Result<StreamReport, DeviceError>> {
        if !self.handle.as_ref()?.is_finished() {
            return None;
        }
        let handle = self.handle.take()?;
        Some(handle.join().unwrap_or_else(|_| Err(DeviceError::Io(std::io::Error::other("the streaming thread crashed")))))
    }

    pub fn is_running(&self) -> bool {
        self.handle.is_some()
    }
}

impl Drop for JobRunner {
    fn drop(&mut self) {
        // Never leave a laser job running unattended when its owner goes away.
        self.cancel();
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use crate::SimulatedGrbl;

    use super::*;

    fn wait(runner: &mut JobRunner) -> Result<StreamReport, DeviceError> {
        let start = Instant::now();
        loop {
            if let Some(r) = runner.poll() {
                return r;
            }
            assert!(start.elapsed() < Duration::from_secs(10), "runner hung");
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    #[test]
    fn runs_to_completion_in_the_background() {
        let mut r = JobRunner::start(Box::new(SimulatedGrbl::new()), "G21\nG90\nG1 X1 Y1 F100\n".into());
        assert_eq!(wait(&mut r).unwrap().lines, 3);
        assert_eq!(r.progress().acknowledged, 3);
        assert!(!r.is_running());
    }

    #[test]
    fn cancel_stops_a_slow_job() {
        let mut sim = SimulatedGrbl::new();
        sim.set_latency(Duration::from_millis(5));
        let program: String = (0..2000).map(|i| format!("G1 X{} Y0 F100\n", i % 100)).collect();
        let mut r = JobRunner::start(Box::new(sim), program);
        std::thread::sleep(Duration::from_millis(30));
        r.cancel();
        assert!(matches!(wait(&mut r), Err(DeviceError::Cancelled)));
    }

    #[test]
    fn errors_surface_through_poll() {
        let mut sim = SimulatedGrbl::new();
        sim.fail_on("G1");
        let mut r = JobRunner::start(Box::new(sim), "G1 X1\n".into());
        assert!(matches!(wait(&mut r), Err(DeviceError::Grbl { .. })));
    }
}
