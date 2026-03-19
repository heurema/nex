use crate::core::{
    dirs::{Dirs, validate_name},
    hash::compute_sha256,
    lock::FileLock,
    platform,
    reconcile,
    registry::Registry,
    state,
};
use chrono::Utc; // ac-013: chrono crate for portable datetime
use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::Path;

pub fn run(
    name: &str,
    claude_code: bool,
    codex: bool,
    gemini: bool,
    scope: &str,
) -> anyhow::Result<()> {
    // ac-001: validate plugin name against [a-z0-9-]+
    validate_name(name)?;

    let dirs = Dirs::new()?;
    dirs.ensure_dirs()?;
    let _lock = FileLock::acquire(&dirs.lock_path())?;

    install_inner(name, claude_code, codex, gemini, scope, &dirs)
}

/// Inner install logic — does not acquire the lock.
/// Called by run() (which holds the lock) and by update::run() (which also holds the lock).
pub fn install_inner(
    name: &str,
    claude_code: bool,
    codex: bool,
    gemini: bool,
    scope: &str,
    dirs: &Dirs,
) -> anyhow::Result<()> {
    // Fix 1: validate name inside install_inner so update path also validates
    validate_name(name)?;
    let registry = Registry::load(&dirs.registry_path(), false)?;
    let pkg = registry
        .get(name)
        .ok_or_else(|| anyhow::anyhow!("Package '{name}' not found in registry"))?;

    // Reconcile: compute target platforms using precedence rules
    let targets = reconcile::resolve_targets(
        &pkg.platforms,
        claude_code,
        codex,
        gemini,
        None, // profile wiring deferred to Stage 3
    );
    if targets.is_empty() {
        anyhow::bail!(
            "No installable platforms for '{name}'. \
             Detected CLIs must overlap with supported platforms: [{}]",
            pkg.platforms.join(", ")
        );
    }

    println!(
        "Installing {name} v{} for: {}",
        pkg.version,
        targets
            .iter()
            .map(|p| p.label())
            .collect::<Vec<_>>()
            .join(", ")
    );

    // PREFLIGHT
    if targets.contains(&platform::Platform::ClaudeCode) {
        check_blocklist(name, dirs)?;
        check_conflicts(name, pkg, dirs)?;
    }

    // STAGE: Clone to temp
    let tmp_dir = tempfile::tempdir()?;
    let clone_path = tmp_dir.path().join(name);
    clone_repo(&pkg.repo, &pkg.version, &clone_path)?;

    // SHA256 = hard error (unless --dev skip)
    let computed_sha = compute_sha256(&clone_path)?;
    if pkg.sha256 == "skip-dev" {
        eprintln!("SHA256 check skipped (dev mode)");
    } else if computed_sha != pkg.sha256 {
        anyhow::bail!(
            "SHA256 MISMATCH — aborting install!\n  Expected: {}\n  Got:      {}\n  \
             This may indicate a tampered package or uncommitted changes.",
            pkg.sha256,
            computed_sha
        );
    } else {
        println!("SHA256 verified ✓");
    }

    // COMMIT: Move to final location
    let skill_dir = dirs.skills_store.join(name);
    let backup_dir = dirs.skills_store.join(format!("{name}.prev"));

    // Rollback-safe rename
    let had_backup = if skill_dir.exists() {
        if backup_dir.exists() {
            fs::remove_dir_all(&backup_dir)?;
        }
        fs::rename(&skill_dir, &backup_dir)?;
        true
    } else {
        false
    };
    if let Err(e) = fs::rename(&clone_path, &skill_dir) {
        if backup_dir.exists() {
            // restore: move backup back to skill_dir on commit failure
            let _ = fs::rename(&backup_dir, &skill_dir);
        }
        anyhow::bail!(
            "Failed to move package to ~/.skills/{name}: {e}. Previous version restored."
        );
    }

    // PER-PLATFORM install
    let mut platform_statuses: HashMap<String, state::PlatformStatus> = HashMap::new();

    for target in &targets {
        let result = match target {
            platform::Platform::ClaudeCode => install_claude_code(name, &pkg.category, scope, dirs),
            platform::Platform::Codex | platform::Platform::Gemini => {
                install_agent_skill(name, target, dirs)
            }
        };

        match result {
            Ok(ref_str) => {
                println!("  {} ✓", target.label());
                platform_statuses.insert(
                    target.label().to_string(),
                    state::PlatformStatus {
                        status: state::Status::Ok,
                        r#ref: ref_str,
                        error: None,
                        scope: if *target == platform::Platform::ClaudeCode {
                            Some(scope.to_string()) // persist scope
                        } else {
                            None
                        },
                    },
                );
            }
            Err(e) => {
                eprintln!("  {} ✗ {e}", target.label());
                platform_statuses.insert(
                    target.label().to_string(),
                    state::PlatformStatus {
                        status: state::Status::Failed,
                        r#ref: String::new(),
                        error: Some(e.to_string()),
                        scope: None,
                    },
                );
            }
        }
    }

    let ok_count = platform_statuses
        .values()
        .filter(|p| p.status == state::Status::Ok)
        .count();
    let total = targets.len();

    // ac-005: bail if no platforms succeeded (ok_count == 0 → bail)
    // ac-007 + claude-finding-3: rollback only when ALL platforms fail (ok_count == 0)
    // claude-finding-2: do NOT save new state when rolling back — disk has old version
    if ok_count == 0 {
        if had_backup && backup_dir.exists() {
            eprintln!("All platforms failed; restoring previous version...");
            let _ = fs::remove_dir_all(&skill_dir);
            if let Err(e) = fs::rename(&backup_dir, &skill_dir) {
                eprintln!("Warning: could not restore backup: {e}");
            } else {
                eprintln!("Previous version restored from backup.");
            }
        } else if skill_dir.exists() {
            // Fresh install, no backup — clean up orphaned skill_dir
            let _ = fs::remove_dir_all(&skill_dir);
        }
        anyhow::bail!("Install failed: no platforms succeeded (0/{total})");
    }

    let mut st = state::InstalledState::load(&dirs.installed_path())?;
    st.set(
        name.to_string(),
        state::InstalledPlugin {
            version: pkg.version.clone(),
            sha256: computed_sha,
            installed_at: chrono_now(),
            source: skill_dir.to_string_lossy().to_string(),
            platforms: platform_statuses,
            origin: state::Origin::Managed,
            last_applied_profile: None,
        },
    );
    st.save(&dirs.installed_path())?;

    // Drift detection: check if all target platforms are realized
    let drift = reconcile::detect_drift(&targets, &st.get(name).unwrap().platforms);
    if drift.is_empty() {
        println!(
            "\nInstalled {name} v{} ({ok_count}/{total} platforms)",
            pkg.version
        );
    } else {
        eprintln!(
            "warning: {name}: platform drift detected — missing: [{}]",
            drift.join(", ")
        );
        println!(
            "\nPartially installed {name} v{} ({ok_count}/{total} platforms, {} in drift)",
            pkg.version,
            drift.len()
        );
    }
    println!("Restart active CLI sessions to apply changes.");

    Ok(())
}

