mod cli;
mod isolation;
mod llm;
mod state;
mod tools;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    cli::run().await
}
