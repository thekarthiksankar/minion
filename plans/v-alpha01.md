# Minion v-alpha01

## Goal

A single end-to-end run completes on a trivial task. The branch is created, the commit exists, the change is correct. No telemetry, no retry logic, no lint/test nodes — just the core loop working.

---

## Scope

**In scope:**
- `minion run "<task>"` CLI command
- Git worktree create and teardown per run
- Minimal context: task description passed directly as the opening prompt
- Claude API client with the conversation loop
- 5 core tools: `read_file`, `write_file`, `run_command`, `git_add`, `git_commit`
- Linear state sequence: GatherContext → Implement → PushBranch
- Branch pushed to origin; developer opens PR manually

**Explicitly deferred:**
- `statig` state machine — use a plain `match` loop first
- Lint node, RunTests node, Fix node
- Telemetry, SQLite, structured JSON logs
- Retry/backoff beyond basic error propagation
- Tool allowlist enforcement
- Context token budgeting
- `minion status`, `minion logs`, `minion clean`, `minion cancel` commands

---

## Timeline Estimate

| Pace | Estimate |
|---|---|
| Part-time (~10 hrs/week) | 4–5 weeks |
| Near-full-time (~30 hrs/week) | 2 weeks |

**Week 1:** Project setup, CLI, worktree isolation. End state: `minion run` parses args and creates a worktree.

**Week 2:** Claude client, core tools, tool dispatcher, agent loop. End state: loop runs and makes tool calls against the worktree.

**Week 3 (part-time) / mid-week-2 (full-time):** Wire states together, push branch, validate on a trivial real task.

---

## Tasks

### #1 — Initialize Rust project and define module structure

Run `cargo new minion`, set up Cargo.toml with all v-alpha01 dependencies, and stub out the module tree.

**Dependencies (Cargo.toml):**
- `tokio` 1.x — async runtime
- `clap` 4.x — CLI argument parsing
- `reqwest` 0.12 — HTTP client for Anthropic API
- `serde` + `serde_json` 1.x — JSON serialization
- `uuid` 1.x — run ID generation
- `anyhow` 1.x — error handling
- `async-trait` 0.1 — async trait support for LlmClient
- `tracing` + `tracing-subscriber` — structured logging

**Module tree:**
```
src/
  main.rs
  cli/       -- argument parsing and entrypoint
  llm/       -- LlmClient trait + ClaudeClient
  tools/     -- MCP tool implementations + dispatcher
  state/     -- RunContext + state machine
  isolation/ -- git worktree management
```

---

### #2 — Build CLI entrypoint with `minion run` command

Use clap to implement `minion run "<task>"` and `minion run -f <file>`. Parse args, validate input, hand off a task string to the run pipeline.

```
minion run "<task description>"   Start a run with inline task
minion run -f <file>              Read task from file
```

No other subcommands needed for alpha.

---

### #3 — Implement git worktree isolation

Generate a UUID v4 run ID. Create a worktree and branch per run. Tear down on exit.

```
git worktree add .minion/runs/{run_id} -b minion/{run_id}
```

- Store `worktree_path` in `RunContext`
- All file tool calls resolve paths relative to `worktree_path`
- Remove worktree on run end (success or failure)

---

### #4 — Define LlmClient trait and implement ClaudeClient

Define the async abstraction. Implement against the Anthropic messages API.

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

- Auth via `ANTHROPIC_API_KEY` env var
- Model: `claude-sonnet-4-6`
- Basic retry with exponential backoff: 3 attempts, base delay 1s
- Parse tool_use and text response blocks from API response

---

### #5 — Implement core MCP tools

Five tools, each executing against the active `worktree_path`:

| Tool | Inputs | Output |
|---|---|---|
| `read_file` | `path`, `start_line?`, `end_line?` | File content |
| `write_file` | `path`, `content` | Success / error |
| `run_command` | `command`, `args[]` | stdout, stderr, exit code |
| `git_add` | `files[]` | Success / error |
| `git_commit` | `message` | Commit hash |

All inputs and outputs serialize to JSON. Paths are validated to stay within `worktree_path`.

---

### #6 — Build tool dispatcher

Map LLM `tool_use` block names to tool implementations. Execute the matched tool, serialize the result, return it as a `tool_result` message.

- Unknown tool name → return error as tool result (agent recovers)
- Execution error → return error as tool result (agent recovers)
- Log tool name, input summary, and success/failure to stdout for alpha debugging

---

### #7 — Implement agent runtime loop

The LLM conversation loop for the Implement node:

```
1. Build opening messages from task context
2. Call LLM with messages + tool schemas
3. If response has tool_use blocks:
   a. Dispatch each tool call
   b. Append tool_result messages
   c. Increment turn counter
   d. If turn counter >= 40: exit StepLimit
   e. Go to step 2
4. If response has no tool_use: exit Complete
```

---

### #8 — Define RunContext and simplified state machine

```rust
struct RunContext {
    run_id: String,
    task: String,
    worktree_path: PathBuf,
}
```

Plain `match`-based state sequence — no `statig` yet:

```
GatherContext  →  build opening prompt from task string
Implement      →  run agent loop
PushBranch     →  git push origin minion/{run_id}
```

Branch name format: `minion/{run_id_short}/{task_slug}`
- `run_id_short`: first 8 chars of UUID
- `task_slug`: first 5 words, lowercased, hyphenated, special chars stripped

---

### #9 — Wire end-to-end and validate on a real task

Connect: CLI → RunContext → state machine → agent loop → branch push.

**Validation target:**
```
minion run "add a hello_world function to src/lib.rs"
```

Run against a real local repo. Verify:
- Worktree is created at `.minion/runs/{run_id}`
- Agent makes tool calls and modifies the file
- Commit exists with correct message format
- Branch is pushed to origin
- Worktree is cleaned up after run

---

## Dependency Order

```
#1 Project setup
 ├─ #2 CLI entrypoint
 ├─ #3 Worktree isolation
 └─ #4 LlmClient + ClaudeClient
     └─ #5 Core tools
         └─ #6 Tool dispatcher
             └─ #7 Agent loop
                 └─ #8 State machine
                     └─ #9 End-to-end validation
```

Tasks #1–#3 are independent of each other and can be done in any order.

---

## Notes

- **#4 is the first real Rust challenge** — async traits, reqwest, deserializing the Anthropic API response shape.
- **#5 is mechanical but useful** — repetitive enough to get comfortable with Rust error handling before the harder pieces.
- **#8 avoids `statig` intentionally** — understand what the transitions feel like before adding the abstraction. Migration to `statig` is a post-alpha task.
- **#9 is where everything breaks** — budget more time here than it looks like it needs.
- Read compiler error messages fully, including the `help:` and `note:` lines. They usually tell you exactly what to write.
