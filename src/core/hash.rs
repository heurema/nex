use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;

/// Compute SHA-256 over package files using length-prefixed encoding.
/// Format per file: path_len (u64 LE) || path_bytes || content_len (u64 LE) || content_bytes
/// Excludes .git/ and .signum/ directories. Matches publish's compute_sha256_git_tree.
/// Rejects packages containing symlinks (security).
pub fn compute_sha256(dir: &Path) -> anyhow::Result<String> {
    let mut hasher = Sha256::new();
    let mut entries: Vec<_> = walkdir(dir)?
        .into_iter()
        .filter(|p| !p.components().any(|c| {
            let s = c.as_os_str();
            s == ".git" || s == ".signum"
        }))
        .collect();
    entries.sort();

    for path in entries {
        let meta = path.symlink_metadata()?;
        if meta.file_type().is_symlink() {
            anyhow::bail!("Package contains symlink: {} — aborting for security", path.display());
        }
        if meta.is_file() {
            let relative = path.strip_prefix(dir)?;
            let path_bytes = relative.to_string_lossy();
            let path_bytes = path_bytes.as_bytes();
            let content = fs::read(&path)?;
            hasher.update((path_bytes.len() as u64).to_le_bytes());
            hasher.update(path_bytes);
            hasher.update((content.len() as u64).to_le_bytes());
            hasher.update(&content);
        }
    }

    Ok(format!("{:x}", hasher.finalize()))
}

fn walkdir(dir: &Path) -> anyhow::Result<Vec<std::path::PathBuf>> {
    let mut result = Vec::new();
    if dir.is_dir() {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            let meta = path.symlink_metadata()?;
            if meta.file_type().is_symlink() {
                result.push(path);
            } else if meta.is_dir() {
                result.extend(walkdir(&path)?);
            } else {
                result.push(path);
            }
        }
    }
    Ok(result)
}
