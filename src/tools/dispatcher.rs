use std::collections::HashMap;
use std::path::Path;

use crate::llm::ToolSchema;

use super::{Tool, ToolCallInput};

pub struct Dispatcher {
    tools: HashMap<String, Box<dyn Tool>>,
}

impl Dispatcher {
    pub fn new() -> Self {
        Self { tools: HashMap::new() }
    }

    pub fn with_default_tools() -> Self {
        let mut d = Self::new();
        d.register(Box::new(super::read_file::ReadFileTool));
        d.register(Box::new(super::write_file::WriteFileTool));
        d.register(Box::new(super::rename_file::RenameFileTool));
        d.register(Box::new(super::run_command::RunCommandTool));
        d.register(Box::new(super::git_add::GitAddTool));
        d.register(Box::new(super::git_commit::GitCommitTool));
        d.register(Box::new(super::task_complete::TaskCompleteTool));
        d
    }

    pub fn register(&mut self, tool: Box<dyn Tool>) {
        let name = tool.schema().name.clone();
        self.tools.insert(name, tool);
    }

    pub fn schemas(&self) -> Vec<ToolSchema> {
        self.tools.values().map(|t| t.schema()).collect()
    }

    pub fn dispatch(&self, name: &str, input: ToolCallInput, root: &Path) -> anyhow::Result<String> {
        match self.tools.get(name) {
            Some(tool) => tool.run(root, input),
            None => anyhow::bail!("unknown tool: {name}"),
        }
    }

    pub fn summary(&self, name: &str, input: &ToolCallInput) -> String {
        match self.tools.get(name) {
            Some(tool) => tool.summary(input),
            None => String::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::read_file::ReadFileTool;

    #[test]
    fn dispatches_read_file() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("hello.txt"), "hello").unwrap();

        let mut dispatcher = Dispatcher::new();
        dispatcher.register(Box::new(ReadFileTool));

        let result = dispatcher
            .dispatch("read_file", serde_json::json!({ "path": "hello.txt" }), dir.path())
            .unwrap();

        assert_eq!(result, "hello");
    }

    #[test]
    fn returns_error_for_unknown_tool() {
        let dispatcher = Dispatcher::new();
        let err = dispatcher
            .dispatch("nonexistent", serde_json::json!({}), Path::new("."))
            .unwrap_err();
        assert!(err.to_string().contains("unknown tool"));
    }

    #[test]
    fn schemas_includes_registered_tools() {
        let mut dispatcher = Dispatcher::new();
        dispatcher.register(Box::new(ReadFileTool));

        let schemas = dispatcher.schemas();
        assert!(schemas.iter().any(|s| s.name == "read_file"));
    }

    #[test]
    fn default_tools_includes_read_file() {
        let dispatcher = Dispatcher::with_default_tools();
        assert!(dispatcher.schemas().iter().any(|s| s.name == "read_file"));
    }
}
