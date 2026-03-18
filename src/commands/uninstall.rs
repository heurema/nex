use crate::core::{
    dirs::{Dirs, validate_name},
    lock::FileLock,
    state::{InstalledState, Status},
};
use std::fs;
use std::process::Command;

pub fn run(name: &str) -> anyhow::Result<()> {
    // ac-001: validate plugin name against [a-z0-9-]+
    validate_name(name)?;

    let dirs = Dirs::new()?;
    let _lock = FileLock::acquire(&dirs.lock_path())?;

    let mut state = InstalledState::load(&dirs.installed_path())?;
    let plugin = state
        .get(name)
        .ok_or_else(|| anyhow::anyhow!("{name} is not installed"))?
        .clone();

    println!("Uninstalling {name} v{}...", plugin.version);

    let mut all_ok = true;
    // ac-006: track which platforms succeeded so we can keep only failed ones
    let mut succeeded_platforms: Vec<String> = Vec::new();

    for (plat, status) in &plugin.platforms {
        if status.status != Status::Ok {
            continue;
        }
        match plat.as_str() {
            "claude-code" => {
                let ref_name = &status.r#ref;
                // Use scope from our own state, not from CC's installed_plugins.json
                let scope = status.scope.as_deref().unwrap_or("user");
                let output = Command::new("claude")
                    .args(["plugin", "uninstall", ref_name, "--scope", scope])
                    .output();
                match output {
                    Ok(o) if o.status.success() => {
                        println!("  {plat} ✓ removed {ref_name}");
                        // Use dirs.marketplace_dir to get correct path
                        let parts: Vec<&str> = ref_name.split('@').collect();
                        if parts.len() == 2 {
                            let category = parts[1].strip_prefix("nex-").unwrap_or(parts[1]);
                            // finding-9: validate category before passing to marketplace_dir
                            if let Ok(mp_dir) = dirs.marketplace_dir(category) {
                                let marketplace_link = mp_dir.join("plugins").join(name);
                                let _ = fs::remove_file(&marketplace_link);
                            }
                        }
                        succeeded_platforms.push(plat.clone());
                    }
                    Ok(o) => {
                        let stderr = String::from_utf8_lossy(&o.stderr).trim().to_string();
                        eprintln!("  {plat} ✗ {stderr}");
                        all_ok = false;
                    }
                    Err(e) => {
                        eprintln!("  {plat} ✗ {e}");
                        all_ok = false;
                    }
                }
            }
            "codex" | "gemini" => {
                let managed_dir = if plat == "codex" {
                    &dirs.codex_skills
                } else {
                    &dirs.agents_skills
                };
                let link = managed_dir.join(name);
                if link.exists() || link.is_symlink() {
                    let managed_dir_canonical = managed_dir.canonicalize().map_err(|e| {
                        anyhow::anyhow!("cannot canonicalize managed skill dir: {e}")
                    })?;
                    let link_parent_canonical =
                        link.parent()
                            .and_then(|p| p.canonicalize().ok())
                            .ok_or_else(|| anyhow::anyhow!("cannot canonicalize link parent"))?;
                    if link_parent_canonical != managed_dir_canonical {
                        anyhow::bail!(
                            "symlink parent resolves outside managed tree — aborting for security"
                        );
                    }
                    fs::remove_file(&link).or_else(|_| fs::remove_dir_all(&link))?;
                    println!("  {plat} ✓ removed {}", link.display());
                } else {
                    println!("  {plat} ✓ (no entry found)");
                }
                succeeded_platforms.push(plat.clone());
            }
            _ => {}
        }
    }

    // ac-006: keep state for failed platforms; remove only succeeded ones
    if !all_ok {
        eprintln!(
            "Warning: some platform uninstalls failed. Removing only succeeded platforms from state."
        );
        eprintln!("Source kept at ~/.skills/{name}/");
        eprintln!("Run `nex install {name}` to re-sync or manually clean up.");

        // Update state: remove succeeded platforms, keep failed ones
        if let Some(mut plugin_entry) = state.plugins.get(name).cloned() {
            for succeeded in &succeeded_platforms {
                plugin_entry.platforms.remove(succeeded.as_str());
            }
            if plugin_entry.platforms.is_empty() {
                state.remove(name);
            } else {
                state.set(name.to_string(), plugin_entry);
            }
        }
        state.save(&dirs.installed_path())?;
        return Ok(());
    }

    // Remove source only if ALL platform uninstalls succeeded
    let skill_dir = dirs.skills_store.join(name);
    if skill_dir.exists() {
        fs::remove_dir_all(&skill_dir)?;
    }
    let backup_dir = dirs.skills_store.join(format!("{name}.prev"));
    if backup_dir.exists() {
        fs::remove_dir_all(&backup_dir)?;
    }

    state.remove(name);
    state.save(&dirs.installed_path())?;

    println!("Uninstalled {name}.");
    println!("Restart active CLI sessions to apply changes.");
    Ok(())
}
