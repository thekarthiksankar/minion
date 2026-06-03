mod agent;
mod cli;
mod isolation;
mod llm;
mod state;
mod telemetry;
mod tools;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    cli::run().await
}
