use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use serde::Serialize;

use super::TelemetryBackend;

pub struct RunLogBackend {
    inner: Mutex<Inner>,
}

struct Inner {
    log: BufWriter<File>,
    run_dir: PathBuf,
    meta: RunMeta,
    steps: Vec<StepRecord>,
    current_step: Option<OpenStep>,
}

struct OpenStep {
    name: String,
    start: Instant,
    turns: Vec<TurnRecord>,
    current_turn: Option<OpenTurn>,
}

struct OpenTurn {
    number: u32,
    start: Instant,
    tool_calls: Vec<ToolCallRecord>,
}

// ── JSON structures ──────────────────────────────────────────────────────────

#[derive(Serialize, Clone)]
struct RunMeta {
    run_id: String,
    task: String,
    branch: String,
    repo: String,
    started_at: String,
}

#[derive(Serialize)]
struct StepRecord {
    name: String,
    duration_ms: u64,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    turns: Vec<TurnRecord>,
}

#[derive(Serialize, Clone)]
struct TurnRecord {
    number: u32,
    duration_ms: u64,
    input_tokens: u32,
    output_tokens: u32,
    tool_calls: Vec<ToolCallRecord>,
}

#[derive(Serialize, Clone)]
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

// ── Construction ─────────────────────────────────────────────────────────────

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

        let log_path = run_dir.join("run.log");
        let log = BufWriter::new(File::create(log_path)?);

        Ok(Self {
            inner: Mutex::new(Inner {
                log,
                run_dir,
                meta,
                steps: Vec::new(),
                current_step: None,
            }),
        })
    }
}

// ── Backend impl ─────────────────────────────────────────────────────────────

impl TelemetryBackend for RunLogBackend {
    fn run_phase(&self, number: u32, total: u32, name: &str) {
        let mut g = self.inner.lock().unwrap();
        let line = format!("\n[{number}/{total}] {name}\n");
        print!("{line}");
        let _ = g.log.write_all(line.as_bytes());
    }

    fn info(&self, tag: &str, message: &str) {
        let mut g = self.inner.lock().unwrap();
        emit(&mut g.log, "I", tag, message);
    }

    fn step_started(&self, name: &str) {
        let mut g = self.inner.lock().unwrap();
        emit(&mut g.log, "I", "step", &format!("{name} started"));
        g.current_step = Some(OpenStep {
            name: name.to_string(),
            start: Instant::now(),
            turns: Vec::new(),
            current_turn: None,
        });
    }

    fn step_finished(&self, name: &str, duration_ms: u64) {
        let mut g = self.inner.lock().unwrap();
        emit(&mut g.log, "I", "step", &format!("{name} finished [{duration_ms}ms]"));
        if let Some(step) = g.current_step.take() {
            g.steps.push(StepRecord { name: step.name, duration_ms, turns: step.turns });
        }
    }

    fn turn_started(&self, number: u32) {
        let mut g = self.inner.lock().unwrap();
        emit(&mut g.log, "I", "turn", &format!("#{number} started"));
        if let Some(step) = g.current_step.as_mut() {
            step.current_turn = Some(OpenTurn {
                number,
                start: Instant::now(),
                tool_calls: Vec::new(),
            });
        }
    }

    fn turn_finished(&self, number: u32, duration_ms: u64, input_tokens: u32, output_tokens: u32) {
        let mut g = self.inner.lock().unwrap();
        emit(
            &mut g.log,
            "I",
            "turn",
            &format!("#{number} complete [{duration_ms}ms | {input_tokens}↑ {output_tokens}↓ tokens]"),
        );
        if let Some(step) = g.current_step.as_mut() {
            if let Some(turn) = step.current_turn.take() {
                step.turns.push(TurnRecord {
                    number: turn.number,
                    duration_ms,
                    input_tokens,
                    output_tokens,
                    tool_calls: turn.tool_calls,
                });
            }
        }
    }

    fn tool_called(&self, name: &str, summary: &str, success: bool, duration_ms: u64, error: Option<&str>) {
        let mut g = self.inner.lock().unwrap();
        let status = if success {
            format!("ok [{duration_ms}ms]")
        } else {
            format!("failed [{duration_ms}ms] — {}", error.unwrap_or(""))
        };
        let level = if success { "I" } else { "E" };
        emit(&mut g.log, level, "tool", &format!("{name}({summary}) → {status}"));

        if let Some(step) = g.current_step.as_mut() {
            if let Some(turn) = step.current_turn.as_mut() {
                turn.tool_calls.push(ToolCallRecord {
                    tool: name.to_string(),
                    summary: summary.to_string(),
                    success,
                    duration_ms,
                    error: error.map(str::to_string),
                });
            }
        }
    }

    fn finish(&self, outcome: &str, error: Option<&str>, duration_ms: u64) -> anyhow::Result<()> {
        let mut g = self.inner.lock().unwrap();

        if let Some(msg) = error {
            emit(&mut g.log, "E", "run", &format!("failed — {msg}"));
        }

        let _ = g.log.flush();

        let steps = std::mem::take(&mut g.steps);

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

        let summary = ReportSummary {
            total_turns,
            total_tool_calls,
            failed_tool_calls,
            total_input_tokens,
            total_output_tokens,
        };

        let report = TelemetryReport {
            meta: g.meta.clone(),
            finished_at: now_iso8601(),
            duration_ms,
            outcome: outcome.to_string(),
            error: error.map(str::to_string),
            steps,
            summary: ReportSummary {
                total_turns: summary.total_turns,
                total_tool_calls: summary.total_tool_calls,
                failed_tool_calls: summary.failed_tool_calls,
                total_input_tokens: summary.total_input_tokens,
                total_output_tokens: summary.total_output_tokens,
            },
        };

        let report_path = g.run_dir.join("telemetry.json");
        fs::write(&report_path, serde_json::to_string_pretty(&report)?)?;

        let run_dir = g.run_dir.display().to_string();
        let run_id = g.meta.run_id.clone();
        print_summary(outcome, error, duration_ms, &summary, &run_dir, &run_id);

        Ok(())
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn emit(log: &mut BufWriter<File>, level: &str, tag: &str, message: &str) {
    let ts = wall_time();
    let line = format!("{ts}  {level}  [{tag:<8}]  {message}\n");
    print!("{line}");
    let _ = log.write_all(line.as_bytes());
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
