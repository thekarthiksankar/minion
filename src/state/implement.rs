use crate::agent::{AgentLoop, LoopOutcome};
use crate::llm::LlmClient;

use super::RunContext;

pub struct Implement;

impl Implement {
    pub async fn run(&self, ctx: &RunContext, client: Box<dyn LlmClient>) -> LoopOutcome {
        tracing::info!(run_id = %ctx.run_id, "implement");
        AgentLoop::new(client).run(ctx).await
    }
}
