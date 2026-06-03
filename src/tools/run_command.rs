use serde::Deserialize;
use std::path::Path;
use std::process::Command;

use super::{Tool, ToolCallInput};
use crate::llm::ToolSchema;

pub struct RunCommandTool;

#[derive(Deserialize)]
struct RunCommandInput {
    command: String,
    args: Option<Vec<String>>,
}

impl RunCommandTool {
    fn execute(&self, root: &Path, input: RunCommandInput) -> anyhow::Result<String> {
        let output = Command::new(&input.command)
            .args(input.args.unwrap_or_default())
            .current_dir(root)
            .output()
            .map_err(|e| anyhow::anyhow!("failed to run '{}': {e}", input.command))?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        let exit_code = output.status.code().unwrap_or(-1);

        Ok(format!(
            "exit_code: {exit_code}\nstdout:\n{stdout}\nstderr:\n{stderr}"
        ))
    }
}

impl Tool for RunCommandTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "run_command".into(),
            description: "Run a shell command inside the working directory. \
                          Returns exit_code, stdout, and stderr as labelled sections. \
                          A non-zero exit_code means the command failed."
                .into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "command": {
                        "type": "string",
                        "description": "The program to run (e.g. 'cargo', 'git', 'ls')."
                    },
                    "args": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Optional. Arguments to pass to the command."
                    }
                },
                "required": ["command"]
            }),
        }
    }

    fn summary(&self, input: &ToolCallInput) -> String {
        let cmd = input["command"].as_str().unwrap_or("?");
        let args = input["args"]
            .as_array()
            .map(|a| a.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>().join(" "))
            .unwrap_or_default();
        if args.is_empty() { cmd.to_string() } else { format!("{cmd} {args}") }
    }

    fn run(&self, root: &Path, input: ToolCallInput) -> anyhow::Result<String> {
        let input: RunCommandInput = serde_json::from_value(input)?;
        self.execute(root, input)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runs_command_and_captures_output() {
        let dir = tempfile::tempdir().unwrap();
        let result = RunCommandTool
            .run(dir.path(), serde_json::json!({ "command": "echo", "args": ["hello"] }))
            .unwrap();
        assert!(result.contains("hello"));
        assert!(result.contains("exit_code: 0"));
    }

    #[test]
    fn captures_non_zero_exit_code() {
        let dir = tempfile::tempdir().unwrap();
        let result = RunCommandTool
            .run(dir.path(), serde_json::json!({ "command": "ls", "args": ["nonexistent_path_xyz"] }))
            .unwrap();
        assert!(!result.contains("exit_code: 0"));
    }

    #[test]
    fn runs_without_args() {
        let dir = tempfile::tempdir().unwrap();
        let result = RunCommandTool
            .run(dir.path(), serde_json::json!({ "command": "pwd" }))
            .unwrap();
        assert!(result.contains("exit_code: 0"));
    }

    #[test]
    fn returns_error_for_unknown_command() {
        let dir = tempfile::tempdir().unwrap();
        let err = RunCommandTool
            .run(dir.path(), serde_json::json!({ "command": "nonexistent_binary_xyz" }))
            .unwrap_err();
        assert!(err.to_string().contains("failed to run"));
    }
}
