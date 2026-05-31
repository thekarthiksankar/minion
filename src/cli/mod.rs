use std::path::PathBuf;
use clap::{Parser, Subcommand};

use crate::state::RunContext;

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
            let task_input = resolve_task(task, file).await?;
            let ctx = RunContext::new(task_input, &repo)?;
            tracing::info!(run_id = %ctx.run_id, branch = %ctx.branch(), "run started");
            println!("run {} on branch {}", ctx.run_id, ctx.branch());
            // TODO: hand off to state machine — task #8
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
