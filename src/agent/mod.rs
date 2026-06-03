use crate::llm::{ContentBlock, LlmClient, Message, Role};
use crate::state::RunContext;
use crate::tools::Dispatcher;

const MAX_TURNS: u32 = 40;
const MAX_TOKENS: u32 = 8192;

pub enum LoopOutcome {
    Complete,
    StepLimitExhausted,
    Failed(anyhow::Error),
}

pub struct AgentLoop {
    client: Box<dyn LlmClient>,
    dispatcher: Dispatcher,
}

impl AgentLoop {
    pub fn new(client: Box<dyn LlmClient>) -> Self {
        Self {
            client,
            dispatcher: Dispatcher::with_default_tools(),
        }
    }

    pub async fn run(&self, ctx: &RunContext) -> LoopOutcome {
        let tools = self.dispatcher.schemas();
        let mut messages = vec![opening_message(&ctx.task)];
        let mut turns = 0;

        loop {
            let response = match self.client.complete(messages.clone(), tools.clone(), MAX_TOKENS).await {
                Ok(r) => r,
                Err(e) => return LoopOutcome::Failed(e),
            };

            let tool_uses: Vec<&ContentBlock> = response
                .content
                .iter()
                .filter(|b| matches!(b, ContentBlock::ToolUse { .. }))
                .collect();

            messages.push(Message {
                role: Role::Assistant,
                content: response.content.clone(),
            });

            if tool_uses.is_empty() {
                return LoopOutcome::Complete;
            }

            turns += 1;
            if turns >= MAX_TURNS {
                return LoopOutcome::StepLimitExhausted;
            }

            let mut tool_results = Vec::new();
            for block in tool_uses {
                if let ContentBlock::ToolUse { id, name, input } = block {
                    print!("    → {name} ... ");
                    let result = self
                        .dispatcher
                        .dispatch(name.as_str(), input.clone(), ctx.working_path())
                        .unwrap_or_else(|e| format!("error: {e}"));

                    if result.starts_with("error:") {
                        println!("failed: {result}");
                    } else {
                        println!("ok");
                    }

                    tool_results.push(ContentBlock::ToolResult {
                        tool_use_id: id.clone(),
                        content: result,
                    });
                }
            }

            messages.push(Message {
                role: Role::User,
                content: tool_results,
            });
        }
    }
}

fn opening_message(task: &str) -> Message {
    Message {
        role: Role::User,
        content: vec![ContentBlock::Text {
            text: format!(
                "You are an autonomous coding agent. Complete the following task by using the \
                 tools available to you. When you are done, stop calling tools.\n\nTask: {task}"
            ),
        }],
    }
}
