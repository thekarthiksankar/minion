use std::time::Instant;

use crate::llm::{ContentBlock, LlmClient, Message, Role};
use crate::state::RunContext;
use crate::telemetry::Telemetry;
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

    pub async fn run(&self, ctx: &RunContext, telemetry: &Telemetry) -> LoopOutcome {
        let tools = self.dispatcher.schemas();
        let mut messages = vec![opening_message(&ctx.task)];
        let mut turn_num = 0u32;

        loop {
            turn_num += 1;
            let turn_start = Instant::now();
            telemetry.turn_started(turn_num);

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
                telemetry.turn_finished(
                    turn_num,
                    turn_start.elapsed().as_millis() as u64,
                    response.usage.input_tokens,
                    response.usage.output_tokens,
                );
                return LoopOutcome::Complete;
            }

            if turn_num >= MAX_TURNS {
                telemetry.turn_finished(
                    turn_num,
                    turn_start.elapsed().as_millis() as u64,
                    response.usage.input_tokens,
                    response.usage.output_tokens,
                );
                return LoopOutcome::StepLimitExhausted;
            }

            let mut tool_results = Vec::new();
            for block in tool_uses {
                if let ContentBlock::ToolUse { id, name, input } = block {
                    let summary = self.dispatcher.summary(name.as_str(), &input);
                    let tool_start = Instant::now();
                    let result = self
                        .dispatcher
                        .dispatch(name.as_str(), input.clone(), ctx.working_path())
                        .unwrap_or_else(|e| format!("error: {e}"));
                    let tool_duration = tool_start.elapsed().as_millis() as u64;

                    let success = !result.starts_with("error:");
                    let error = if !success {
                        Some(result.trim_start_matches("error: ").to_string())
                    } else {
                        None
                    };

                    telemetry.tool_called(
                        name,
                        &summary,
                        success,
                        tool_duration,
                        error.as_deref(),
                    );

                    tool_results.push(ContentBlock::ToolResult {
                        tool_use_id: id.clone(),
                        content: result,
                    });
                }
            }

            telemetry.turn_finished(
                turn_num,
                turn_start.elapsed().as_millis() as u64,
                response.usage.input_tokens,
                response.usage.output_tokens,
            );

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
