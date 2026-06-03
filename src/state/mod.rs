mod gather_context;
mod implement;
mod push_branch;

use std::path::Path;
use uuid::Uuid;

use crate::agent::LoopOutcome;
use crate::isolation::{InPlaceBranchIsolation, Isolation};
use crate::isolation::in_place::find_repo_root;
use crate::llm::LlmClient;

use gather_context::GatherContext;
use implement::Implement;
use push_branch::PushBranch;

pub struct RunContext {
    pub run_id: String,
    pub task: String,
    isolation: Box<dyn Isolation>,
}

impl RunContext {
    pub fn new(task: String, repo: &Path) -> anyhow::Result<Self> {
        let run_id = Uuid::new_v4().to_string();
        let repo_root = find_repo_root(repo)?;
        let isolation = InPlaceBranchIsolation::create(&repo_root, &run_id, &task)?;

        Ok(Self {
            run_id,
            task,
            isolation: Box::new(isolation),
        })
    }

    pub fn working_path(&self) -> &Path {
        self.isolation.working_path()
    }

    pub fn branch(&self) -> &str {
        self.isolation.branch()
    }
}

pub enum RunOutcome {
    Succeeded { branch: String },
    StepLimitExhausted { branch: String },
    Failed(anyhow::Error),
}

pub async fn run_state_machine(ctx: RunContext, client: Box<dyn LlmClient>) -> RunOutcome {
    GatherContext.run(&ctx);

    match Implement.run(&ctx, client).await {
        LoopOutcome::Failed(e) => return RunOutcome::Failed(e),
        LoopOutcome::StepLimitExhausted => {
            tracing::warn!(run_id = %ctx.run_id, "step limit exhausted");
            return RunOutcome::StepLimitExhausted { branch: ctx.branch().to_string() };
        }
        LoopOutcome::Complete => {}
    }

    if let Err(e) = PushBranch.run(&ctx) {
        return RunOutcome::Failed(e);
    }

    RunOutcome::Succeeded { branch: ctx.branch().to_string() }
}
