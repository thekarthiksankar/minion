use std::path::Path;
use std::process::Command;
use std::time::Instant;

use crate::telemetry::Telemetry;

use super::RunContext;

pub struct PushBranch;

impl PushBranch {
    pub fn run(&self, ctx: &RunContext, telemetry: &Telemetry) -> anyhow::Result<()> {
        let start = Instant::now();
        telemetry.step_started("push");
        telemetry.info("push", &format!("pushing {} to origin", ctx.branch()));

        let output = Command::new("git")
            .args(["push", "origin", ctx.branch()])
            .current_dir(ctx.working_path())
            .output()
            .map_err(|e| anyhow::anyhow!("failed to run git push: {e}"))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("git push failed: {stderr}");
        }

        telemetry.step_finished("push", start.elapsed().as_millis() as u64);
        Ok(())
    }
}
