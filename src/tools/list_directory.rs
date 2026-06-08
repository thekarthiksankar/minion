use serde::Deserialize;
use std::path::Path;

use super::path::resolve_readable_path;
use super::{Tool, ToolCallInput};
use crate::llm::ToolSchema;

pub struct ListDirectoryTool;

#[derive(Deserialize)]
struct ListDirectoryInput {
    #[serde(default = "default_path")]
    path: String,
    #[serde(default = "default_depth")]
    depth: u32,
    #[serde(default = "default_max_per_dir")]
    max_per_dir: usize,
}

fn default_path() -> String { ".".into() }
fn default_depth() -> u32 { 2 }
fn default_max_per_dir() -> usize { 5 }

fn render_tree(dir: &Path, prefix: &str, depth_left: u32, max_per_dir: usize, output: &mut String) {
    let read = match std::fs::read_dir(dir) {
        Ok(r) => r,
        Err(_) => return,
    };

    let mut entries: Vec<_> = read.filter_map(|e| e.ok()).collect();
    entries.sort_by(|a, b| {
        let a_dir = a.path().is_dir();
        let b_dir = b.path().is_dir();
        match (a_dir, b_dir) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a.file_name().cmp(&b.file_name()),
        }
    });

    let total = entries.len();
    let will_truncate = total > max_per_dir;
    let shown_count = total.min(max_per_dir);

    for (i, entry) in entries.iter().take(shown_count).enumerate() {
        let is_last = i == shown_count - 1 && !will_truncate;
        let connector = if is_last { "└── " } else { "├── " };
        let child_prefix = if is_last {
            format!("{}    ", prefix)
        } else {
            format!("{}│   ", prefix)
        };
        let name = entry.file_name().to_string_lossy().into_owned();
        let is_dir = entry.path().is_dir();

        if is_dir {
            output.push_str(&format!("{}{}{}/\n", prefix, connector, name));
            if depth_left == 0 {
                output.push_str(&format!("{}... [depth limit reached]\n", child_prefix));
            } else {
                render_tree(&entry.path(), &child_prefix, depth_left - 1, max_per_dir, output);
            }
        } else {
            output.push_str(&format!("{}{}{}\n", prefix, connector, name));
        }
    }

    if will_truncate {
        output.push_str(&format!(
            "{}... [{} more hidden]\n",
            prefix,
            total - shown_count
        ));
    }
}

impl ListDirectoryTool {
    fn execute(&self, root: &Path, input: ListDirectoryInput) -> anyhow::Result<String> {
        let abs = resolve_readable_path(root, &input.path)?;
        if !abs.is_dir() {
            anyhow::bail!("'{}' is not a directory", input.path);
        }

        let display_name = input.path.trim_end_matches('/');
        let mut output = format!("{}/\n", display_name);
        let depth_left = input.depth.saturating_sub(1);
        render_tree(&abs, "", depth_left, input.max_per_dir, &mut output);
        Ok(output)
    }
}

impl Tool for ListDirectoryTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "list_directory".into(),
            description: "Display a directory tree with indented structure. \
                Limits depth and items per directory to keep output compact. \
                Use `path` to target a subdirectory and `depth` to expand deeper."
                .into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Relative path to the directory. Defaults to \".\" (working directory root)."
                    },
                    "depth": {
                        "type": "integer",
                        "description": "How many levels deep to expand. Default: 2."
                    },
                    "max_per_dir": {
                        "type": "integer",
                        "description": "Maximum entries shown per directory before truncating. Default: 5."
                    }
                },
                "required": []
            }),
        }
    }

    fn summary(&self, input: &ToolCallInput) -> String {
        let path = input["path"].as_str().unwrap_or(".");
        let depth = input["depth"].as_u64().unwrap_or(2);
        format!("{path} (depth {depth})")
    }

    fn run(&self, root: &Path, input: ToolCallInput) -> anyhow::Result<String> {
        let input: ListDirectoryInput = serde_json::from_value(input)?;
        self.execute(root, input)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(root: &Path, input: serde_json::Value) -> String {
        ListDirectoryTool.run(root, input).unwrap()
    }

    #[test]
    fn lists_flat_directory() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "").unwrap();
        std::fs::write(dir.path().join("b.txt"), "").unwrap();
        let out = run(dir.path(), serde_json::json!({}));
        assert!(out.contains("a.txt"));
        assert!(out.contains("b.txt"));
    }

    #[test]
    fn dirs_appear_before_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("z.txt"), "").unwrap();
        std::fs::create_dir(dir.path().join("aaa")).unwrap();
        let out = run(dir.path(), serde_json::json!({}));
        let dir_pos = out.find("aaa/").unwrap();
        let file_pos = out.find("z.txt").unwrap();
        assert!(dir_pos < file_pos);
    }

    #[test]
    fn respects_max_per_dir() {
        let dir = tempfile::tempdir().unwrap();
        for i in 0..8 {
            std::fs::write(dir.path().join(format!("f{i}.txt")), "").unwrap();
        }
        let out = run(dir.path(), serde_json::json!({ "max_per_dir": 3 }));
        assert!(out.contains("... [5 more hidden]"));
    }

    #[test]
    fn respects_depth_limit() {
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("sub");
        std::fs::create_dir(&sub).unwrap();
        std::fs::create_dir(sub.join("deep")).unwrap();
        let out = run(dir.path(), serde_json::json!({ "depth": 1 }));
        assert!(out.contains("sub/"));
        assert!(out.contains("... [depth limit reached]"));
        assert!(!out.contains("deep/"));
    }

    #[test]
    fn rejects_path_escape() {
        let dir = tempfile::tempdir().unwrap();
        let err = ListDirectoryTool
            .run(dir.path(), serde_json::json!({ "path": "../" }))
            .unwrap_err();
        assert!(err.to_string().contains("escapes"));
    }

    #[test]
    fn root_header_uses_dot_not_dir_name() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("file.txt"), "").unwrap();
        let out = run(dir.path(), serde_json::json!({}));
        assert!(
            out.starts_with("./\n"),
            "header was: {}",
            out.lines().next().unwrap_or("")
        );
    }

    #[test]
    fn rejects_file_as_directory() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("file.txt"), "x").unwrap();
        let err = ListDirectoryTool
            .run(dir.path(), serde_json::json!({ "path": "file.txt" }))
            .unwrap_err();
        assert!(err.to_string().contains("not a directory"));
    }
}
