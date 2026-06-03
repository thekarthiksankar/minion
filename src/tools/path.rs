use anyhow::{bail, Context};
use std::path::{Path, PathBuf};

pub(super) fn resolve_readable_path(root: &Path, rel: &str) -> anyhow::Result<PathBuf> {
    let canonical_root = canonical_root(root)?;
    let candidate = resolve_existing_candidate(&canonical_root, rel)?;
    check_within_root(&canonical_root, &candidate, rel)?;
    Ok(candidate)
}

pub(super) fn resolve_writable_path(root: &Path, rel: &str) -> anyhow::Result<PathBuf> {
    let canonical_root = canonical_root(root)?;
    let candidate = resolve_new_candidate(&canonical_root, rel);
    check_within_root(&canonical_root, &candidate, rel)?;
    Ok(candidate)
}

/// Resolves the working directory to its real absolute path on disk.
fn canonical_root(root: &Path) -> anyhow::Result<PathBuf> {
    root.canonicalize()
        .with_context(|| format!("working directory '{}' not found", root.display()))
}

/// Builds the full path and confirms the file exists on disk.
fn resolve_existing_candidate(canonical_root: &Path, rel: &str) -> anyhow::Result<PathBuf> {
    let candidate = canonical_root.join(rel);
    candidate
        .canonicalize()
        .map_err(|e| anyhow::anyhow!("path '{}' not found: {e}", candidate.display()))
}

/// Builds the full path without touching the filesystem — the file doesn't need to exist yet.
fn resolve_new_candidate(canonical_root: &Path, rel: &str) -> PathBuf {
    let candidate = canonical_root.join(rel);
    let mut out = PathBuf::new();
    for component in candidate.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => { out.pop(); }
            c => out.push(c),
        }
    }
    out
}

/// Checks the path is inside the working directory and not pointing somewhere outside it.
fn check_within_root(canonical_root: &Path, candidate: &Path, rel: &str) -> anyhow::Result<()> {
    if !candidate.starts_with(canonical_root) {
        bail!("path '{}' escapes the working directory", rel);
    }
    Ok(())
}
