# Minion: Personal Coding Agent
## Requirements & Architecture Specification

---

## 1. Project Overview

### 1.1 Purpose and Goals

Minion is a personal, unattended coding agent that runs on a single developer machine. It accepts a task description, implements the change in an isolated environment, validates the result, and produces a reviewable git branch. No human interaction occurs between task input and branch output.

Goals:
- Implement tasks end-to-end without developer involvement during execution
- Produce output that passes local lints and tests before handoff
- Run three concurrent instances without disrupting active development workloads
- Provide enough telemetry to evaluate and improve agent behavior over time
- Allow the LLM backend to be swapped without changing orchestration code

### 1.2 Design Philosophy

- **Load-bearing context only.** Every item in the agent's context window must improve its reasoning. Items that do not are removed.
- **Determinism at the boundaries.** Steps that can be deterministic must be. The LLM handles ambiguity; code handles certainty.
- **Instrument to learn.** Telemetry captures behavior at every layer so agent performance can be measured and improved.
- **Simple over clever.** Prefer the approach with fewer moving parts. Complexity is added only when a simpler alternative fails.

### 1.3 Out of Scope

- Web or GUI interface
- Slack, webhook, or ticket system integration
- Local LLM inference
- Multi-repository support
- Production deployment or cloud hosting
- Automatic PR merge or approval

---

## 2. System Architecture

### 2.1 Layer Diagram and Responsibilities

```
┌─────────────────────────────────────┐
│             Invocation              │  Accepts task input. Starts a run.
├─────────────────────────────────────┤
│          Blueprint Engine           │  Orchestrates state transitions.
├─────────────────────────────────────┤
│           Agent Runtime             │  Runs the LLM loop within a node.
├─────────────────────────────────────┤
│          MCP Tool Layer             │  Exposes file, shell, and git tools.
├─────────────────────────────────────┤
│       Isolation Environment         │  Git worktree per run.
└─────────────────────────────────────┘
```

Each layer has a single responsibility. Layers communicate through defined interfaces only.

### 2.2 Data Flow: Task In → Branch Out

```
Task input
  → Parse signals
  → Gather context (parallel)
  → Implement (agent loop)
  → Lint + autofix (deterministic)
  → Run tests (deterministic)
  → Fix failures if any (agent loop, one retry)
  → Commit + push branch
  → Emit run summary
```

### 2.3 Core Design Decisions and Rationale

| Decision | Rationale |
|---|---|
| Rust + `statig` for state machine | Type-safe state transitions; async-native; no runtime overhead |
| Git worktrees for isolation | Native disk speed; no VM overhead; sufficient blast radius control for personal use |
| MCP for tool interface | Standard schema; tools are independently replaceable |
| LLM abstraction trait | Backend is swappable without touching orchestration |
| Two CI rounds maximum | Diminishing returns beyond two; prevents runaway loops |
| Per-node context scoping | Smaller, focused contexts produce higher-quality outputs |
| Per-node tool curation | Reduces model decision space; improves action accuracy |

---

## 3. Blueprint Engine

### 3.1 State Machine Overview

The blueprint is a state machine implemented with `statig`. Each state corresponds to one stage of a run. States are either deterministic (run code) or agentic (run an LLM loop). The machine transitions based on the output of each state.

### 3.2 Node Types: Deterministic vs Agentic

**Deterministic node**: Executes code. No LLM call. Output is predictable given the same input. Used for steps where behavior must be guaranteed.

**Agentic node**: Starts a scoped LLM conversation. Receives a curated context and tool set. Runs until the task is complete or a step limit is reached.

### 3.3 State Definitions and Transitions

```
GatherContext      (deterministic)
  → always → Implement

Implement          (agentic)
  → success → Lint
  → step_limit_exceeded → Terminate(StepLimit)
  → error → Terminate(Error)

Lint               (deterministic)
  → always → RunTests

RunTests           (deterministic)
  → all_pass → PushBranch
  → failures_exist, retry_count == 0 → Fix
  → failures_exist, retry_count == 1 → PushBranch(WithFailures)

Fix                (agentic)
  → always → RunTests  [increments retry_count]

PushBranch         (deterministic)
  → always → Terminate(Complete)

Terminate          (deterministic)
  → emits run summary, cleans up worktree
```

