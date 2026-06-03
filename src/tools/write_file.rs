use anyhow::Context;
use serde::Deserialize;
use std::path::Path;

use super::path::resolve_writable_path;
use super::{Tool, ToolCallInput};
use crate::llm::ToolSchema;

pub struct WriteFileTool;

#[derive(Deserialize)]
struct WriteFileInput {
    path: String,
    content: String,
}

impl WriteFileTool {
    fn execute(&self, root: &Path, input: WriteFileInput) -> anyhow::Result<String> {
        let abs = resolve_writable_path(root, &input.path)?;
        if let Some(parent) = abs.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("create dirs for '{}'", abs.display()))?;
        }
        std::fs::write(&abs, &input.content)
            .with_context(|| format!("write '{}'", abs.display()))?;
        Ok(format!("wrote {}", input.path))
    }
}

impl Tool for WriteFileTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "write_file".into(),
            description: "Write content to a file in the working directory. \
                          Creates the file and any missing parent directories if they don't exist."
                .into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Path to the file, relative to the working directory."
                    },
                    "content": {
                        "type": "string",
                        "description": "Full content to write to the file. Overwrites any existing content."
                    }
                },
                "required": ["path", "content"]
            }),
        }
    }

    fn run(&self, root: &Path, input: ToolCallInput) -> anyhow::Result<String> {
        let input: WriteFileInput = serde_json::from_value(input)?;
        self.execute(root, input)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_new_file() {
        let dir = tempfile::tempdir().unwrap();
        WriteFileTool
            .run(dir.path(), serde_json::json!({ "path": "out.txt", "content": "hello" }))
            .unwrap();
        assert_eq!(std::fs::read_to_string(dir.path().join("out.txt")).unwrap(), "hello");
    }

    #[test]
    fn overwrites_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("out.txt"), "old").unwrap();
        WriteFileTool
            .run(dir.path(), serde_json::json!({ "path": "out.txt", "content": "new" }))
            .unwrap();
        assert_eq!(std::fs::read_to_string(dir.path().join("out.txt")).unwrap(), "new");
    }

    #[test]
    fn creates_missing_parent_dirs() {
        let dir = tempfile::tempdir().unwrap();
        WriteFileTool
            .run(dir.path(), serde_json::json!({ "path": "a/b/c.txt", "content": "deep" }))
            .unwrap();
        assert_eq!(std::fs::read_to_string(dir.path().join("a/b/c.txt")).unwrap(), "deep");
    }

    #[test]
    fn rejects_path_escape() {
        let dir = tempfile::tempdir().unwrap();
        let err = WriteFileTool
            .run(dir.path(), serde_json::json!({ "path": "../escape.txt", "content": "x" }))
            .unwrap_err();
        assert!(err.to_string().contains("escapes"));
    }
}
