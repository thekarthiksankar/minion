mod gather_context;
mod implement;
mod push_branch;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use uuid::Uuid;

use crate::agent::LoopOutcome;
use crate::isolation::{InPlaceBranchIsolation, Isolation};
use crate::isolation::in_place::find_repo_root;
use crate::llm::LlmClient;
use crate::telemetry::{Telemetry, RunLogBackend};

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
        let run_id = Uuid::now_v7().to_string();
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
    Abandoned { branch: String },
    Failed { branch: Option<String>, error: anyhow::Error },
}

pub async fn run_state_machine(ctx: RunContext, client: Box<dyn LlmClient>) -> RunOutcome {
    let branch = ctx.branch().to_string();
    let repo = ctx.working_path().to_path_buf();

    let run_dir = repo.join(".minion").join("runs").join(&ctx.run_id);
    let backend = match RunLogBackend::new(
        &ctx.run_id,
        &ctx.task,
        &branch,
        &repo.display().to_string(),
        run_dir,
    ) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("warning: could not initialise telemetry: {e}");
            cleanup_branch(&repo, &branch);
            return RunOutcome::Failed { branch: Some(branch), error: e };
        }
    };
    let telemetry = Arc::new(Telemetry::new(Box::new(backend)));

    telemetry.run_phase(1, 3, "gathering context");
    GatherContext.run(&ctx, &telemetry);

    telemetry.run_phase(2, 3, "implementing");
    match Implement.run(&ctx, client, Arc::clone(&telemetry)).await {
        LoopOutcome::Failed(e) => {
            let _ = telemetry.finish("failed", Some(&e.to_string()));
            cleanup_branch(&repo, &branch);
            return RunOutcome::Failed { branch: Some(branch), error: e };
        }
        LoopOutcome::StepLimitExhausted => {
            let _ = telemetry.finish("step_limit_exhausted", None);
            cleanup_branch(&repo, &branch);
            return RunOutcome::StepLimitExhausted { branch };
        }
        LoopOutcome::Abandoned => {
            let _ = telemetry.finish("task_abandoned", None);
            cleanup_branch(&repo, &branch);
            return RunOutcome::Abandoned { branch };
        }
        LoopOutcome::Complete => {}
    }

    telemetry.run_phase(3, 3, "pushing branch");
    if let Err(e) = PushBranch.run(&ctx, &telemetry) {
        let _ = telemetry.finish("failed", Some(&e.to_string()));
        cleanup_branch(&repo, &branch);
        return RunOutcome::Failed { branch: Some(branch), error: e };
    }

    let _ = telemetry.finish("succeeded", None);
    RunOutcome::Succeeded { branch }
}

fn cleanup_branch(repo: &PathBuf, branch: &str) {
    let _ = Command::new("git")
        .args(["branch", "-D", branch])
        .current_dir(repo)
        .output();
}
