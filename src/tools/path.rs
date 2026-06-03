use anyhow::{bail, Context};
use std::path::{Path, PathBuf};

/// Resolves `rel` relative to `root` and verifies the result stays inside `root`.
pub(super) fn resolve_path(root: &Path, rel: &str) -> anyhow::Result<PathBuf> {
    let canonical_root = root
        .canonicalize()
        .with_context(|| format!("working directory '{}' not found", root.display()))?;

    let candidate = canonical_root.join(rel);
    let canonical = candidate
        .canonicalize()
        .map_err(|e| anyhow::anyhow!("path '{}' not found: {e}", candidate.display()))?;

    if !canonical.starts_with(&canonical_root) {
        bail!("path '{}' escapes the working directory", rel);
    }
    Ok(canonical)
}
