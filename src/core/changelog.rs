use std::path::Path;

/// Insert an empty `## [X.Y.Z] - YYYY-MM-DD` section at the top of CHANGELOG.md.
///
/// If the file does not exist, this is a no-op and returns `false`.
/// Returns `true` when the file was modified.
pub fn insert_template_section(
    changelog_path: &Path,
    version: &str,
    date: &str,
) -> anyhow::Result<bool> {
    if !changelog_path.exists() {
        return Ok(false);
    }

    let existing = std::fs::read_to_string(changelog_path)
        .map_err(|e| anyhow::anyhow!("failed to read {}: {e}", changelog_path.display()))?;

    let header = format!("## [{version}] - {date}\n\n");

    // Avoid inserting a duplicate if the header already exists
    if existing.contains(&format!("## [{version}]")) {
        return Ok(false);
    }

    let new_content = format!("{header}{existing}");

    std::fs::write(changelog_path, new_content)
        .map_err(|e| anyhow::anyhow!("failed to write {}: {e}", changelog_path.display()))?;

    Ok(true)
}
