use std::path::Path;

/// Update version references in README.md.
/// Replaces version references in common README patterns, including badges.
/// Returns true if file was modified.
pub fn sync_readme_version(
    plugin_root: &Path,
    old_version: &str,
    new_version: &str,
) -> anyhow::Result<bool> {
    let readme_path = plugin_root.join("README.md");
    if !readme_path.exists() {
        return Ok(false);
    }

    let content = std::fs::read_to_string(&readme_path)?;

    let updated = content
        .replace(&format!("v{old_version}"), &format!("v{new_version}"))
        .replace(
            &format!("`{old_version}`"),
            &format!("`{new_version}`"),
        )
        .replace(
            &format!("version-{old_version}-"),
            &format!("version-{new_version}-"),
        );

    if updated == content {
        return Ok(false);
    }

    std::fs::write(&readme_path, updated)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::sync_readme_version;

    #[test]
    fn sync_readme_updates_badges_and_version_refs() {
        let temp = tempfile::tempdir().expect("temp dir");
        let readme = temp.path().join("README.md");
        std::fs::write(
            &readme,
            concat!(
                "![Version](https://img.shields.io/badge/version-0.11.0-5b21b6)\n",
                "Current tag is v0.11.0 and CLI reports `0.11.0`.\n"
            ),
        )
        .expect("write README");

        let changed =
            sync_readme_version(temp.path(), "0.11.0", "0.12.0").expect("sync readme version");
        let updated = std::fs::read_to_string(&readme).expect("read README");

        assert!(changed);
        assert!(updated.contains("version-0.12.0-5b21b6"));
        assert!(updated.contains("v0.12.0"));
        assert!(updated.contains("`0.12.0`"));
    }
}

/// Sync description from plugin.json into SKILL.md frontmatter.
/// Looks for `description: ...` in YAML frontmatter and replaces the value.
/// Returns true if any file was modified.
pub fn sync_skill_descriptions(plugin_root: &Path) -> anyhow::Result<bool> {
    // Read description from plugin.json
    let plugin_json_path = plugin_root.join(".claude-plugin").join("plugin.json");
    if !plugin_json_path.exists() {
        return Ok(false);
    }

    let plugin_json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&plugin_json_path)?)?;
    let description = match plugin_json.get("description").and_then(|v| v.as_str()) {
        Some(d) => d,
        None => return Ok(false),
    };

    // Find all SKILL.md files in skills/ directory
    let skills_dir = plugin_root.join("skills");
    if !skills_dir.exists() {
        return Ok(false);
    }

    let mut modified = false;
    for entry in walkdir(&skills_dir)? {
        let path = entry;
        if path.file_name().and_then(|n| n.to_str()) != Some("SKILL.md") {
            continue;
        }
        if update_skill_description(&path, description)? {
            modified = true;
        }
    }

    Ok(modified)
}

/// Walk directory and collect file paths (simple, no external dep).
fn walkdir(dir: &Path) -> anyhow::Result<Vec<std::path::PathBuf>> {
    let mut result = Vec::new();
    if !dir.is_dir() {
        return Ok(result);
    }
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            result.extend(walkdir(&path)?);
        } else {
            result.push(path);
        }
    }
    Ok(result)
}

/// Update `description:` field in SKILL.md YAML frontmatter.
fn update_skill_description(skill_path: &Path, new_description: &str) -> anyhow::Result<bool> {
    let content = std::fs::read_to_string(skill_path)?;

    // Check for YAML frontmatter
    if !content.starts_with("---") {
        return Ok(false);
    }

    // Find end of frontmatter
    let rest = &content[3..];
    let Some(end_idx) = rest.find("\n---") else {
        return Ok(false);
    };

    let frontmatter = &rest[..end_idx];
    let after_frontmatter = &rest[end_idx..];

    // Find and replace description line
    let mut updated_lines = Vec::new();
    let mut found = false;
    for line in frontmatter.lines() {
        if line.starts_with("description:") {
            let old_desc = line.trim_start_matches("description:").trim();
            if old_desc.trim_matches('"') == new_description
                || old_desc.trim_matches('\'') == new_description
                || old_desc == new_description
            {
                // Already in sync
                updated_lines.push(line.to_string());
                continue;
            }
            // Preserve quoting style
            if old_desc.starts_with('"') {
                updated_lines.push(format!("description: \"{new_description}\""));
            } else if old_desc.starts_with('\'') {
                updated_lines.push(format!("description: '{new_description}'"));
            } else {
                updated_lines.push(format!("description: {new_description}"));
            }
            found = true;
        } else {
            updated_lines.push(line.to_string());
        }
    }

    if !found {
        return Ok(false);
    }

    let new_frontmatter = updated_lines.join("\n");
    let new_content = format!("---\n{new_frontmatter}{after_frontmatter}");

    if new_content == content {
        return Ok(false);
    }

    std::fs::write(skill_path, new_content)?;
    Ok(true)
}