fn clone_repo(repo_url: &str, tag: &str, dest: &Path) -> anyhow::Result<()> {
    let repo = git2::Repository::clone(repo_url, dest)
        .map_err(|e| anyhow::anyhow!("git clone failed: {e}"))?;

    let tag_ref = format!("refs/tags/{tag}");
    let reference = repo
        .find_reference(&tag_ref)
        .or_else(|_| repo.find_reference(&format!("refs/tags/v{tag}")))
        .map_err(|_| {
            anyhow::anyhow!(
                "tag '{tag}' not found in repo. Available tags: {}",
                list_tags(&repo)
            )
        })?;

    let commit = reference
        .peel_to_commit()
        .map_err(|e| anyhow::anyhow!("cannot resolve tag {tag}: {e}"))?;
    repo.checkout_tree(commit.as_object(), None)
        .map_err(|e| anyhow::anyhow!("checkout failed: {e}"))?;
    repo.set_head_detached(commit.id())
        .map_err(|e| anyhow::anyhow!("detach HEAD failed: {e}"))?;

    Ok(())
}

fn list_tags(repo: &git2::Repository) -> String {
    repo.tag_names(None)
        .map(|tags| tags.iter().flatten().collect::<Vec<_>>().join(", "))
        .unwrap_or_else(|_| "none".to_string())
}

// ac-003 + codex-finding-5: symlinks in packages are REJECTED (not skipped) to prevent integrity bypass
// compute_sha256 and walkdir moved to core::hash

