# minion

## Working agreement

How changes should be made in this repo. These exist because unrequested cleanup and
unreviewed rewrites have slipped real bugs and noise into diffs before.

1. **Plan first, code second.** For any non-trivial change, propose the approach and get
   agreement *before* touching files. Don't jump straight to writing code, and don't reach
   for a quick patch when the real issue is the design — name the design problem first.

2. **Minimal diff.** Make the smallest change that does the job. Prefer targeted edits over
   whole-file rewrites. Do **not** reformat, rename, or delete comments in code unrelated to
   the task. If a full rewrite is genuinely needed, diff it against the original afterward and
   be ready to justify every changed line.

3. **Self-audit before finishing.** After making a change, review your own diff: list what
   changed and why, flag anything not strictly required by the task, and re-check the logic in
   the exact path the change targets (e.g. the interruption/recovery path for a crash-safety
   fix). Add a test for the specific scenario being fixed when feasible.

<!-- Codebase docs can be added below later (e.g. via /init). -->
