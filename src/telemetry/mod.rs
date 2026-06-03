mod run_log;

pub use run_log::RunLogBackend;

use std::time::Instant;

/// The public interface for recording telemetry events.
/// Components call methods on this — the backend decides how to store and display them.
pub trait TelemetryBackend: Send + Sync {
    fn run_phase(&self, number: u32, total: u32, name: &str);
    fn info(&self, tag: &str, message: &str);
    fn step_started(&self, name: &str);
    fn step_finished(&self, name: &str, duration_ms: u64);
    fn turn_started(&self, number: u32);
    fn turn_finished(&self, number: u32, duration_ms: u64, input_tokens: u32, output_tokens: u32);
    fn tool_called(&self, name: &str, summary: &str, success: bool, duration_ms: u64, error: Option<&str>);
    fn finish(&self, outcome: &str, error: Option<&str>, duration_ms: u64) -> anyhow::Result<()>;
}

/// Facade passed through the system. Components only see this — not the backend.
pub struct Telemetry {
    backend: Box<dyn TelemetryBackend>,
    started: Instant,
}

impl Telemetry {
    pub fn new(backend: Box<dyn TelemetryBackend>) -> Self {
        Self { backend, started: Instant::now() }
    }

    pub fn run_phase(&self, number: u32, total: u32, name: &str) {
        self.backend.run_phase(number, total, name);
    }

    pub fn info(&self, tag: &str, message: &str) {
        self.backend.info(tag, message);
    }

    pub fn step_started(&self, name: &str) {
        self.backend.step_started(name);
    }

    pub fn step_finished(&self, name: &str, duration_ms: u64) {
        self.backend.step_finished(name, duration_ms);
    }

    pub fn turn_started(&self, number: u32) {
        self.backend.turn_started(number);
    }

    pub fn turn_finished(&self, number: u32, duration_ms: u64, input_tokens: u32, output_tokens: u32) {
        self.backend.turn_finished(number, duration_ms, input_tokens, output_tokens);
    }

    pub fn tool_called(&self, name: &str, summary: &str, success: bool, duration_ms: u64, error: Option<&str>) {
        self.backend.tool_called(name, summary, success, duration_ms, error);
    }

    pub fn finish(&self, outcome: &str, error: Option<&str>) -> anyhow::Result<()> {
        self.backend.finish(outcome, error, self.started.elapsed().as_millis() as u64)
    }
}
