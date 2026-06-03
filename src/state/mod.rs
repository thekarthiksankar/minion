mod gather_context;
mod implement;
mod push_branch;

use std::path::{Path, PathBuf};
use std::process::Command;
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
    let branch = ctx.branch().to_string();
    let repo = ctx.working_path().to_path_buf();

    GatherContext.run(&ctx);

    match Implement.run(&ctx, client).await {
        LoopOutcome::Failed(e) => {
            cleanup_branch(&repo, &branch);
            return RunOutcome::Failed(e);
        }
        LoopOutcome::StepLimitExhausted => {
            cleanup_branch(&repo, &branch);
            return RunOutcome::StepLimitExhausted { branch };
        }
        LoopOutcome::Complete => {}
    }

    if let Err(e) = PushBranch.run(&ctx) {
        cleanup_branch(&repo, &branch);
        return RunOutcome::Failed(e);
    }

    RunOutcome::Succeeded { branch }
}

/// Deletes the minion branch after the run context is dropped (i.e. after checkout back to original branch).
fn cleanup_branch(repo: &PathBuf, branch: &str) {
    let _ = Command::new("git")
        .args(["branch", "-D", branch])
        .current_dir(repo)
        .output();
}
