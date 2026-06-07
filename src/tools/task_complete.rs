use std::path::Path;

use crate::llm::ToolSchema;

use super::{Tool, ToolCallInput};

pub struct TaskCompleteTool;

impl Tool for TaskCompleteTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "task_complete".into(),
            description: "Signal that you have finished the task. Call this once all work is \
                          done and committed. Pass a short summary of what was accomplished."
                .into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "summary": {
                        "type": "string",
                        "description": "Short description of what was accomplished."
                    }
                },
                "required": ["summary"]
            }),
        }
    }

    fn summary(&self, input: &ToolCallInput) -> String {
        input
            .get("summary")
            .and_then(|v| v.as_str())
            .unwrap_or("(no summary)")
            .to_string()
    }

    fn run(&self, _root: &Path, _input: ToolCallInput) -> anyhow::Result<String> {
        Ok("Task marked as complete.".into())
    }
}