### 3.4 Retry and Termination Policy

- Fix node runs at most once per run. `retry_count` is tracked in state.
- If tests still fail after Fix, the branch is pushed as-is with failures noted in the run summary.
- Implement node terminates if step limit is reached. Default step limit: 40 turns.
- Any unhandled tool error in an agentic node terminates that node and transitions to the next deterministic state.

### 3.5 Implementation: `statig` + `tokio`

- Each state is a variant of a Rust `enum`.
- State data (task input, retry count, context payload, test results) is held in a shared `RunContext` struct passed through transitions.
- Agentic nodes spawn a `tokio` task for the LLM loop and await completion.
- Deterministic nodes execute inline within the state handler.

```rust
// Illustrative structure only
enum MinionState {
    GatherContext,
    Implement,
    Lint,
    RunTests { retry_count: u8 },
    Fix,
    PushBranch { with_failures: bool },
    Terminate(TerminateReason),
}

enum TerminateReason {
    Complete,
    StepLimit,
    Error(String),
}
```

---

## 4. Context Gathering

### 4.1 Governing Principle: Load-Bearing Context Only

Each item loaded into the context window must improve the agent's reasoning on the current task. If removing an item would not degrade output quality, it is not loaded. The agent's tools handle discovery of anything not pre-loaded.

### 4.2 Signal Extraction from Task Input

Extracted deterministically from raw task text before any lookup:

| Signal type | Extraction method |
|---|---|
| File paths | Regex: strings matching `[\w/.-]+\.\w+` |
| URLs | Regex: `https?://\S+` |
| Identifiers | Regex: CamelCase and snake_case tokens present in the repo |
| Change type | Keyword matching: fix / add / refactor / remove / update |

Extracted signals drive what each parallel lookup fetches.

### 4.3 Rule File Traversal and Scoping

- Walk from repo root to each directory containing a file referenced in the task.
- Load `CLAUDE.md`, `AGENTS.md`, `.cursorrules` at each level encountered.
- Do not load rule files from directories unrelated to the task.
- Strip blank lines and comment-only lines before loading.
- Apply in root-first order. More specific (deeper) rules override less specific ones where they conflict.

### 4.4 Code Search Strategy

- For each extracted identifier, run `ripgrep` across the repo.
- Load at most 3 results: definition site, primary call site, most relevant test file.
- Load ±30 lines around the target symbol, not the full file.
- If a high-certainty dependency (shared interface, config type, imported utility) is identified, load its full definition. This trades tokens for reduced tool-call latency during implementation.

### 4.5 URL and Ticket Fetching

- Fetch each URL found in the task input via the HTTP MCP tool before the agent starts.
- Strip HTML boilerplate, navigation, headers, footers.
- For GitHub issues: load body and last 3 comments only.
- For documentation pages: if a URL fragment is present, extract only the matching section.
- Record fetch success or failure per URL.

### 4.6 Git Context

- Run `git log --oneline -5` for each file identified as relevant.
- Do not load `git blame` at gathering time. Available as a tool call during implementation if needed.

### 4.7 Context Assembly and Ordering

Context is assembled in this order for the Implement node's opening prompt:

1. Task description
2. Rule files (root → specific)
3. Code snippets (most directly referenced first)
4. Fetched URL content
5. Git log

### 4.8 Token Budget Targets per Node

These are upper bounds, not targets to fill.

| Node | Opening context target |
|---|---|
| Implement | ≤ 10K tokens |
| Fix | ≤ 4K tokens (diff + test failure output only) |
| All others | No LLM context — deterministic |

If assembled context exceeds the Implement target, drop in reverse priority order: git log first, then fetched URLs, then code snippets beyond the most directly referenced.

---

## 5. Agent Runtime

### 5.1 LLM Conversation Loop

Each agentic node runs this loop:

```
1. Build opening messages from node context
2. Call LLM with messages + tool schemas
3. If response contains tool calls:
   a. Execute each tool call via MCP tool layer
   b. Append tool results to messages
   c. Increment turn counter
   d. If turn counter >= step limit: exit loop with StepLimit
   e. Go to step 2
4. If response contains no tool calls: exit loop with Complete
```

### 5.2 LLM Abstraction Trait

All LLM calls go through a trait. The state machine and tool layer never reference a specific provider.

