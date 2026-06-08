mod dispatcher;
mod git_add;
mod git_commit;
mod list_directory;
mod path;
mod read_file;
mod rename_file;
mod run_command;
mod task_complete;
mod write_file;

pub use dispatcher::Dispatcher;

use std::path::Path;
use crate::llm::ToolSchema;

/// Raw JSON object received from an LLM tool call.
pub type ToolCallInput = serde_json::Value;

pub trait Tool: Send + Sync {
    fn schema(&self) -> ToolSchema;
    fn summary(&self, input: &ToolCallInput) -> String;
    fn run(&self, root: &Path, input: ToolCallInput) -> anyhow::Result<String>;
}