fn install_claude_code(
    name: &str,
    category: &str,
    scope: &str,
    dirs: &Dirs,
) -> anyhow::Result<String> {
    // ac-002: validate category (marketplace_dir returns Result now)
    validate_name(category)?;
    let marketplace_dir = dirs.marketplace_dir(category)?;
    let plugins_dir = marketplace_dir.join("plugins");
    let manifest_dir = marketplace_dir.join(".claude-plugin");

    fs::create_dir_all(&plugins_dir)?;
    fs::create_dir_all(&manifest_dir)?;

    // Security: verify plugins_dir and manifest_dir are not symlinks and stay within managed tree
    {
        let expected_base = dirs
            .claude_plugins
            .canonicalize()
            .map_err(|e| anyhow::anyhow!("cannot canonicalize claude_plugins dir: {e}"))?;

        let plugins_dir_meta = plugins_dir
            .symlink_metadata()
            .map_err(|e| anyhow::anyhow!("cannot stat plugins dir: {e}"))?;
        if plugins_dir_meta.file_type().is_symlink() {
            anyhow::bail!("plugins directory is a symlink — aborting for security");
        }
        let plugins_dir_canonical = plugins_dir
            .canonicalize()
            .map_err(|e| anyhow::anyhow!("cannot canonicalize plugins dir: {e}"))?;
        if !plugins_dir_canonical.starts_with(&expected_base) {
            anyhow::bail!("plugins directory is outside managed tree — aborting for security");
        }

        // Fix 2: also verify manifest_dir (.claude-plugin) stays within managed tree
        let manifest_dir_meta = manifest_dir
            .symlink_metadata()
            .map_err(|e| anyhow::anyhow!("cannot stat manifest dir: {e}"))?;
        if manifest_dir_meta.file_type().is_symlink() {
            anyhow::bail!(
                "manifest directory (.claude-plugin) is a symlink — aborting for security"
            );
        }
        let manifest_dir_canonical = manifest_dir
            .canonicalize()
            .map_err(|e| anyhow::anyhow!("cannot canonicalize manifest dir: {e}"))?;
        if !manifest_dir_canonical.starts_with(&expected_base) {
            anyhow::bail!("manifest directory is outside managed tree — aborting for security");
        }
    }

    let link_path = plugins_dir.join(name);
    let target = dirs.skills_store.join(name).join("platforms/claude-code");

    if !target.exists() {
        anyhow::bail!("platforms/claude-code/ not found in package");
    }

    // codex-finding-6: verify adapter path stays within skill_dir (prevent symlink escape)
    let skill_dir_canonical = dirs
        .skills_store
        .join(name)
        .canonicalize()
        .map_err(|e| anyhow::anyhow!("cannot canonicalize skill dir: {e}"))?;
    // target must not itself be a symlink
    let target_meta = target
        .symlink_metadata()
        .map_err(|e| anyhow::anyhow!("cannot stat adapter path: {e}"))?;
    if target_meta.file_type().is_symlink() {
        anyhow::bail!("platforms/claude-code is a symlink — package rejected for security");
    }
    let target_canonical = target
        .canonicalize()
        .map_err(|e| anyhow::anyhow!("cannot canonicalize adapter path: {e}"))?;
    if !target_canonical.starts_with(&skill_dir_canonical) {
        anyhow::bail!(
            "platforms/claude-code escapes skill directory — package rejected for security"
        );
    }

    if link_path.exists() || link_path.is_symlink() {
        fs::remove_file(&link_path).or_else(|_| fs::remove_dir_all(&link_path))?;
    }
    std::os::unix::fs::symlink(&target, &link_path)?;

    let marketplace_json = generate_marketplace_json(category, &plugins_dir)?;
    fs::write(manifest_dir.join("marketplace.json"), marketplace_json)?;

    let marketplace_name = format!("nex-{category}");
    register_marketplace(&marketplace_name, &marketplace_dir, dirs)?;

    let validate = std::process::Command::new("claude")
        .args(["plugin", "validate", &marketplace_dir.to_string_lossy()])
        .output();

    if let Ok(output) = validate {
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let _ = fs::remove_file(&link_path);
            anyhow::bail!("Claude Code validation failed: {stderr}");
        }
    }

    let install = std::process::Command::new("claude")
        .args([
            "plugin",
            "install",
            &format!("{name}@{marketplace_name}"),
            "--scope",
            scope,
        ])
        .output()
        .map_err(|e| anyhow::anyhow!("claude plugin install failed: {e}"))?;

    if !install.status.success() {
        let stderr = String::from_utf8_lossy(&install.stderr);
        anyhow::bail!("claude plugin install failed: {stderr}");
    }

    Ok(format!("{name}@{marketplace_name}"))
}