```rust
#[async_trait]
pub trait LlmClient: Send + Sync {
    async fn complete(
        &self,
        messages: Vec<Message>,
        tools: Vec<ToolSchema>,
        max_tokens: u32,
    ) -> Result<LlmResponse, LlmError>;
}
```

Concrete implementations: `ClaudeClient`, `OllamaClient` (backlog). Swap at initialization with no changes to orchestration code.

### 5.3 Context Scoping per Node

Each agentic node receives a fresh conversation. It does not inherit the previous node's message history.

| Node | Opening context |
|---|---|
| Implement | Assembled context from Section 4 + task description |
| Fix | `git diff HEAD` of changed files + raw test failure output |

This prevents the Fix node from being anchored on the Implement node's reasoning, which may have caused the failure.

### 5.4 Tool Dispatch and Result Handling

- Tool calls are executed sequentially within a turn.
- Tool output is appended to messages as a `tool_result` role message.
- If a tool call fails, the error message is returned as the tool result. The agent decides how to proceed.
- Tool call inputs and outputs are logged for telemetry before execution.

### 5.5 Step Limits and Loop Termination

| Node | Step limit |
|---|---|
| Implement | 40 turns |
| Fix | 20 turns |

On step limit: exit loop, record `TerminateReason::StepLimit`, transition to next state as defined in Section 3.3.

### 5.6 Supported Models and Per-Node Model Selection

Model selection is per-node. Faster models are used where deep reasoning is not required.

| Node | Model |
|---|---|
| Implement | claude-sonnet-4-5 |
| Fix | claude-sonnet-4-5 |

Future: expose model selection per node in run configuration. Haiku-class models may be evaluated for Fix once baseline one-shot rates are established.

---

## 6. MCP Tool Layer

### 6.1 Tool Design Principles

- Each tool does one thing.
- Tool names are verbs: `read_file`, `write_file`, `run_command`.
- All inputs and outputs are serializable to JSON.
- Tools do not call the LLM. They execute and return.
- Side effects are confined to the active worktree.

### 6.2 Core Tool Definitions

| Tool | Inputs | Output |
|---|---|---|
| `read_file` | `path`, `start_line?`, `end_line?` | File content (lines) |
| `write_file` | `path`, `content` | Success / error |
| `list_dir` | `path`, `depth?` | Directory tree |
| `run_command` | `command`, `args[]` | stdout, stderr, exit code |
| `search_code` | `pattern`, `path?`, `max_results?` | List of matches with file + line |
| `git_status` | — | Changed files list |
| `git_diff` | `files[]?` | Diff output |
| `git_add` | `files[]` | Success / error |
| `git_commit` | `message` | Commit hash |
| `git_log` | `path?`, `n?` | Log lines |
| `git_blame` | `path`, `start_line`, `end_line` | Blame output |
| `fetch_url` | `url` | Stripped text content |

### 6.3 Per-Node Tool Curation

Each agentic node receives only the tools relevant to its task.

| Node | Tools available |
|---|---|
| Implement | All tools in 6.2 |
| Fix | `read_file`, `list_dir`, `run_command`, `git_diff`, `search_code` |

Tools available but never called across runs are candidates for removal from that node's set. Telemetry in Section 11.6 tracks this.

### 6.4 Shell Command Allowlist

`run_command` enforces an allowlist of permitted executables. Commands outside this list return an error.

Default allowlist:
```
git, cargo, rustfmt, clippy, rg (ripgrep),
cat, ls, find, grep, echo,
[project-specific test runner]
```

The allowlist is configurable per project via a config file in the repo root.

### 6.5 Tool Schema Conventions (`rmcp`)

- Tools are defined using `rmcp` schema macros.
- Each tool has a `name`, `description` (one sentence), and typed `parameters`.
- Descriptions are written for model consumption: precise, no filler.
- Optional parameters include a `default` in the schema.

---

## 7. Isolation Environment

### 7.1 Git Worktrees: Setup and Lifecycle

Each run gets a dedicated git worktree. The worktree shares the repo's object store — no duplication of git history or blobs.

Setup sequence:
```
1. Generate run ID (UUID v4)
2. git worktree add .minion/runs/{run_id} -b minion/{run_id}
3. Set RunContext.worktree_path = .minion/runs/{run_id}
4. All agent file operations are relative to worktree_path
```

