mod dispatcher;
mod path;
mod read_file;
mod write_file;

pub use dispatcher::Dispatcher;

use std::path::Path;
use crate::llm::ToolSchema;

/// Raw JSON object received from an LLM tool call.
pub type ToolCallInput = serde_json::Value;

pub trait Tool: Send + Sync {
    fn schema(&self) -> ToolSchema;
    fn run(&self, root: &Path, input: ToolCallInput) -> anyhow::Result<String>;
}
