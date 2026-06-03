use anyhow::Context;
use serde::Deserialize;
use std::path::Path;

use super::path::resolve_readable_path;
use super::{Tool, ToolCallInput};
use crate::llm::ToolSchema;

pub struct ReadFileTool;

#[derive(Deserialize)]
struct ReadFileInput {
    path: String,
    start_line: Option<usize>,
    end_line: Option<usize>,
}

impl ReadFileTool {
    fn execute(&self, root: &Path, input: ReadFileInput) -> anyhow::Result<String> {
        let abs = resolve_readable_path(root, &input.path)?;
        let content =
            std::fs::read_to_string(&abs).with_context(|| format!("read '{}'", abs.display()))?;

        match (input.start_line, input.end_line) {
            (None, None) => Ok(content),
            (start, end) => {
                let start = start.unwrap_or(1).saturating_sub(1); // 1-indexed → 0-indexed
                let lines: Vec<&str> = content.lines().collect();
                let end = end.unwrap_or(lines.len()).min(lines.len());
                if start >= end {
                    return Ok(String::new());
                }
                Ok(lines[start..end].join("\n"))
            }
        }
    }
}

impl Tool for ReadFileTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "read_file".into(),
            description: "Read the contents of a file in the working directory. \
                          Optionally restrict to a line range (1-indexed, inclusive)."
                .into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Path to the file, relative to the working directory."
                    },
                    "start_line": {
                        "type": "integer",
                        "description": "Optional. First line to return (1-indexed). Defaults to 1."
                    },
                    "end_line": {
                        "type": "integer",
                        "description": "Optional. Last line to return (1-indexed, inclusive). Defaults to end of file."
                    }
                },
                "required": ["path"]
            }),
        }
    }

    fn run(&self, root: &Path, input: ToolCallInput) -> anyhow::Result<String> {
        let input: ReadFileInput = serde_json::from_value(input)?;
        self.execute(root, input)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root_and_file(content: &str) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("file.txt");
        std::fs::write(&path, content).unwrap();
        (dir, path)
    }

    #[test]
    fn reads_whole_file() {
        let (dir, _) = root_and_file("line1\nline2\nline3\n");
        let out = ReadFileTool
            .run(dir.path(), serde_json::json!({ "path": "file.txt" }))
            .unwrap();
        assert_eq!(out, "line1\nline2\nline3\n");
    }

    #[test]
    fn reads_line_range() {
        let (dir, _) = root_and_file("a\nb\nc\nd\n");
        let out = ReadFileTool
            .run(dir.path(), serde_json::json!({ "path": "file.txt", "start_line": 2, "end_line": 3 }))
            .unwrap();
        assert_eq!(out, "b\nc");
    }

    #[test]
    fn rejects_path_escape() {
        let dir = tempfile::tempdir().unwrap();
        let outer = tempfile::NamedTempFile::new().unwrap();
        let rel = format!("../{}", outer.path().file_name().unwrap().to_str().unwrap());
        let err = ReadFileTool
            .run(dir.path(), serde_json::json!({ "path": rel }))
            .unwrap_err();
        assert!(err.to_string().contains("escapes"));
    }
}
