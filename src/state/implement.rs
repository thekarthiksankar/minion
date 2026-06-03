use std::sync::Arc;
use std::time::Instant;

use crate::agent::{AgentLoop, LoopOutcome};
use crate::llm::LlmClient;
use crate::telemetry::Telemetry;

use super::RunContext;

pub struct Implement;

impl Implement {
    pub async fn run(&self, ctx: &RunContext, client: Box<dyn LlmClient>, telemetry: Arc<Telemetry>) -> LoopOutcome {
        let start = Instant::now();
        telemetry.step_started("implement");
        let outcome = AgentLoop::new(client).run(ctx, &telemetry).await;
        telemetry.step_finished("implement", start.elapsed().as_millis() as u64);
        outcome
    }
}
