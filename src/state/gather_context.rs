use std::sync::Arc;
use crate::telemetry::Telemetry;
use super::RunContext;

pub struct GatherContext;

impl GatherContext {
    pub fn run(&self, ctx: &RunContext, telemetry: &Arc<Telemetry>) -> String {
        let start = std::time::Instant::now();
        telemetry.step_started("context");
        telemetry.info("context", &format!("task    : {}", ctx.task));
        telemetry.info("context", &format!("branch  : {}", ctx.branch()));
        telemetry.info("context", &format!("run id  : {}", ctx.run_id));
        telemetry.step_finished("context", start.elapsed().as_millis() as u64);
        ctx.task.clone()
    }
}