### 7.2 Per-Run Workspace Management

- Each run operates exclusively within its worktree directory.
- The MCP tool layer resolves all relative paths against `worktree_path`.
- Absolute paths outside `worktree_path` are rejected by the tool layer.
- Runs do not share worktrees. Concurrent runs use separate worktrees on separate branches.

### 7.3 Cleanup Policy

On `Terminate`:
- If run succeeded: retain worktree until branch is pushed. Remove after push confirmation.
- If run failed: retain worktree for 24 hours for inspection. Remove after.
- Manual cleanup: `minion clean` command removes all worktrees older than 24 hours.

### 7.4 Parallelism Model

- Maximum 3 concurrent runs.
- Limit is enforced at invocation time.
- Each run is an independent `tokio` task.
- No shared mutable state between runs. `RunContext` is not shared.

### 7.5 Docker: Backlog

Docker is not used in the initial implementation. Considered for cases where stronger process isolation is needed. See Section 14.1.

---

## 8. Invocation

### 8.1 CLI Interface and Flags

```
minion run "<task description>"   Start a run
minion run -f <file>              Read task from file
minion status                     List active runs and their current state
minion logs <run_id>              Stream or display logs for a run
minion clean                      Remove stale worktrees
minion cancel <run_id>            Terminate a run and clean up its worktree
```

### 8.2 Task Input Formats

| Format | How to use |
|---|---|
| Inline string | `minion run "fix the token expiry bug in auth.rs"` |
| Text file | `minion run -f task.txt` |
| Stdin | `echo "task" \| minion run -` |

Task input may include URLs, file paths, and identifier names. These are extracted as signals per Section 4.2.

### 8.3 Keep-Awake Handling (`caffeinate`)

The CLI wraps each run in `caffeinate -i` to prevent system sleep during execution.

```rust
// Invoked internally — not exposed to user
Command::new("caffeinate")
    .arg("-i")
    .arg("minion-agent")
    .arg("--run-id").arg(&run_id)
    .spawn()
```

The caffeinate process exits when the run terminates. No persistent daemon is required.

### 8.4 Future Invocation Surfaces (Backlog)

- HTTP webhook endpoint
- File watcher (drop a task file into a directory to trigger a run)
- GitHub issue integration

See Section 14.

---

## 9. Feedback and Iteration Loops

### 9.1 Local Lint Pass

Runs as a deterministic node after Implement, before RunTests.

- Executes the project's configured linter (e.g. `clippy`, `rustfmt --check`).
- Applies autofixes where the linter supports them (`--fix`, `--apply`).
- Commits autofix changes under a separate commit message: `minion: lint autofixes`.
- Remaining lint failures (no autofix available) are passed to the agent as context in the Fix node if tests also fail.
- Lint failures alone do not trigger the Fix node. They are noted in the run summary.

### 9.2 Test Runner and Selective Test Selection

- Test files are identified from the set of changed files.
- Naming conventions used for selection: `*_test.rs`, `tests/`, `*_spec.rs`.
- If no test files are identified, the full default test suite runs.
- Test command is configurable per project in the project config file.
- Test output (stdout + stderr) is captured and stored for telemetry and Fix node input.

### 9.3 Failure Parsing and Routing

Test output is parsed to extract:
- Number of tests failed
- Failed test names
- Failure messages and line references

Parsed failure data is passed to the Fix node as structured input, not raw output. This reduces noise in the Fix node's opening context.

### 9.4 Autofix Handling

- Some test frameworks support autofixes (snapshot updates, generated code).
- If the test runner exits with a known autofix flag, the autofix is applied and committed before routing to Fix.
- Autofix detection is framework-specific and configured per project.

### 9.5 Retry Cap and Handoff to Human

- Fix node runs at most once.
- If tests still fail after Fix, the branch is pushed with a run summary noting:
  - Which tests failed
  - What the Fix node attempted
  - Suggested next steps for manual resolution
- The branch is always pushed. A failing run still produces a reviewable starting point.

---

## 10. Output

### 10.1 Branch Naming and Git Hygiene

Branch name format:
```
minion/{run_id_short}/{task_slug}

Example: minion/a3f9b2/fix-token-expiry
```

