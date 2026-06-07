use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use super::TelemetryBackend;

/// The backend writes every event as a single JSON line to `run.log` immediately,
/// with no userspace buffering. The file is opened O_APPEND so each write is
/// atomic at the kernel level. `telemetry.json` is a derived artifact produced
/// at the end by replaying the log — it is never the source of truth.
pub struct RunLogBackend {
    log: Mutex<File>,
    run_dir: PathBuf,
    started: Instant,
    meta: RunMeta,
}

// ── Immutable run identity ────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Clone)]
struct RunMeta {
    run_id: String,
    task: String,
    branch: String,
    repo: String,
    started_at: String,
}

// ── Event schema (append side) ────────────────────────────────────────────────

#[derive(Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
enum Event<'a> {
    RunPhase { number: u32, total: u32, name: &'a str },
    Info { tag: &'a str, message: &'a str },
    StepStarted { name: &'a str },
    StepFinished { name: &'a str, duration_ms: u64 },
    TurnStarted { number: u32 },
    TurnFinished { number: u32, duration_ms: u64, input_tokens: u32, output_tokens: u32 },
    ToolCalled {
        tool: &'a str,
        summary: &'a str,
        success: bool,
        duration_ms: u64,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<&'a str>,
    },
    RunFinished {
        outcome: &'a str,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<&'a str>,
        duration_ms: u64,
    },
    LlmRequest {
        turn: u32,
        provider: &'a str,
        model: &'a str,
        params: serde_json::Value,
        messages: serde_json::Value,
        tools: serde_json::Value,
    },
    LlmResponse {
        turn: u32,
        content: serde_json::Value,
        stop_reason: &'a str,
        input_tokens: u32,
        output_tokens: u32,
        duration_ms: u64,
    },
}

#[derive(Serialize)]
struct LogLine<'a> {
    ts: String,
    #[serde(flatten)]
    event: Event<'a>,
}

// ── Event schema (replay side) ────────────────────────────────────────────────

#[derive(Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
enum StoredEvent {
    RunPhase { number: u32, total: u32, name: String },
    Info { tag: String, message: String },
    StepStarted { name: String },
    StepFinished { name: String, duration_ms: u64 },
    TurnStarted { number: u32 },
    TurnFinished { number: u32, duration_ms: u64, input_tokens: u32, output_tokens: u32 },
    ToolCalled { tool: String, summary: String, success: bool, duration_ms: u64, error: Option<String> },
    RunFinished { outcome: String, error: Option<String>, duration_ms: u64 },
    LlmRequest { turn: u32, provider: String, model: String, params: serde_json::Value, messages: serde_json::Value, tools: serde_json::Value },
    LlmResponse { turn: u32, content: serde_json::Value, stop_reason: String, input_tokens: u32, output_tokens: u32, duration_ms: u64 },
}

// ── Report structures (derived, never authoritative) ──────────────────────────

#[derive(Serialize)]
struct StepRecord {
    name: String,
    duration_ms: u64,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    turns: Vec<TurnRecord>,
}

#[derive(Serialize)]
struct TurnRecord {
    number: u32,
    duration_ms: u64,
    input_tokens: u32,
    output_tokens: u32,
    tool_calls: Vec<ToolCallRecord>,
}

#[derive(Serialize)]
struct ToolCallRecord {
    tool: String,
    summary: String,
    success: bool,
    duration_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[derive(Serialize)]
struct TelemetryReport {
    #[serde(flatten)]
    meta: RunMeta,
    finished_at: String,
    duration_ms: u64,
    outcome: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    steps: Vec<StepRecord>,
    summary: ReportSummary,
}

#[derive(Serialize)]
struct ReportSummary {
    total_turns: u32,
    total_tool_calls: u32,
    failed_tool_calls: u32,
    total_input_tokens: u32,
    total_output_tokens: u32,
}

// ── Construction ──────────────────────────────────────────────────────────────

impl RunLogBackend {
    pub fn new(
        run_id: &str,
        task: &str,
        branch: &str,
        repo: &str,
        run_dir: PathBuf,
    ) -> anyhow::Result<Self> {
        fs::create_dir_all(&run_dir)?;

        let meta = RunMeta {
            run_id: run_id.to_string(),
            task: task.to_string(),
            branch: branch.to_string(),
            repo: repo.to_string(),
            started_at: now_iso8601(),
        };

        // Write meta.json immediately so it exists even if the run crashes.
        let meta_path = run_dir.join("meta.json");
        let meta_json = serde_json::to_string_pretty(&meta)?;
        fs::write(meta_path, meta_json)?;

        let log = OpenOptions::new()
            .create(true)
            .append(true)
            .open(run_dir.join("run.log"))?;

        Ok(Self { log: Mutex::new(log), run_dir, started: Instant::now(), meta })
    }
}

// ── TelemetryBackend impl ─────────────────────────────────────────────────────

impl TelemetryBackend for RunLogBackend {
    fn run_phase(&self, number: u32, total: u32, name: &str) {
        let line = format!("\n[{number}/{total}] {name}\n");
        print!("{line}");
        self.append(Event::RunPhase { number, total, name });
    }

    fn info(&self, tag: &str, message: &str) {
        print_line("I", tag, message);
        self.append(Event::Info { tag, message });
    }

    fn step_started(&self, name: &str) {
        print_line("I", "step", &format!("{name} started"));
        self.append(Event::StepStarted { name });
    }

    fn step_finished(&self, name: &str, duration_ms: u64) {
        print_line("I", "step", &format!("{name} finished [{duration_ms}ms]"));
        self.append(Event::StepFinished { name, duration_ms });
    }

    fn turn_started(&self, number: u32) {
        print_line("I", "turn", &format!("#{number} started"));
        self.append(Event::TurnStarted { number });
    }

    fn turn_finished(&self, number: u32, duration_ms: u64, input_tokens: u32, output_tokens: u32) {
        print_line(
            "I",
            "turn",
            &format!("#{number} complete [{duration_ms}ms | {input_tokens}↑ {output_tokens}↓ tokens]"),
        );
        self.append(Event::TurnFinished { number, duration_ms, input_tokens, output_tokens });
    }

    fn tool_called(&self, name: &str, summary: &str, success: bool, duration_ms: u64, error: Option<&str>) {
        let status = if success {
            format!("ok [{duration_ms}ms]")
        } else {
            format!("failed [{duration_ms}ms] — {}", error.unwrap_or(""))
        };
        print_line(if success { "I" } else { "E" }, "tool", &format!("{name}({summary}) → {status}"));
        self.append(Event::ToolCalled { tool: name, summary, success, duration_ms, error });
    }

    fn llm_request(&self, turn: u32, provider: &str, model: &str, params: serde_json::Value, messages: serde_json::Value, tools: serde_json::Value) {
        print_line("I", "llm", &format!("→ {provider}/{model} turn #{turn}"));
        self.append(Event::LlmRequest { turn, provider, model, params, messages, tools });
    }

    fn llm_response(&self, turn: u32, content: serde_json::Value, stop_reason: &str, input_tokens: u32, output_tokens: u32, duration_ms: u64) {
        print_line("I", "llm", &format!("← turn #{turn} [{stop_reason} | {input_tokens}↑ {output_tokens}↓ | {duration_ms}ms]"));
        self.append(Event::LlmResponse { turn, content, stop_reason, input_tokens, output_tokens, duration_ms });
    }

    fn finish(&self, outcome: &str, error: Option<&str>, duration_ms: u64) -> anyhow::Result<()> {
        if let Some(msg) = error {
            print_line("E", "run", &format!("failed — {msg}"));
        }
        self.append(Event::RunFinished { outcome, error, duration_ms });
        self.derive_report()
    }
}

// ── Core: append one event ────────────────────────────────────────────────────

impl RunLogBackend {
    fn append(&self, event: Event<'_>) {
        if let Ok(mut line) = serde_json::to_string(&LogLine { ts: wall_time(), event }) {
            line.push('\n');
            let mut f = self.log.lock().unwrap();
            let _ = f.write_all(line.as_bytes());
        }
    }

    /// Replay `run.log` to reconstruct the structured report.
    /// Called at the end of a normal run. For interrupted runs the log
    /// itself is the artifact — callers can replay it at any time.
    fn derive_report(&self) -> anyhow::Result<()> {
        let content = fs::read_to_string(self.run_dir.join("run.log"))?;

        let mut steps: Vec<StepRecord> = Vec::new();
        let mut current_step: Option<(String, Vec<TurnRecord>)> = None;
        let mut current_turn: Option<(u32, Vec<ToolCallRecord>)> = None;
        let mut outcome = "unknown".to_string();
        let mut run_error: Option<String> = None;
        let mut duration_ms = self.started.elapsed().as_millis() as u64;

        for line in content.lines() {
            let Ok(val) = serde_json::from_str::<serde_json::Value>(line) else { continue };
            let Ok(stored) = serde_json::from_value::<StoredEvent>(val) else { continue };

            match stored {
                StoredEvent::StepStarted { name } => {
                    current_step = Some((name, Vec::new()));
                }
                StoredEvent::StepFinished { name, duration_ms: d } => {
                    if let Some((_, turns)) = current_step.take() {
                        steps.push(StepRecord { name, duration_ms: d, turns });
                    }
                }
                StoredEvent::TurnStarted { number } => {
                    current_turn = Some((number, Vec::new()));
                }
                StoredEvent::TurnFinished { number, duration_ms: d, input_tokens, output_tokens } => {
                    if let Some((_, tool_calls)) = current_turn.take() {
                        if let Some((_, turns)) = current_step.as_mut() {
                            turns.push(TurnRecord {
                                number,
                                duration_ms: d,
                                input_tokens,
                                output_tokens,
                                tool_calls,
                            });
                        }
                    }
                }
                StoredEvent::ToolCalled { tool, summary, success, duration_ms: d, error } => {
                    if let Some((_, calls)) = current_turn.as_mut() {
                        calls.push(ToolCallRecord { tool, summary, success, duration_ms: d, error });
                    }
                }
                StoredEvent::RunFinished { outcome: o, error: e, duration_ms: d } => {
                    outcome = o;
                    run_error = e;
                    duration_ms = d;
                }
                StoredEvent::RunPhase { .. }
                | StoredEvent::Info { .. }
                | StoredEvent::LlmRequest { .. }
                | StoredEvent::LlmResponse { .. } => {}
            }
        }

        // Capture any open step/turn from an interrupted run.
        if let Some((number, calls)) = current_turn.take() {
            if let Some((_, turns)) = current_step.as_mut() {
                turns.push(TurnRecord {
                    number,
                    duration_ms: 0,
                    input_tokens: 0,
                    output_tokens: 0,
                    tool_calls: calls,
                });
            }
        }
        if let Some((name, turns)) = current_step.take() {
            steps.push(StepRecord { name, duration_ms: 0, turns });
        }

        let summary = compute_summary(&steps);

        let report = TelemetryReport {
            meta: self.meta.clone(),
            finished_at: now_iso8601(),
            duration_ms,
            outcome: outcome.clone(),
            error: run_error.clone(),
            steps,
            summary: ReportSummary {
                total_turns: summary.total_turns,
                total_tool_calls: summary.total_tool_calls,
                failed_tool_calls: summary.failed_tool_calls,
                total_input_tokens: summary.total_input_tokens,
                total_output_tokens: summary.total_output_tokens,
            },
        };

        fs::write(self.run_dir.join("telemetry.json"), serde_json::to_string_pretty(&report)?)?;

        print_summary(
            &outcome,
            run_error.as_deref(),
            duration_ms,
            &summary,
            &self.run_dir.display().to_string(),
            &self.meta.run_id,
        );

        Ok(())
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn compute_summary(steps: &[StepRecord]) -> ReportSummary {
    let (total_turns, total_tool_calls, failed_tool_calls, total_input_tokens, total_output_tokens) =
        steps.iter().fold((0u32, 0u32, 0u32, 0u32, 0u32), |mut acc, s| {
            for t in &s.turns {
                acc.0 += 1;
                acc.1 += t.tool_calls.len() as u32;
                acc.2 += t.tool_calls.iter().filter(|c| !c.success).count() as u32;
                acc.3 += t.input_tokens;
                acc.4 += t.output_tokens;
            }
            acc
        });
    ReportSummary { total_turns, total_tool_calls, failed_tool_calls, total_input_tokens, total_output_tokens }
}

fn print_line(level: &str, tag: &str, message: &str) {
    let ts = wall_time();
    let line = format!("{ts}  {level}  [{tag:<8}]  {message}\n");
    print!("{line}");
}

fn print_summary(outcome: &str, error: Option<&str>, duration_ms: u64, s: &ReportSummary, run_dir: &str, run_id: &str) {
    let secs = duration_ms as f64 / 1000.0;
    let sep = "─".repeat(52);
    println!("\n{sep}");
    println!("  Run Summary");
    println!("{sep}");
    println!("  run id       : {run_id}");
    println!("  outcome      : {outcome}");
    if let Some(msg) = error {
        println!("  error        : {msg}");
    }
    println!("  duration     : {secs:.1}s");
    println!("  turns        : {}", s.total_turns);
    println!("  tool calls   : {} ({} failed)", s.total_tool_calls, s.failed_tool_calls);
    println!("  tokens       : {} in / {} out", s.total_input_tokens, s.total_output_tokens);
    println!("  run log      : {run_dir}/run.log");
    println!("  telemetry    : {run_dir}/telemetry.json");
    println!("{sep}\n");
}

fn wall_time() -> String {
    let d = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    let secs = d.as_secs();
    let millis = d.subsec_millis();
    let h = (secs % 86400) / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    format!("{h:02}:{m:02}:{s:02}.{millis:03}")
}

fn now_iso8601() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    // Gregorian calendar from Unix epoch seconds.
    let z = (secs / 86400) as i64 + 719468;
    let era = if z >= 0 { z / 146097 } else { (z - 146096) / 146097 };
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe as i64 + era * 400 + if month <= 2 { 1 } else { 0 };

    let time = secs % 86400;
    let hh = time / 3600;
    let mm = (time % 3600) / 60;
    let ss = time % 60;

    format!("{year:04}-{month:02}-{day:02}T{hh:02}:{mm:02}:{ss:02}Z")
}
