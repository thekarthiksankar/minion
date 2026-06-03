use anyhow::{Context, bail};
use std::path::{Path, PathBuf};
use std::process::Command;

use super::Isolation;

/// Isolates a run by creating a new branch in the target repo's working directory.
/// The developer's original branch is restored on Drop.
pub struct InPlaceBranchIsolation {
    repo_root: PathBuf,
    branch: String,
    original_branch: String,
}

impl InPlaceBranchIsolation {
    pub fn create(repo_root: &Path, run_id: &str, task: &str) -> anyhow::Result<Self> {
        ensure_git_installed()?;
        ensure_git_initialised(repo_root)?;
        let original_branch = current_branch(repo_root)?;
        ensure_git_identity(repo_root)?;
        let branch = format!("minion/{}/{}", &run_id[..8], make_task_slug(task));

        git(repo_root, &["checkout", "-b", &branch])
            .with_context(|| format!("failed to create branch {branch}"))?;

        Ok(Self {
            repo_root: repo_root.to_path_buf(),
            branch,
            original_branch,
        })
    }
}

impl Isolation for InPlaceBranchIsolation {
    fn working_path(&self) -> &Path {
        &self.repo_root
    }

    fn branch(&self) -> &str {
        &self.branch
    }
}

impl Drop for InPlaceBranchIsolation {
    fn drop(&mut self) {
        let _ = git(&self.repo_root, &["checkout", &self.original_branch]);
    }
}

pub fn find_repo_root(start: &Path) -> anyhow::Result<PathBuf> {
    let mut current = start.canonicalize()?;
    loop {
        if current.join(".git").exists() {
            return Ok(current);
        }
        match current.parent() {
            Some(parent) => current = parent.to_path_buf(),
            None => bail!("no git repository found from {}", start.display()),
        }
    }
}

fn current_branch(repo_root: &Path) -> anyhow::Result<String> {
    git_output(repo_root, &["rev-parse", "--abbrev-ref", "HEAD"])
}

/// Checks that the git binary is installed and on PATH before the run starts.
fn ensure_git_installed() -> anyhow::Result<()> {
    let output = Command::new("git").arg("--version").output();
    match output {
        Ok(o) if o.status.success() => Ok(()),
        _ => bail!("git is not installed or not on PATH — install git and retry"),
    }
}

/// Checks that the target directory is inside an initialised git repository.
fn ensure_git_initialised(repo_root: &Path) -> anyhow::Result<()> {
    let output = Command::new("git")
        .args(["rev-parse", "--is-inside-work-tree"])
        .current_dir(repo_root)
        .output();
    match output {
        Ok(o) if o.status.success() => Ok(()),
        _ => bail!("'{}' is not inside a git repository — run 'git init' first", repo_root.display()),
    }
}

/// Checks that git user.name and user.email are configured before the run starts.
/// Fails with an actionable message so the user can fix it before retrying.
fn ensure_git_identity(repo_root: &Path) -> anyhow::Result<()> {
    let name = git_output(repo_root, &["config", "user.name"]).unwrap_or_default();
    let email = git_output(repo_root, &["config", "user.email"]).unwrap_or_default();
    if name.is_empty() || email.is_empty() {
        bail!(
            "git identity is not configured. Run:\n  \
            git config --global user.name \"Your Name\"\n  \
            git config --global user.email \"you@example.com\""
        );
    }
    Ok(())
}

/// Runs a git command, returns stdout as a trimmed string.
fn git_output(repo_root: &Path, args: &[&str]) -> anyhow::Result<String> {
    let out = Command::new("git")
        .args(args)
        .current_dir(repo_root)
        .output()
        .context("failed to spawn git")?;

    if !out.status.success() {
        bail!("git {} failed: {}", args.join(" "), String::from_utf8_lossy(&out.stderr));
    }

    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Runs a git command, checks exit status only. Captures output to prevent git printing to the terminal.
fn git(repo_root: &Path, args: &[&str]) -> anyhow::Result<()> {
    let out = Command::new("git")
        .args(args)
        .current_dir(repo_root)
        .output()
        .context("failed to spawn git")?;

    if !out.status.success() {
        bail!("git {} failed: {}", args.join(" "), String::from_utf8_lossy(&out.stderr));
    }
    Ok(())
}

fn make_task_slug(task: &str) -> String {
    task.split_whitespace()
        .take(5)
        .map(|w| {
            w.chars()
                .filter(|c| c.is_alphanumeric())
                .collect::<String>()
                .to_lowercase()
        })
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}
