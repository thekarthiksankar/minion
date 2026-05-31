use clap::{Parser, Subcommand};

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
    },
}

pub async fn run() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Run { task, file } => {
            let task_input = resolve_task(task, file).await?;
            tracing::info!("Starting run: {}", task_input);
            println!("Starting run: {}", task_input);
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