`task_slug` is the first 5 words of the task description, lowercased, hyphenated, stripped of special characters.

Commit messages follow this format:
```
minion: <one-line summary of change>

Task: <full task description>
Run: <run_id>
```

Lint autofix commits use:
```
minion: lint autofixes
```

### 10.2 PR Description Generation

Generated by a single LLM call (not an agent loop) after the branch is pushed. Input: task description + final `git diff` against base branch.

PR description includes:
- What changed and why
- Files modified
- Tests affected
- Whether any tests failed (if applicable)

PR description is written to stdout and to the run log. The developer opens the PR manually.

### 10.3 Run Logs and Node-Level Trace

Each run produces a structured JSON log at `.minion/logs/{run_id}.json`.

Log contains:
- Run metadata (run_id, task, start time, end time, terminal state)
- One entry per state transition with timestamp
- Per-node: all LLM messages, all tool calls and results, token counts
- Lint and test output

Logs are human-readable via `minion logs <run_id>` which formats the JSON for terminal display.

### 10.4 Cost and Latency Tracking per Run

Recorded in run log and aggregated in SQLite (Section 11.2):
- Input and output token count per node
- Wall time per node
- Total wall time per run
- API call count per node

Used as a diagnostic tool to validate that pruning decisions are effective.

---

## 11. Telemetry and Analytics

### 11.1 Philosophy: Instrument to Learn, Not to Monitor

Telemetry captures agent behavior at every layer to answer questions that are not known in advance. More data is captured than is immediately useful. Storage is local and queryable ad-hoc. No data leaves the machine.

### 11.2 Storage: Structured JSON Logs + SQLite for Aggregation

- Each run writes a complete JSON log to `.minion/logs/{run_id}.json`.
- On run completion, key metrics are inserted into `.minion/metrics.db` (SQLite).
- SQLite enables cross-run queries without parsing JSON logs.
- Pre-built views are defined for common queries (Section 11.9).

### 11.3 Run-Level Telemetry

Captured per run:

| Field | Type | Description |
|---|---|---|
| `run_id` | string | UUID v4 |
| `task` | string | Raw task input |
| `terminal_state` | enum | Complete / StepLimit / Error / CompleteWithFailures |
| `state_path` | string[] | Ordered list of states visited |
| `wall_time_ms` | int | Total run duration |
| `start_time` | ISO8601 | Run start timestamp |

### 11.4 Context Gathering Telemetry

| Field | Type | Description |
|---|---|---|
| `signals.file_refs` | string[] | File paths extracted from task |
| `signals.urls` | string[] | URLs extracted from task |
| `signals.identifiers` | string[] | Code identifiers extracted |
| `rule_files` | `{path, token_count}[]` | Each rule file loaded |
| `preloaded_files` | `{path, lines_loaded, reason}[]` | Code snippets pre-loaded |
| `fetched_urls` | `{url, token_count, success}[]` | URLs fetched |
| `pruning_gap` | string[] | Files fetched by agent via tool call that were not pre-loaded |
| `total_context_tokens` | int | Token count of assembled context |

`pruning_gap` is the primary signal for tuning the gathering strategy.

### 11.5 Agent Behavior Telemetry

Captured per agentic node per run:

| Field | Type | Description |
|---|---|---|
| `node` | string | Node name |
| `turn_count` | int | Number of LLM turns |
| `input_tokens_start` | int | Context tokens at turn 1 |
| `input_tokens_end` | int | Context tokens at final turn |
| `output_tokens_total` | int | Sum of all output tokens |
| `tool_calls` | `{tool, latency_ms, success}[]` | All tool calls in order |
| `tools_available` | string[] | Tools given to this node |
| `tools_unused` | string[] | Tools available but never called |
| `termination_reason` | enum | Complete / StepLimit / Error |

### 11.6 Tool Layer Telemetry

Captured per tool call:

| Field | Type | Description |
|---|---|---|
| `run_id` | string | Parent run |
| `node` | string | Node that made the call |
| `tool` | string | Tool name |
| `input_summary` | string | Sanitized input (paths, not content) |
| `latency_ms` | int | Execution duration |
| `output_tokens` | int | Token count of output |
| `success` | bool | Whether tool returned without error |

### 11.7 Feedback Loop Telemetry

**Lint node:**