fn generate_marketplace_json(category: &str, plugins_dir: &Path) -> anyhow::Result<String> {
    let mut plugins = Vec::new();

    if plugins_dir.exists() {
        for entry in fs::read_dir(plugins_dir)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().to_string();
            let plugin_json_path = entry.path().join(".claude-plugin/plugin.json");
            let description = if plugin_json_path.exists() {
                let content = fs::read_to_string(&plugin_json_path)?;
                let v: serde_json::Value = serde_json::from_str(&content)?;
                v.get("description")
                    .and_then(|d| d.as_str())
                    .unwrap_or("")
                    .to_string()
            } else {
                String::new()
            };

            plugins.push(serde_json::json!({
                "name": name,
                "source": format!("./plugins/{name}"),
                "description": description,
            }));
        }
    }

    let marketplace = serde_json::json!({
        "name": format!("nex-{category}"),
        "owner": { "name": "heurema" },
        "metadata": {
            "description": format!("heurema {category} plugins")
        },
        "plugins": plugins
    });

    Ok(serde_json::to_string_pretty(&marketplace)?)
}

fn install_agent_skill(
    name: &str,
    preferred: &platform::Platform,
    dirs: &Dirs,
) -> anyhow::Result<String> {
    let managed_dir = match preferred {
        platform::Platform::Codex => &dirs.codex_skills,
        platform::Platform::Gemini => &dirs.agents_skills,
        platform::Platform::ClaudeCode => {
            anyhow::bail!("install_agent_skill called for claude-code")
        }
    };

    // Verify managed skill dir resolves to the expected home-based path.
    {
        let home =
            dirs::home_dir().ok_or_else(|| anyhow::anyhow!("cannot determine home directory"))?;
        let expected_managed_dir = match preferred {
            platform::Platform::Codex => home.join(".codex").join("skills"),
            platform::Platform::Gemini => home.join(".agents").join("skills"),
            platform::Platform::ClaudeCode => unreachable!(),
        };
        let managed_dir_canonical = managed_dir
            .canonicalize()
            .map_err(|e| anyhow::anyhow!("cannot canonicalize managed skill dir: {e}"))?;
        let expected_canonical = expected_managed_dir
            .canonicalize()
            .map_err(|e| anyhow::anyhow!("cannot canonicalize expected managed skill path: {e}"))?;
        if managed_dir_canonical != expected_canonical {
            anyhow::bail!(
                "managed skill directory resolves outside expected path — aborting for security"
            );
        }
    }

    let link_path = managed_dir.join(name);
    let skill_dir = dirs.skills_store.join(name);

    // codex-finding-7: canonicalize skill_dir to verify all sources stay within it
    let skill_dir_canonical = skill_dir
        .canonicalize()
        .map_err(|e| anyhow::anyhow!("cannot canonicalize skill dir: {e}"))?;

    // Determine format_version to decide fallback policy
    let format_version = {
        let pj = skill_dir.join(".claude-plugin/plugin.json");
        if pj.exists() {
            fs::read_to_string(&pj)
                .ok()
                .and_then(|c| serde_json::from_str::<serde_json::Value>(&c).ok())
                .and_then(|v| v.get("format_version").and_then(|x| x.as_u64()))
                .unwrap_or(0) as u32
        } else {
            0
        }
    };

    // Resolve platform adapter: preferred dir, then fallback (legacy only)
    let (first, second) = match preferred {
        platform::Platform::Gemini => ("platforms/gemini", "platforms/codex"),
        _ => ("platforms/codex", "platforms/gemini"),
    };

    let source = if skill_dir.join(first).exists() {
        skill_dir.join(first)
    } else if format_version >= 2 {
        // Strict: format_version >= 2 requires dedicated platform adapter
        anyhow::bail!(
            "{name}: no dedicated adapter at {}. \
             format_version >= 2 does not allow fallback. \
             Create platforms/{} or downgrade format_version.",
            first,
            preferred.label()
        );
    } else if skill_dir.join(second).exists() {
        eprintln!(
            "warning: {name}: using fallback adapter for {}. Create a dedicated platform adapter.",
            preferred.label()
        );
        skill_dir.join(second)
    } else {
        let root_skill = skill_dir.join("SKILL.md");
        if !root_skill.exists() {
            anyhow::bail!("No platform adapter or root SKILL.md found");
        }
        eprintln!(
            "warning: {name}: using root SKILL.md fallback for {}. Create platforms/{} adapter.",
            preferred.label(),
            preferred.label()
        );
        // Legacy fallback: agents expect a directory target; create wrapper
        let fallback_dir = skill_dir.join("platforms/_fallback");
        fs::create_dir_all(&fallback_dir)?;
        let fallback_skill = fallback_dir.join("SKILL.md");
        fs::copy(&root_skill, &fallback_skill)?;
        fallback_dir
    };

    // codex-finding-7: verify source is not a symlink and stays within skill_dir
    let source_meta = source
        .symlink_metadata()
        .map_err(|e| anyhow::anyhow!("cannot stat source path: {e}"))?;
    if source_meta.file_type().is_symlink() {
        anyhow::bail!("platform adapter source is a symlink — package rejected for security");
    }
    let source_canonical = source
        .canonicalize()
        .map_err(|e| anyhow::anyhow!("cannot canonicalize source path: {e}"))?;
    if !source_canonical.starts_with(&skill_dir_canonical) {
        anyhow::bail!("platform adapter escapes skill directory — package rejected for security");
    }

    if link_path.exists() || link_path.is_symlink() {
        fs::remove_file(&link_path).or_else(|_| fs::remove_dir_all(&link_path))?;
    }
    std::os::unix::fs::symlink(&source, &link_path)?;

    Ok(link_path.to_string_lossy().to_string())
}

