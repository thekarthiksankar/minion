use serde::Deserialize;
use std::path::Path;
use std::process::Command;

use super::{Tool, ToolCallInput};
use crate::llm::ToolSchema;

pub struct GitCommitTool;

#[derive(Deserialize)]
struct GitCommitInput {
    message: String,
}

impl GitCommitTool {
    fn execute(&self, root: &Path, input: GitCommitInput) -> anyhow::Result<String> {
        let output = run_commit(root, &input.message)?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("git commit failed: {stderr}");
        }

        let hash = commit_hash(root)?;
        Ok(format!("committed: {hash}"))
    }
}

impl Tool for GitCommitTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "git_commit".into(),
            description: "Commit all staged changes with a message. Returns the commit hash.".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "message": {
                        "type": "string",
                        "description": "The commit message."
                    }
                },
                "required": ["message"]
            }),
        }
    }

    fn summary(&self, input: &ToolCallInput) -> String {
        input["message"].as_str().unwrap_or("?").to_string()
    }

    fn run(&self, root: &Path, input: ToolCallInput) -> anyhow::Result<String> {
        let input: GitCommitInput = serde_json::from_value(input)?;
        self.execute(root, input)
    }
}

fn run_commit(root: &Path, message: &str) -> anyhow::Result<std::process::Output> {
    Command::new("git")
        .args(["commit", "-m", message])
        .current_dir(root)
        .output()
        .map_err(|e| anyhow::anyhow!("failed to run git commit: {e}"))
}


fn commit_hash(root: &Path) -> anyhow::Result<String> {
    let output = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(root)
        .output()
        .map_err(|e| anyhow::anyhow!("failed to read commit hash: {e}"))?;
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn init_repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        Command::new("git").arg("init").current_dir(dir.path()).output().unwrap();
        dir
    }

    #[test]
    fn commits_staged_changes() {
        let dir = init_repo();
        std::fs::write(dir.path().join("hello.txt"), "hello").unwrap();
        Command::new("git").args(["add", "hello.txt"]).current_dir(dir.path()).output().unwrap();

        let result = GitCommitTool
            .run(dir.path(), serde_json::json!({ "message": "add hello" }))
            .unwrap();

        assert!(result.contains("committed:"));

        let log = Command::new("git")
            .args(["log", "--oneline"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        assert!(String::from_utf8_lossy(&log.stdout).contains("add hello"));
    }


    #[test]
    fn fails_with_nothing_staged() {
        let dir = init_repo();
        let err = GitCommitTool
            .run(dir.path(), serde_json::json!({ "message": "empty" }))
            .unwrap_err();
        assert!(err.to_string().contains("git commit failed"));
    }
}
