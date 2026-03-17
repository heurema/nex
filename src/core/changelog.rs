use std::path::Path;
use std::process::Command;

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

/// Insert a `## [X.Y.Z] - YYYY-MM-DD` section with auto-generated content
/// from `git log --oneline <prev_tag>..HEAD`.
///
/// If the file does not exist, this is a no-op and returns `false`.
/// Returns `true` when the file was modified.
pub fn insert_auto_section(
    changelog_path: &Path,
    plugin_root: &Path,
    version: &str,
    date: &str,
) -> anyhow::Result<bool> {
    if !changelog_path.exists() {
        return Ok(false);
    }

    let existing = std::fs::read_to_string(changelog_path)
        .map_err(|e| anyhow::anyhow!("failed to read {}: {e}", changelog_path.display()))?;

    if existing.contains(&format!("## [{version}]")) {
        return Ok(false);
    }

    // Find previous tag for git log range
    let prev_tag = find_previous_tag(plugin_root);
    let commits = collect_commits(plugin_root, prev_tag.as_deref());

    let mut section = format!("## [{version}] - {date}\n\n");
    if commits.is_empty() {
        section.push_str("- Initial release\n");
    } else {
        for commit in &commits {
            section.push_str(&format!("- {commit}\n"));
        }
    }
    section.push('\n');

    let new_content = format!("{section}{existing}");

    std::fs::write(changelog_path, new_content)
        .map_err(|e| anyhow::anyhow!("failed to write {}: {e}", changelog_path.display()))?;

    Ok(true)
}

/// Find the most recent tag in the repo (used as range start for git log).
fn find_previous_tag(plugin_root: &Path) -> Option<String> {
    let output = Command::new("git")
        .args(["describe", "--tags", "--abbrev=0", "HEAD"])
        .current_dir(plugin_root)
        .output()
        .ok()?;
    if output.status.success() {
        Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        None
    }
}

/// Collect commit messages from `prev_tag..HEAD` (or all commits if no tag).
fn collect_commits(plugin_root: &Path, prev_tag: Option<&str>) -> Vec<String> {
    let range = match prev_tag {
        Some(tag) => format!("{tag}..HEAD"),
        None => "HEAD".to_string(),
    };

    let mut args = vec!["log", "--oneline", "--no-decorate"];
    args.push(&range);

    let output = Command::new("git")
        .args(&args)
        .current_dir(plugin_root)
        .output();

    let Ok(output) = output else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }

    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|line| {
            // Strip short SHA prefix: "abc1234 feat: ..." → "feat: ..."
            if let Some(idx) = line.find(' ') {
                line[idx + 1..].to_string()
            } else {
                line.to_string()
            }
        })
        .filter(|s| !s.is_empty())
        // Skip changelog/version bump commits (noise)
        .filter(|s| !s.starts_with("chore: bump version") && !s.starts_with("chore: release"))
        .collect()
}
