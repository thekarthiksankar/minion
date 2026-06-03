use std::path::Path;
use std::process::Command;

use super::RunContext;

pub struct PushBranch;

impl PushBranch {
    pub fn run(&self, ctx: &RunContext) -> anyhow::Result<()> {
        println!("  pushing {} to origin...", ctx.branch());

        let output = Command::new("git")
            .args(["push", "origin", ctx.branch()])
            .current_dir(ctx.working_path())
            .output()
            .map_err(|e| anyhow::anyhow!("failed to run git push: {e}"))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("git push failed: {stderr}");
        }

        println!("  pushed");
        Ok(())
    }
}
