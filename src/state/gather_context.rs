use super::RunContext;

pub struct GatherContext;

impl GatherContext {
    pub fn run(&self, ctx: &RunContext) -> String {
        tracing::info!(run_id = %ctx.run_id, task = %ctx.task, "gather context");
        ctx.task.clone()
    }
}
