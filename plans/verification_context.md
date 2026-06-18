# Context: Change Verification for Minion Agent

## The Problem

Minion is a one-shot coding agent that runs a task on a git branch and pushes it.
Previously, a run could be reported as `succeeded` even when the agent made no commits
and did not complete the task. Example run `019ea0cd`: the agent spent 6 turns
failing to locate a "spec" file, returned an empty response, and the loop treated
that as clean completion → branch was pushed → `outcome: "succeeded"`.

## Fix Already Implemented

A `task_complete` tool was added. The agent **must** call it explicitly to signal
completion. The loop now has three deterministic exit paths:

| Agent action | Loop outcome | Telemetry outcome |
|---|---|---|
| Calls `task_complete(summary=...)` | `Complete` | `succeeded` |
| Stops calling tools without `task_complete` | `Abandoned` | `task_abandoned` |
| Hits 40-turn limit | `StepLimitExhausted` | `step_limit_exhausted` |
| LLM/tool error | `Failed` | `failed` |

The `task_complete` tool schema:
```json
{
  "name": "task_complete",
  "input_schema": {
    "type": "object",
    "properties": {
      "summary": { "type": "string", "description": "Short description of what was accomplished." }
    },
    "required": ["summary"]
  }
}
```

When `task_complete` is called, the agent's `summary` string is logged as a
`tool_called` event in the run log with `tool: "task_complete"` and `summary: <value>`.

## What Verification Needs to Do

After a run reaches `outcome: "succeeded"`, we have:
1. The agent's claimed `summary` (from `task_complete`)
2. The actual git diff on the branch (`git diff main..HEAD`)
3. The original task description
4. The full run log with every tool call

The open question is: **did the agent actually do what the task asked?**

Verification could work at multiple levels:
- **Structural**: Did any commits land? (`git log main..HEAD` non-empty)
- **Diff-based**: Does the diff touch the files/areas the task described?
- **LLM-based**: Ask a model whether the diff satisfies the task description and matches the agent's summary
- **Test-based**: Run the repo's test suite on the branch and check exit code

## Relevant Source Files

```
src/
  agent/mod.rs          — loop logic, task_complete detection, LoopOutcome enum
  state/mod.rs          — run_state_machine, RunOutcome enum, outcome → telemetry mapping
  state/push_branch.rs  — git push step (runs after Complete, before succeeded)
  tools/task_complete.rs — task_complete tool definition
  telemetry/run_log.rs  — NDJSON event log schema (Event enum) and report derivation
```

## Run Log Format

Every run writes an append-only NDJSON file at:
```
<repo>/.minion/runs/<run-id>/run.log
```

Each line is a JSON object with a `ts` timestamp and an `event` field. Relevant events for verification:

```jsonc
// Agent declared completion — summary is the claim to verify
{"ts":"...","event":"tool_called","tool":"task_complete","summary":"renamed lexlog-product-doc.md to README.md","success":true,"duration_ms":0}

// Overall outcome
{"ts":"...","event":"run_finished","outcome":"succeeded","duration_ms":38382}
```

The derived `telemetry.json` in the same directory has a structured summary but
`run.log` is the authoritative source.

## Worked Example — The Failing Run

Run id: `019ea0cd-6a0f-7581-827e-29fd2eac2b85`  
Repo: `/Users/beast/Workspace/lexlog`  
Task: `rename spec file to readme.md`  
Log: `/Users/beast/Workspace/lexlog/.minion/runs/019ea0cd-6a0f-7581-827e-29fd2eac2b85/run.log`

What happened turn by turn:
1. `ls` → saw `lexlog-product-doc.md`, `mvp.md` (no file named "spec")
2. `ls -i *spec*` → no such file
3. `find -R . | grep spec` → illegal flag (macOS find)
4. `ls -R | grep spec` → dispatcher error (pipe not supported as single command)
5. `find . -name \"*spec*\"` → empty result
6. LLM returned `content: []` (empty, 70 output tokens) — gave up silently

Outcome was `succeeded`. Branch was pushed with zero commits. Under the new protocol
this run would emit `outcome: "task_abandoned"` and clean up the branch.

## Open Design Questions for Verification

1. Where does verification run — as a fourth state machine phase after push, or as a
   separate command (`minion verify <run-id>`)?
2. What is the verification signal — LLM judge, test suite, diff heuristic, or all three?
3. What happens on verification failure — revert the branch, flag it, retry the run?
4. How does the verifier get the original task? It's in `run.log` as `info` events and
   in `telemetry.json` under `meta.task`.
