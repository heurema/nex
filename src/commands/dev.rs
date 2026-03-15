use crate::core::dirs::validate_name;
use std::fs;
use std::path::Path;

pub fn dev_link(path: &str) -> anyhow::Result<()> {
    let src = std::path::PathBuf::from(path)
        .canonicalize()
        .map_err(|e| anyhow::anyhow!("path '{}' not found: {e}", path))?;

    let name = extract_plugin_name(&src)?;
    validate_name(&name)?;

    let home = dirs::home_dir()
        .ok_or_else(|| anyhow::anyhow!("cannot determine home directory"))?;
    let plugins_dir = home.join(".claude").join("plugins");
    fs::create_dir_all(&plugins_dir)?;

    let link = plugins_dir.join(&name);
    if link.exists() || link.is_symlink() {
        anyhow::bail!(
            "symlink '{}' already exists in ~/.claude/plugins/. Run `dev unlink {name}` first.",
            name
        );
    }

    std::os::unix::fs::symlink(&src, &link)
        .map_err(|e| anyhow::anyhow!("failed to create symlink: {e}"))?;

    println!("Linked: ~/.claude/plugins/{name} -> {}", src.display());
    Ok(())
}

pub fn dev_unlink(name: &str) -> anyhow::Result<()> {
    validate_name(name)?;

    let home = dirs::home_dir()
        .ok_or_else(|| anyhow::anyhow!("cannot determine home directory"))?;
    let link = home.join(".claude").join("plugins").join(name);

    if !link.exists() && !link.is_symlink() {
        anyhow::bail!("no dev link found at ~/.claude/plugins/{name}");
    }

    // Only remove if it is a symlink (don't touch real installs)
    let meta = link.symlink_metadata()
        .map_err(|e| anyhow::anyhow!("cannot stat link path: {e}"))?;
    if !meta.file_type().is_symlink() {
        anyhow::bail!("~/.claude/plugins/{name} is not a symlink — refusing to remove");
    }

    fs::remove_file(&link)
        .map_err(|e| anyhow::anyhow!("failed to remove symlink: {e}"))?;

    println!("Unlinked: ~/.claude/plugins/{name}");
    Ok(())
}

fn extract_plugin_name(dir: &Path) -> anyhow::Result<String> {
    // Try plugin.json name field first
    let plugin_json = dir.join(".claude-plugin").join("plugin.json");
    if plugin_json.exists() {
        let content = fs::read_to_string(&plugin_json)?;
        let v: serde_json::Value = serde_json::from_str(&content)
            .map_err(|e| anyhow::anyhow!("failed to parse plugin.json: {e}"))?;
        if let Some(name) = v.get("name").and_then(|n| n.as_str()) {
            if !name.is_empty() {
                return Ok(name.to_string());
            }
        }
    }

    // Fallback: directory name
    let name = dir.file_name()
        .ok_or_else(|| anyhow::anyhow!("cannot determine plugin name from path"))?
        .to_string_lossy()
        .to_string();
    Ok(name)
}
