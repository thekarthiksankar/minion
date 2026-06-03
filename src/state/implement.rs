use crate::agent::{AgentLoop, LoopOutcome};
use crate::llm::LlmClient;

use super::RunContext;

pub struct Implement;

impl Implement {
    pub async fn run(&self, ctx: &RunContext, client: Box<dyn LlmClient>) -> LoopOutcome {
        println!("  agent is working...");
        let outcome = AgentLoop::new(client).run(ctx).await;
        match &outcome {
            LoopOutcome::Complete => println!("  agent finished"),
            LoopOutcome::StepLimitExhausted => println!("  agent hit step limit"),
            LoopOutcome::Failed(e) => println!("  agent failed: {e}"),
        }
        outcome
    }
}