| Field | Type | Description |
|---|---|---|
| `issues_found` | int | Total lint issues detected |
| `issues_autofixed` | int | Issues resolved automatically |
| `issues_remaining` | int | Issues not resolvable automatically |

**Test node:**

| Field | Type | Description |
|---|---|---|
| `tests_selected` | int | Tests run |
| `tests_passed` | int | Tests passed |
| `tests_failed` | int | Tests failed |
| `failure_names` | string[] | Names of failing tests |
| `second_push_needed` | bool | Whether Fix node was invoked |

**Fix node:**

| Field | Type | Description |
|---|---|---|
| `failures_on_entry` | string[] | Failing test names when Fix started |
| `terminal_state` | enum | Complete / StepLimit / Error |
| `tests_passed_after_fix` | bool | Whether tests passed after Fix ran |

### 11.8 Learning-Specific Derived Metrics

These are computed from raw telemetry and stored as SQLite views.

| Metric | Definition |
|---|---|
| **One-shot rate** | Runs where all tests pass on first push, no Fix node invoked / total runs |
| **Pruning gap rate** | Runs where `pruning_gap` is non-empty / total runs |
| **Fix effectiveness rate** | Fix node invocations where tests passed after Fix / total Fix node invocations |
| **Tool overprovision rate** | Tool calls where `tools_unused` is non-empty / total agentic node runs, grouped by node |
| **Step limit hit rate** | Runs terminating with StepLimit / total runs |
| **Context growth rate** | `input_tokens_end - input_tokens_start` per node, averaged across runs |
| **Node token share** | Each node's token usage as a percentage of total run tokens |

### 11.9 Query Interface: Pre-Built SQLite Views

```sql
-- One-shot rate over time
CREATE VIEW v_one_shot_rate AS
SELECT
    date(start_time) as day,
    COUNT(*) as total_runs,
    SUM(CASE WHEN terminal_state = 'Complete' AND second_push_needed = 0 THEN 1 ELSE 0 END) as one_shot,
    ROUND(100.0 * SUM(CASE WHEN terminal_state = 'Complete' AND second_push_needed = 0 THEN 1 ELSE 0 END) / COUNT(*), 1) as one_shot_pct
FROM runs
GROUP BY day;

-- Pruning gap: most frequently fetched-but-not-preloaded files
CREATE VIEW v_pruning_gaps AS
SELECT
    gap_file,
    COUNT(*) as frequency
FROM run_pruning_gaps
GROUP BY gap_file
ORDER BY frequency DESC;

-- Tool usage per node
CREATE VIEW v_tool_usage AS
SELECT
    node,
    tool,
    COUNT(*) as call_count,
    ROUND(AVG(latency_ms), 0) as avg_latency_ms,
    SUM(CASE WHEN success = 0 THEN 1 ELSE 0 END) as error_count
FROM tool_calls
GROUP BY node, tool
ORDER BY node, call_count DESC;

-- Tools never called per node (overprovision signal)
CREATE VIEW v_unused_tools AS
SELECT node, tool, COUNT(*) as runs_unused
FROM run_unused_tools
GROUP BY node, tool
ORDER BY runs_unused DESC;

-- Fix effectiveness
CREATE VIEW v_fix_effectiveness AS
SELECT
    COUNT(*) as total_fix_invocations,
    SUM(CASE WHEN tests_passed_after_fix = 1 THEN 1 ELSE 0 END) as fixed,
    ROUND(100.0 * SUM(CASE WHEN tests_passed_after_fix = 1 THEN 1 ELSE 0 END) / COUNT(*), 1) as effectiveness_pct
FROM fix_node_runs;
```

---

## 12. Non-Functional Requirements

### 12.1 Target Hardware: Apple M4 Pro, 24 GB

All performance targets are defined for this hardware.

### 12.2 Concurrent Run Budget

Maximum 3 concurrent minion runs.

Estimated steady-state memory per run:
- Agent runtime process: ~80–200 MB
- Git worktree (shared object store): negligible additional disk
- Spawned subprocesses (linter, test runner): ~100–300 MB, ephemeral

Estimated concurrent background load: 250–700 MB for 3 runs in steady state.

Active development workload (Android Studio, browser, Zed, Obsidian): estimated 6–10 GB. Combined system load: 7–11 GB. Within the 24 GB envelope without pressure.

