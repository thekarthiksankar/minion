use super::RunContext;

pub struct GatherContext;

impl GatherContext {
    pub fn run(&self, ctx: &RunContext) -> String {
        println!("  task    : {}", ctx.task);
        println!("  branch  : {}", ctx.branch());
        println!("  run id  : {}", ctx.run_id);
        ctx.task.clone()
    }
}
