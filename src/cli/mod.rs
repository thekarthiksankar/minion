use std::path::PathBuf;
use clap::{Parser, Subcommand};

use crate::llm;
use crate::state::{run_state_machine, RunContext, RunOutcome};

#[derive(Parser)]
#[command(name = "minion", about = "One shot coding agent")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Start a run with an inline task description
    Run {
        /// Task description
        #[arg(conflicts_with = "file")]
        task: Option<String>,

        /// Read task from a file
        #[arg(short, long, name = "file")]
        file: Option<String>,

        /// Path to the target git repository (defaults to current directory)
        #[arg(short, long, name = "repo", default_value = ".")]
        repo: PathBuf,
    },
}

pub async fn run() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Run { task, file, repo } => {
            let client = llm::default_client()?;
            let task_input = resolve_task(task, file).await?;
            let ctx = RunContext::new(task_input, &repo)?;

            match run_state_machine(ctx, client).await {
                RunOutcome::Succeeded { branch } => {
                    println!("\ndone — open a PR from branch: {branch}");
                }
                RunOutcome::StepLimitExhausted { branch: _ } => {
                    println!("\nstopped — step limit reached, branch cleaned up");
                }
                RunOutcome::Failed(e) => {
                    eprintln!("\nfailed: {e}");
                    std::process::exit(1);
                }
            }
            Ok(())
        }
    }
}

async fn resolve_task(task: Option<String>, file: Option<String>) -> anyhow::Result<String> {
    match (task, file) {
        (Some(t), _) => Ok(t),
        (_, Some(path)) => {
            let content = std::fs::read_to_string(&path)?;
            Ok(content.trim().to_string())
        }
        (None, None) => anyhow::bail!("Provide a task description or use -f <file>"),
    }
}