fn register_marketplace(
    marketplace_name: &str,
    marketplace_dir: &Path,
    dirs: &Dirs,
) -> anyhow::Result<()> {
    let known_path = dirs.claude_plugins.join("known_marketplaces.json");
    let mut known: serde_json::Value = if known_path.exists() {
        let content = fs::read_to_string(&known_path)?;
        serde_json::from_str(&content)?
    } else {
        serde_json::json!({})
    };

    if known.get(marketplace_name).is_some() {
        return Ok(());
    }

    known[marketplace_name] = serde_json::json!({
        "source": {
            "source": "directory",
            "path": marketplace_dir.to_string_lossy()
        },
        "installLocation": marketplace_dir.to_string_lossy(),
        "lastUpdated": chrono_now()
    });

    let parent = if let Some(p) = known_path.parent() {
        fs::create_dir_all(p)?;
        p.to_path_buf()
    } else {
        std::path::PathBuf::from(".")
    };
    // Security: use NamedTempFile (not a predictable path) to prevent symlink-follow attacks
    let mut tmp = tempfile::NamedTempFile::new_in(&parent)?;
    tmp.write_all(serde_json::to_string_pretty(&known)?.as_bytes())?;
    tmp.flush()?;
    tmp.persist(&known_path)
        .map_err(|e| anyhow::anyhow!("failed to persist known_marketplaces.json: {}", e.error))?;
    eprintln!("Registered marketplace: {marketplace_name}");
    Ok(())
}

fn check_blocklist(name: &str, dirs: &Dirs) -> anyhow::Result<()> {
    let blocklist_path = dirs.claude_plugins.join("blocklist.json");
    if !blocklist_path.exists() {
        return Ok(());
    }
    let content = fs::read_to_string(&blocklist_path)?;
    let json: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(e) => anyhow::bail!("Failed to parse blocklist.json: {e}"),
    };
    // blocklist.json is an object with plugin names as keys
    if json.get(name).is_some() {
        anyhow::bail!("{name} is blocklisted in Claude Code");
    }
    // Also check array format
    if let Some(arr) = json.as_array() {
        if arr.iter().any(|v| v.as_str() == Some(name)) {
            anyhow::bail!("{name} is blocklisted in Claude Code");
        }
    }
    Ok(())
}

fn check_conflicts(
    name: &str,
    _pkg: &crate::core::registry::Package,
    dirs: &Dirs,
) -> anyhow::Result<()> {
    let installed_plugins_path = dirs.claude_plugins.join("installed_plugins.json");
    if !installed_plugins_path.exists() {
        return Ok(());
    }
    let content = fs::read_to_string(&installed_plugins_path)?;
    let json: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(_) => return Ok(()),
    };
    // Check plugins object for conflicting keys
    if let Some(plugins) = json.get("plugins").and_then(|p| p.as_object()) {
        for suffix in ["@emporium", "@local", "@claude-plugins-official"] {
            let key = format!("{name}{suffix}");
            if plugins.contains_key(&key) {
                eprintln!(
                    "Warning: {key} already installed. Consider uninstalling it to avoid duplicates."
                );
            }
        }
    }
    Ok(())
}

// ac-013: portable datetime using chrono crate (no shell subprocess)
fn chrono_now() -> String {
    use chrono::SecondsFormat;
    Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)
}
