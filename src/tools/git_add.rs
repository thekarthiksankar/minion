use serde::Deserialize;
use std::path::Path;
use std::process::Command;

use super::path::resolve_readable_path;
use super::{Tool, ToolCallInput};
use crate::llm::ToolSchema;

pub struct GitAddTool;

#[derive(Deserialize)]
struct GitAddInput {
    files: Vec<String>,
}

impl GitAddTool {
    fn execute(&self, root: &Path, input: GitAddInput) -> anyhow::Result<String> {
        for file in &input.files {
            resolve_readable_path(root, file)?;
        }

        let output = Command::new("git")
            .arg("add")
            .args(&input.files)
            .current_dir(root)
            .output()
            .map_err(|e| anyhow::anyhow!("failed to run git add: {e}"))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("git add failed: {stderr}");
        }

        Ok(format!("staged: {}", input.files.join(", ")))
    }
}

impl Tool for GitAddTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "git_add".into(),
            description: "Stage files in the working directory for the next commit.".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "files": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "List of file paths to stage, relative to the working directory."
                    }
                },
                "required": ["files"]
            }),
        }
    }

    fn run(&self, root: &Path, input: ToolCallInput) -> anyhow::Result<String> {
        let input: GitAddInput = serde_json::from_value(input)?;
        self.execute(root, input)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn init_repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        Command::new("git").arg("init").current_dir(dir.path()).output().unwrap();
        Command::new("git")
            .args(["config", "user.email", "test@test.com"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        dir
    }

    #[test]
    fn stages_a_file() {
        let dir = init_repo();
        std::fs::write(dir.path().join("hello.txt"), "hello").unwrap();

        let result = GitAddTool
            .run(dir.path(), serde_json::json!({ "files": ["hello.txt"] }))
            .unwrap();

        assert!(result.contains("staged"));

        let status = Command::new("git")
            .args(["status", "--porcelain"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        assert!(String::from_utf8_lossy(&status.stdout).contains("A  hello.txt"));
    }

    #[test]
    fn rejects_nonexistent_file() {
        let dir = init_repo();
        let err = GitAddTool
            .run(dir.path(), serde_json::json!({ "files": ["missing.txt"] }))
            .unwrap_err();
        assert!(err.to_string().contains("not found"));
    }

    #[test]
    fn rejects_path_escape() {
        let dir = init_repo();
        let outer = tempfile::NamedTempFile::new().unwrap();
        let rel = format!("../{}", outer.path().file_name().unwrap().to_str().unwrap());
        let err = GitAddTool
            .run(dir.path(), serde_json::json!({ "files": [rel] }))
            .unwrap_err();
        assert!(err.to_string().contains("escapes"));
    }
}
