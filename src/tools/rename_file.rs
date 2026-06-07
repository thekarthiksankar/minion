use anyhow::Context;
use serde::Deserialize;
use std::path::Path;

use super::path::{resolve_readable_path, resolve_writable_path};
use super::{Tool, ToolCallInput};
use crate::llm::ToolSchema;

pub struct RenameFileTool;

#[derive(Deserialize)]
struct RenameFileInput {
    source: String,
    destination: String,
}

impl RenameFileTool {
    fn execute(&self, root: &Path, input: RenameFileInput) -> anyhow::Result<String> {
        let src = resolve_readable_path(root, &input.source)?;
        let dst = resolve_writable_path(root, &input.destination)?;
        if let Some(parent) = dst.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("create dirs for '{}'", dst.display()))?;
        }
        std::fs::rename(&src, &dst)
            .with_context(|| format!("rename '{}' to '{}'", src.display(), dst.display()))?;
        Ok(format!("renamed {} to {}", input.source, input.destination))
    }
}

impl Tool for RenameFileTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "rename_file".into(),
            description: "Rename or move a file within the working directory. \
                          Creates any missing parent directories for the destination. \
                          Returns a confirmation with the source and destination paths."
                .into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "source": {
                        "type": "string",
                        "description": "Current path of the file, relative to the working directory."
                    },
                    "destination": {
                        "type": "string",
                        "description": "New path for the file, relative to the working directory."
                    }
                },
                "required": ["source", "destination"]
            }),
        }
    }

    fn summary(&self, input: &ToolCallInput) -> String {
        let src = input["source"].as_str().unwrap_or("?");
        let dst = input["destination"].as_str().unwrap_or("?");
        format!("{src} -> {dst}")
    }

    fn run(&self, root: &Path, input: ToolCallInput) -> anyhow::Result<String> {
        let input: RenameFileInput = serde_json::from_value(input)?;
        self.execute(root, input)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renames_file() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("old.txt"), "content").unwrap();
        RenameFileTool
            .run(dir.path(), serde_json::json!({ "source": "old.txt", "destination": "new.txt" }))
            .unwrap();
        assert!(!dir.path().join("old.txt").exists());
        assert_eq!(std::fs::read_to_string(dir.path().join("new.txt")).unwrap(), "content");
    }

    #[test]
    fn creates_missing_parent_dirs() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("flat.txt"), "data").unwrap();
        RenameFileTool
            .run(dir.path(), serde_json::json!({ "source": "flat.txt", "destination": "sub/dir/flat.txt" }))
            .unwrap();
        assert_eq!(std::fs::read_to_string(dir.path().join("sub/dir/flat.txt")).unwrap(), "data");
    }

    #[test]
    fn rejects_source_escape() {
        let dir = tempfile::tempdir().unwrap();
        let err = RenameFileTool
            .run(dir.path(), serde_json::json!({ "source": "../outside.txt", "destination": "inside.txt" }))
            .unwrap_err();
        assert!(err.to_string().contains("escapes") || err.to_string().contains("not found"));
    }

    #[test]
    fn rejects_destination_escape() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("file.txt"), "x").unwrap();
        let err = RenameFileTool
            .run(dir.path(), serde_json::json!({ "source": "file.txt", "destination": "../outside.txt" }))
            .unwrap_err();
        assert!(err.to_string().contains("escapes"));
    }
}