### 12.3 Memory Envelope per Minion

| Component | Target |
|---|---|
| Agent runtime (Rust binary) | < 50 MB resident |
| Peak during test run | < 500 MB including subprocess |
| Steady state (waiting on API) | < 150 MB |

### 12.4 Latency Targets per Node

| Node | Target wall time |
|---|---|
| GatherContext | < 5 seconds |
| Implement | Determined by LLM + tool calls; no fixed target |
| Lint | < 10 seconds |
| RunTests | Determined by test suite size |
| Fix | Determined by LLM + tool calls |
| PushBranch | < 10 seconds |

### 12.5 Network Stability Considerations

- All API calls use retry with exponential backoff: 3 retries, base delay 1 second.
- On connection failure mid-run, the run is terminated with `TerminateReason::Error` and the partial state is logged.
- Runs are not resumable after a network failure. Restart from invocation.
- A stable network connection is a prerequisite. VPN instability is a known failure source.

---

## 13. Technology Stack

### 13.1 Language: Rust

All orchestration, tool layer, and CLI code is written in Rust.

Reasons specific to this project:
- Type-safe state machine transitions via enum + match eliminate invalid state bugs
- Memory safety without garbage collection keeps per-run memory footprint low
- `tokio` async runtime handles concurrent runs and I/O-bound API calls efficiently
- Single compiled binary simplifies deployment and startup

### 13.2 Dependency Manifest with Rationale

| Crate | Version | Purpose |
|---|---|---|
| `statig` | latest | Async hierarchical state machine |
| `tokio` | 1.x | Async runtime |
| `rmcp` | latest | MCP tool schema and dispatch |
| `reqwest` | 0.12 | HTTP client for Anthropic API and URL fetching |
| `serde` + `serde_json` | 1.x | JSON serialization |
| `clap` | 4.x | CLI argument parsing |
| `rusqlite` | 0.31 | SQLite for telemetry aggregation |
| `uuid` | 1.x | Run ID generation |
| `tracing` | 0.1 | Structured logging |
| `tracing-subscriber` | 0.3 | Log output and JSON formatting |
| `async-trait` | 0.1 | Async trait support for `LlmClient` |
| `anyhow` | 1.x | Error handling |
| `tokio-process` | via tokio | Subprocess execution for tools |

### 13.3 Excluded Alternatives and Why

| Alternative | Reason excluded |
|---|---|
| Python | No meaningful library advantage for this use case; higher memory overhead; slower binary startup |
| goose | Designed for interactive human-supervised use; architectural assumptions conflict with unattended operation |
| Docker | VM layer adds I/O overhead; git worktrees provide sufficient isolation for personal use |
| Local LLM (llama.cpp, Ollama) | Current inference speed on CPU/Neural Engine insufficient for agent turn latency on available models |
| LangGraph / LangChain | Python-only; abstractions add indirection without benefit for a fixed state machine |

---

## 14. Backlog and Future Considerations

### 14.1 Docker Isolation

Consider if: the command allowlist proves insufficient, or if running untrusted third-party codebases becomes a requirement.

Implementation note: on macOS, Docker Desktop uses a Linux VM. File I/O from the container to the host filesystem crosses the VM boundary. Benchmark worktree vs container performance before committing.

### 14.2 Local Model Support

Consider if: API costs become a concern, or offline operation is needed.

Prerequisite: a model that can handle tool-call-heavy agent loops at acceptable turn latency on M4 Pro hardware. Evaluate when capable models reach sufficient inference speed on Apple Silicon via `ollama` or `llama.cpp`.

Implement via a new `LlmClient` trait implementation. No changes to orchestration code required.

### 14.3 Webhook and HTTP Invocation

A local HTTP server accepting task POSTs would enable integration with external tools (GitHub webhooks, issue trackers). Not prioritized until CLI behavior is stable.

### 14.4 Rule File Hot-Reload

Currently rule files are loaded once per run at gather time. Hot-reload would allow rule file changes to take effect without restarting a run. Low priority; rules are stable during a run.

### 14.5 Multi-Repo Support

Currently assumes a single repository per run. Multi-repo support requires:
- Per-repo worktree management
- Cross-repo code search
- Coordinated branch and PR creation

Not in scope until single-repo behavior is fully validated.
