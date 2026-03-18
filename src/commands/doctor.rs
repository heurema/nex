use crate::core::{
    cc_adapter,
    config::{self, expand_placeholders},
    dirs::Dirs,
    git,
    hash,
    marketplace::{self, MarketplaceRef},
    registry::Registry,
    state::{InstalledPlugin, InstalledState, Status},
};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

#[allow(dead_code)]
enum Severity {
    Ok,
    Warn,
    Error,
}

enum FixAction {
    None,
    RemoveFile(PathBuf),
    RemoveDir(PathBuf),
    /// Tag current HEAD and propagate to marketplace (version already set).
    TagAndPropagate {
        plugin_dir: PathBuf,
    },
    /// Bump version and run full release pipeline.
    BumpAndRelease {
        plugin_dir: PathBuf,
    },
}

struct Issue {
    plugin: String,
    check: &'static str,
    severity: Severity,
    message: String,
    fix: String,
    fix_action: FixAction,
}

pub fn run(deep: bool, fix: bool, filter: Option<&[String]>) -> anyhow::Result<()> {
    let dirs = Dirs::new()?;
    let state = InstalledState::load(&dirs.installed_path())?;
    let registry = Registry::load(&dirs.registry_path(), false)?;

    let mut issues: Vec<Issue> = Vec::new();

    for (name, plugin) in &state.plugins {
        check_skill_dir(name, &dirs, &mut issues);
        check_cc_symlinks(name, plugin, &dirs, &mut issues);
        check_agent_skill_links(name, plugin, &dirs, &mut issues);
        check_registry_orphan(name, &registry, &mut issues);
        if deep {
            check_sha256(name, plugin, &dirs, &mut issues);
        }
    }
    check_stale_lock(&dirs, &mut issues);

    // Emporium-aware checks
    let catalog =
        cc_adapter::load_emporium_catalog(&dirs.emporium_marketplace_path()).unwrap_or_default();
    let cc_cache = cc_adapter::scan_cc_cache(&dirs.cc_cache_dir());
    let dev_symlinks = cc_adapter::scan_dev_symlinks(&dirs.claude_plugins);
    let live_views = cc_adapter::load_plugin_views(&dirs)?;

    check_nex_devtools(&dirs, &mut issues);
    check_emporium_drift(&catalog, &cc_cache, &mut issues);
    check_duplicate_plugins(&catalog, &dev_symlinks, &dirs, &mut issues);
    check_stale_dev_symlinks(&dev_symlinks, &dirs, &mut issues);
    check_orphan_cache(&catalog, &dirs, &mut issues);

    // Release drift checks (dev-linked plugins)
    let global_cfg = config::load_global(&dirs.nex_home)?;
    check_release_drift(&dev_symlinks, &dirs, &catalog, &global_cfg, &mut issues);

    // Apply plugin filter: keep only issues for specified plugins (drop global issues too)
    if let Some(names) = filter {
        issues.retain(|i| names.iter().any(|n| n == &i.plugin));
    }

    // Collect all unique plugin names for output.
    // In filtered mode, also allow plugins discovered from live Claude/Codex state.
    let mut all_names: Vec<String> = if let Some(names) = filter {
        names
            .iter()
            .filter(|n| {
                state.plugins.contains_key(n.as_str())
                    || live_views
                        .iter()
                        .any(|view| view.name == **n && view.is_live_discovered())
                    || issues.iter().any(|i| &i.plugin == *n)
            })
            .cloned()
            .collect()
    } else {
        let mut names: Vec<String> = state.plugins.keys().cloned().collect();
        for issue in &issues {
            if !issue.plugin.is_empty() && !names.contains(&issue.plugin) {
                names.push(issue.plugin.clone());
            }
        }
        names
    };
    all_names.sort();
    all_names.dedup();

    if all_names.is_empty() && issues.is_empty() {
        println!("No plugins installed. Nothing to check.");
        return Ok(());
    }

    let issue_count = issues
        .iter()
        .filter(|i| !matches!(i.severity, Severity::Ok))
        .count();

    if !all_names.is_empty() {
        println!(
            "Checking {} plugin{}...\n",
            all_names.len(),
            if all_names.len() == 1 { "" } else { "s" }
        );
    }

    // Per-plugin summary
    for name in &all_names {
        let plugin_issues: Vec<&Issue> = issues
            .iter()
            .filter(|i| i.plugin == *name && !matches!(i.severity, Severity::Ok))
            .collect();
        if plugin_issues.is_empty() {
            println!("\x1b[32m[OK]\x1b[0m   {name}");
        } else {
            for issue in &plugin_issues {
                let tag = match issue.severity {
                    Severity::Error => "\x1b[31m[ERR]\x1b[0m ",
                    Severity::Warn => "\x1b[33m[WARN]\x1b[0m",
                    Severity::Ok => "\x1b[32m[OK]\x1b[0m  ",
                };
                println!("{tag}  {name}  {}: {}", issue.check, issue.message);
            }
        }
    }

    // Non-plugin issues (stale lock, etc.)
    for issue in issues
        .iter()
        .filter(|i| i.plugin.is_empty() && !matches!(i.severity, Severity::Ok))
    {
        let tag = match issue.severity {
            Severity::Error => "\x1b[31m[ERR]\x1b[0m ",
            Severity::Warn => "\x1b[33m[WARN]\x1b[0m",
            Severity::Ok => "\x1b[32m[OK]\x1b[0m  ",
        };
        println!("{tag}  {}: {}", issue.check, issue.message);
    }

    if issue_count == 0 {
        println!("\nAll checks passed.");
        return Ok(());
    }

    if fix {
        println!("\nApplying fixes...\n");
        let fixed = apply_fixes(&issues, &global_cfg)?;
        let remaining = issue_count - fixed;

        if remaining == 0 {
            println!(
                "\n{fixed} issue{} fixed. All clear.",
                if fixed == 1 { "" } else { "s" }
            );
            return Ok(());
        }

        println!("\n{fixed} fixed, {remaining} remaining (manual fix needed).");
        let unfixable: Vec<&Issue> = issues
            .iter()
            .filter(|i| {
                !matches!(i.severity, Severity::Ok)
                    && matches!(i.fix_action, FixAction::None)
                    && !i.fix.is_empty()
            })
            .collect();
        if !unfixable.is_empty() {
            println!("\nManual fixes:");
            for issue in unfixable {
                let target = if issue.plugin.is_empty() {
                    "—"
                } else {
                    &issue.plugin
                };
                println!("  {target}: {}", issue.fix);
            }
        }
        anyhow::bail!("doctor: {remaining} unfixed issue(s)");
    }

    // Not in fix mode — show suggestions
    let fixable: Vec<&Issue> = issues
        .iter()
        .filter(|i| !i.fix.is_empty() && !matches!(i.severity, Severity::Ok))
        .collect();
    if !fixable.is_empty() {
        println!("\nSuggested fixes:");
        for issue in &fixable {
            let target = if issue.plugin.is_empty() {
                "—".to_string()
            } else {
                issue.plugin.clone()
            };
            println!("  {target}: {}", issue.fix);
        }
        println!("\n  Tip: run `nex doctor --fix` to auto-fix.");
    }

    println!(
        "\n{issue_count} issue{} found.",
        if issue_count == 1 { "" } else { "s" }
    );
    anyhow::bail!("doctor found {issue_count} issue(s)");
}

// ── Fix application ─────────────────────────────────────────────────────────

fn apply_fixes(issues: &[Issue], global_cfg: &config::GlobalConfig) -> anyhow::Result<usize> {
    let mut fixed = 0;
    for issue in issues {
        if matches!(issue.severity, Severity::Ok) {
            continue;
        }
        let label = if issue.plugin.is_empty() {
            issue.check.to_string()
        } else {
            format!("{} ({})", issue.plugin, issue.check)
        };
        match &issue.fix_action {
            FixAction::None => {}
            FixAction::RemoveFile(path) => match fs::remove_file(path) {
                Ok(()) => {
                    println!(
                        "  \x1b[32m[FIXED]\x1b[0m {label}: removed {}",
                        path.display()
                    );
                    fixed += 1;
                }
                Err(e) => {
                    eprintln!("  \x1b[31m[FAIL]\x1b[0m  {label}: {e}");
                }
            },
            FixAction::RemoveDir(path) => match fs::remove_dir_all(path) {
                Ok(()) => {
                    println!(
                        "  \x1b[32m[FIXED]\x1b[0m {label}: removed {}",
                        path.display()
                    );
                    fixed += 1;
                }
                Err(e) => {
                    eprintln!("  \x1b[31m[FAIL]\x1b[0m  {label}: {e}");
                }
            },
            FixAction::TagAndPropagate { plugin_dir } => {
                println!("  {label}:");
                match fix_tag_and_propagate(plugin_dir, global_cfg) {
                    Ok(()) => {
                        fixed += 1;
                    }
                    Err(e) => {
                        eprintln!("    \x1b[31m[FAIL]\x1b[0m  {e}");
                    }
                }
            }
            FixAction::BumpAndRelease { plugin_dir } => {
                println!("  {label}:");
                match fix_bump_and_release(plugin_dir) {
                    Ok(()) => {
                        fixed += 1;
                    }
                    Err(e) => {
                        eprintln!("    \x1b[31m[FAIL]\x1b[0m  {e}");
                    }
                }
            }
        }
    }
    Ok(fixed)
}

/// Tag current HEAD, push, and propagate to marketplace.
/// Used when version is already set in plugin.json but no git tag exists.
fn fix_tag_and_propagate(
    plugin_dir: &Path,
    global_cfg: &config::GlobalConfig,
) -> anyhow::Result<()> {
    let (plugin_name, version) = read_plugin_meta(plugin_dir)
        .ok_or_else(|| anyhow::anyhow!("cannot read plugin.json in {}", plugin_dir.display()))?;

    let plugin_cfg = config::load_plugin(plugin_dir)?;
    let resolved = config::resolve(
        global_cfg,
        &plugin_cfg,
        None,
        None,
        false,
        false,
        Some(plugin_dir),
    )?;

    let tag = expand_placeholders(&resolved.tag_format, &plugin_name, &version, "", "");

    let repo = git2::Repository::discover(plugin_dir)
        .map_err(|e| anyhow::anyhow!("not a git repository: {e}"))?;

    // Verify clean tree (tracked files only)
    let mut opts = git2::StatusOptions::new();
    opts.include_untracked(false).include_ignored(false);
    let statuses = repo
        .statuses(Some(&mut opts))
        .map_err(|e| anyhow::anyhow!("cannot read git status: {e}"))?;
    if !statuses.is_empty() {
        anyhow::bail!("dirty working tree — commit changes first");
    }

    // Verify tag doesn't already exist
    let tag_ref = format!("refs/tags/{tag}");
    if repo.find_reference(&tag_ref).is_ok() {
        anyhow::bail!("tag '{tag}' already exists");
    }

    // Create tag
    let head = repo
        .head()
        .map_err(|e| anyhow::anyhow!("cannot read HEAD: {e}"))?
        .peel(git2::ObjectType::Commit)
        .map_err(|e| anyhow::anyhow!("HEAD is not a commit: {e}"))?;

    if resolved.tag_annotated {
        let sig = repo.signature()?;
        let msg = expand_placeholders(&resolved.tag_message, &plugin_name, &version, &tag, "");
        repo.tag(&tag, &head, &sig, &msg, false)
            .map_err(|e| anyhow::anyhow!("git tag '{tag}' failed: {e}"))?;
    } else {
        repo.tag_lightweight(&tag, &head, false)
            .map_err(|e| anyhow::anyhow!("git tag '{tag}' failed: {e}"))?;
    }
    println!("    \x1b[32m[OK]\x1b[0m TAG    {tag}");

    // Detect branch
    let branch = if resolved.git_branch.is_empty() {
        git::resolve_push_branch(&repo, &resolved.git_remote)
    } else {
        resolved.git_branch.clone()
    };

    // Push exact refs
    let tag_refspec = format!("refs/tags/{tag}");
    let branch_refspec = format!("HEAD:refs/heads/{branch}");
    git_push_exact(&repo, &resolved.git_remote, &branch_refspec, &tag_refspec)?;
    println!(
        "    \x1b[32m[OK]\x1b[0m PUSH   {}/{}",
        resolved.git_remote, branch
    );

    // Propagate to marketplace
    let mut propagate_failed = false;
    if let Some(ref mp_name) = resolved.marketplace {
        if let Some(ref mp_cfg) = resolved.marketplace_config {
            let entry = resolved
                .marketplace_entry
                .as_deref()
                .unwrap_or(&plugin_name);
            let mp =
                MarketplaceRef::new(mp_name.clone(), mp_cfg.clone(), resolved.git_remote.clone());
            // Check marketplace is clean before propagating (avoid committing stale staged changes)
            if let Ok(false) = marketplace::is_clean(&mp) {
                eprintln!(
                    "    \x1b[33m[WARN]\x1b[0m marketplace has uncommitted changes; PROPAGATE skipped"
                );
                propagate_failed = true;
            } else if let Err(e) = marketplace::propagate(
                &mp,
                &plugin_name,
                entry,
                &version,
                &tag,
                false,
                Some(plugin_dir),
            ) {
                eprintln!("    \x1b[33m[WARN]\x1b[0m PROPAGATE failed: {e}");
                propagate_failed = true;
            }
        }
    } else {
        println!("    --   PROPAGATE skipped (no marketplace configured)");
    }

    if propagate_failed {
        anyhow::bail!("tag+push ok but marketplace propagation failed");
    }
    Ok(())
}

/// Bump version and run full release pipeline via `nex release patch --execute`.
fn fix_bump_and_release(plugin_dir: &Path) -> anyhow::Result<()> {
    let nex_exe =
        std::env::current_exe().map_err(|e| anyhow::anyhow!("cannot find nex executable: {e}"))?;

    let status = std::process::Command::new(&nex_exe)
        .args(["release", "patch", "--execute", "--path"])
        .arg(plugin_dir)
        .status()
        .map_err(|e| anyhow::anyhow!("failed to run nex release: {e}"))?;

    if !status.success() {
        anyhow::bail!(
            "nex release failed for {} (exit {})",
            plugin_dir.display(),
            status.code().unwrap_or(-1)
        );
    }
    Ok(())
}

// ── Release drift checks ────────────────────────────────────────────────────

fn check_release_drift(
    dev_symlinks: &HashMap<String, PathBuf>,
    dirs: &Dirs,
    catalog: &HashMap<String, cc_adapter::CatalogPlugin>,
    global_cfg: &config::GlobalConfig,
    issues: &mut Vec<Issue>,
) {
    for (link_name, _target) in dev_symlinks {
        // Resolve symlink to absolute path
        let full_link = dirs.claude_plugins.join(link_name);
        let resolved = match fs::canonicalize(&full_link) {
            Ok(p) => p,
            Err(_) => continue, // broken symlink, caught by check_stale_dev_symlinks
        };

        // Find plugin root (walk up to find .nex/release.toml or root plugin.json)
        let Some(plugin_root) = find_plugin_root(&resolved) else {
            continue;
        };

        // Read plugin metadata
        let Some((plugin_name, version)) = read_plugin_meta(&plugin_root) else {
            continue;
        };

        // Open git repo
        let Ok(repo) = git2::Repository::discover(&plugin_root) else {
            continue;
        };

        // Load configs (defaults if no release.toml)
        let plugin_cfg = config::load_plugin(&plugin_root).unwrap_or_default();
        let Ok(resolved_cfg) = config::resolve(
            global_cfg,
            &plugin_cfg,
            None,
            None,
            false,
            false,
            Some(&plugin_root),
        ) else {
            continue;
        };

        // Compute expected tag for current version
        let tag = expand_placeholders(&resolved_cfg.tag_format, &plugin_name, &version, "", "");
        let tag_ref = format!("refs/tags/{tag}");
        let tag_exists = repo.find_reference(&tag_ref).is_ok();

        if !tag_exists {
            // Version set but no tag → need tag + push + propagate
            issues.push(Issue {
                plugin: plugin_name.clone(),
                check: "release_drift",
                severity: Severity::Warn,
                message: format!("v{version} has no git tag (expected {tag})"),
                fix: format!(
                    "cd {} && nex doctor --fix --plugin {plugin_name}",
                    plugin_root.display()
                ),
                fix_action: FixAction::TagAndPropagate {
                    plugin_dir: plugin_root.clone(),
                },
            });
        } else {
            // Tag exists — check if HEAD is ahead
            let ahead = count_ahead(&repo, &tag_ref);
            if ahead > 0 {
                issues.push(Issue {
                    plugin: plugin_name.clone(),
                    check: "unreleased_commits",
                    severity: Severity::Warn,
                    message: format!(
                        "{ahead} commit{} after {tag}",
                        if ahead == 1 { "" } else { "s" }
                    ),
                    fix: format!(
                        "cd {} && nex release patch --execute",
                        plugin_root.display()
                    ),
                    fix_action: FixAction::BumpAndRelease {
                        plugin_dir: plugin_root.clone(),
                    },
                });
            }
        }

        // Check emporium ref matches current version
        if let Some(cat_entry) = catalog.get(&plugin_name) {
            if !cat_entry.version.is_empty() && cat_entry.version != version {
                // Only report if tag exists (otherwise release_drift covers it)
                if tag_exists {
                    issues.push(Issue {
                        plugin: plugin_name.clone(),
                        check: "marketplace_ref",
                        severity: Severity::Warn,
                        message: format!(
                            "emporium ref v{} != plugin.json v{version}",
                            cat_entry.version
                        ),
                        fix: "will be fixed by release".to_string(),
                        fix_action: FixAction::None,
                    });
                }
            }
        }
    }
}

/// Walk up from `start` looking for a plugin root directory.
/// Matches on `.nex/release.toml` first, then root-level `.claude-plugin/plugin.json`
/// (skipping platform subdirs like `platforms/claude-code/`).
fn find_plugin_root(start: &Path) -> Option<PathBuf> {
    let mut dir = start.to_path_buf();
    for _ in 0..10 {
        if dir.join(".nex").join("release.toml").exists() {
            return Some(dir);
        }
        // Root-level plugin.json (not inside platforms/{platform}/)
        if dir.join(".claude-plugin").join("plugin.json").exists() {
            let name = dir.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name != "claude-code" && name != "codex" && name != "gemini" {
                return Some(dir);
            }
        }
        if !dir.pop() {
            break;
        }
    }
    None
}

/// Read plugin name and version from `.claude-plugin/plugin.json`.
fn read_plugin_meta(plugin_root: &Path) -> Option<(String, String)> {
    let path = plugin_root.join(".claude-plugin").join("plugin.json");
    let content = fs::read_to_string(&path).ok()?;
    let v: serde_json::Value = serde_json::from_str(&content).ok()?;
    let name = v.get("name")?.as_str()?.to_string();
    let version = v.get("version")?.as_str()?.to_string();
    Some((name, version))
}

/// Count commits HEAD is ahead of a reference.
fn count_ahead(repo: &git2::Repository, ref_name: &str) -> usize {
    let head_commit = match repo.head().and_then(|h| h.peel_to_commit()) {
        Ok(c) => c,
        Err(_) => return 0,
    };
    let tag_commit = match repo
        .find_reference(ref_name)
        .and_then(|r| r.peel_to_commit())
    {
        Ok(c) => c,
        Err(_) => return 0,
    };
    if head_commit.id() == tag_commit.id() {
        return 0;
    }
    match repo.graph_ahead_behind(head_commit.id(), tag_commit.id()) {
        Ok((ahead, _)) => ahead,
        Err(_) => 0,
    }
}

// ── Git helpers ──────────────────────────────────────────────────────────────

fn git_push_exact(
    repo: &git2::Repository,
    remote_name: &str,
    branch_refspec: &str,
    tag_refspec: &str,
) -> anyhow::Result<()> {
    // Verify remote exists via git2
    repo.find_remote(remote_name)
        .map_err(|e| anyhow::anyhow!("remote '{remote_name}' not found: {e}"))?;

    // Use system git for push — better SSH auth on macOS (keychain, agent)
    let workdir = repo
        .workdir()
        .ok_or_else(|| anyhow::anyhow!("bare repository"))?;

    let status = std::process::Command::new("git")
        .args(["push", remote_name, branch_refspec, tag_refspec])
        .current_dir(workdir)
        .status()
        .map_err(|e| anyhow::anyhow!("failed to run git push: {e}"))?;

    if !status.success() {
        anyhow::bail!("git push failed (exit {})", status.code().unwrap_or(-1));
    }
    Ok(())
}

// ── Existing health checks ───────────────────────────────────────────────────

fn check_skill_dir(name: &str, dirs: &Dirs, issues: &mut Vec<Issue>) {
    let skill_dir = dirs.skills_store.join(name);
    if !skill_dir.exists() {
        issues.push(Issue {
            plugin: name.to_string(),
            check: "skill_dir",
            severity: Severity::Error,
            message: format!("~/.skills/{name}/ missing"),
            fix: format!("nex install {name}"),
            fix_action: FixAction::None,
        });
    }
}

fn check_cc_symlinks(name: &str, plugin: &InstalledPlugin, dirs: &Dirs, issues: &mut Vec<Issue>) {
    let cc_status = match plugin.platforms.get("claude-code") {
        Some(s) if s.status == Status::Ok => s,
        _ => return,
    };

    let ref_name = &cc_status.r#ref;
    let parts: Vec<&str> = ref_name.split('@').collect();
    if parts.len() == 2 {
        let mp_name = parts[1];
        let link = dirs
            .claude_plugins
            .join("marketplaces")
            .join(mp_name)
            .join("plugins")
            .join(name);
        if !link.exists() {
            issues.push(Issue {
                plugin: name.to_string(),
                check: "cc_symlink",
                severity: Severity::Warn,
                message: format!("marketplace symlink missing: {}", link.display()),
                fix: format!("nex install {name}"),
                fix_action: FixAction::None,
            });
        }
    }
}

fn check_agent_skill_links(
    name: &str,
    plugin: &InstalledPlugin,
    dirs: &Dirs,
    issues: &mut Vec<Issue>,
) {
    for (platform, status) in &plugin.platforms {
        if status.status != Status::Ok {
            continue;
        }

        let (check_name, base_dir, missing_message) = match platform.as_str() {
            "codex" => (
                "codex_skill",
                &dirs.codex_skills,
                "~/.codex/skills/ entry missing",
            ),
            "gemini" => (
                "gemini_skill",
                &dirs.agents_skills,
                "~/.agents/skills/ entry missing",
            ),
            _ => continue,
        };

        let link = base_dir.join(name);
        if !link.exists() {
            if platform == "codex"
                && !status.r#ref.is_empty()
                && std::path::Path::new(&status.r#ref).starts_with(&dirs.agents_skills)
                && std::path::Path::new(&status.r#ref).exists()
            {
                issues.push(Issue {
                    plugin: name.to_string(),
                    check: "legacy_codex_path",
                    severity: Severity::Warn,
                    message: "Codex still linked via legacy ~/.agents/skills path".to_string(),
                    fix: format!("nex install {name}"),
                    fix_action: FixAction::None,
                });
                continue;
            }
            issues.push(Issue {
                plugin: name.to_string(),
                check: check_name,
                severity: Severity::Warn,
                message: missing_message.to_string(),
                fix: format!("nex install {name}"),
                fix_action: FixAction::None,
            });
        } else if link.is_symlink() && fs::metadata(&link).is_err() {
            issues.push(Issue {
                plugin: name.to_string(),
                check: check_name,
                severity: Severity::Warn,
                message: "symlink target does not resolve".to_string(),
                fix: format!("nex install {name}"),
                fix_action: FixAction::None,
            });
        }
    }
}

fn check_registry_orphan(name: &str, registry: &Registry, issues: &mut Vec<Issue>) {
    if registry.get(name).is_none() {
        issues.push(Issue {
            plugin: name.to_string(),
            check: "registry",
            severity: Severity::Warn,
            message: "not found in registry (removed upstream?)".to_string(),
            fix: format!("nex uninstall {name}"),
            fix_action: FixAction::None,
        });
    }
}

fn check_stale_lock(dirs: &Dirs, issues: &mut Vec<Issue>) {
    let lock_path = dirs.lock_path();
    if !lock_path.exists() {
        return;
    }
    let Ok(meta) = fs::metadata(&lock_path) else {
        return;
    };
    let Ok(modified) = meta.modified() else {
        return;
    };
    let age = SystemTime::now()
        .duration_since(modified)
        .unwrap_or(Duration::ZERO);
    if age > Duration::from_secs(300) {
        issues.push(Issue {
            plugin: String::new(),
            check: "stale_lock",
            severity: Severity::Warn,
            message: format!(
                "lock file is {} min old (process may have died)",
                age.as_secs() / 60
            ),
            fix: format!("rm {}", lock_path.display()),
            fix_action: FixAction::RemoveFile(lock_path),
        });
    }
}

fn check_sha256(name: &str, plugin: &InstalledPlugin, dirs: &Dirs, issues: &mut Vec<Issue>) {
    let skill_dir = dirs.skills_store.join(name);
    if !skill_dir.exists() {
        return; // already caught by check_skill_dir
    }
    match hash::compute_sha256(&skill_dir) {
        Ok(current) => {
            if current != plugin.sha256 {
                issues.push(Issue {
                    plugin: name.to_string(),
                    check: "sha256",
                    severity: Severity::Warn,
                    message: format!(
                        "SHA256 drift (installed: {}…, current: {}…)",
                        &plugin.sha256[..8.min(plugin.sha256.len())],
                        &current[..8]
                    ),
                    fix: format!("nex install {name}"),
                    fix_action: FixAction::None,
                });
            }
        }
        Err(e) => {
            issues.push(Issue {
                plugin: name.to_string(),
                check: "sha256",
                severity: Severity::Warn,
                message: format!("SHA256 check failed: {e}"),
                fix: String::new(),
                fix_action: FixAction::None,
            });
        }
    }
}

fn check_nex_devtools(dirs: &Dirs, issues: &mut Vec<Issue>) {
    let nex_devtools = dirs
        .claude_plugins
        .join("marketplaces")
        .join("nex-devtools");
    if nex_devtools.exists() {
        issues.push(Issue {
            plugin: String::new(),
            check: "nex-devtools",
            severity: Severity::Warn,
            message: "nex-devtools marketplace exists (deprecated)".to_string(),
            fix: "rm -rf ~/.claude/plugins/marketplaces/nex-devtools".to_string(),
            fix_action: FixAction::RemoveDir(nex_devtools),
        });
    }
}

fn check_emporium_drift(
    catalog: &HashMap<String, cc_adapter::CatalogPlugin>,
    cc_cache: &HashMap<String, String>,
    issues: &mut Vec<Issue>,
) {
    for (name, cat) in catalog {
        if cat.version.is_empty() {
            continue;
        }
        if let Some(cached) = cc_cache.get(name) {
            if *cached != cat.version {
                issues.push(Issue {
                    plugin: name.clone(),
                    check: "emporium_drift",
                    severity: Severity::Warn,
                    message: format!("emporium=v{} but CC cache=v{cached}", cat.version),
                    fix: "restart `claude` to pull updated cache".to_string(),
                    fix_action: FixAction::None,
                });
            }
        }
    }
}

fn check_duplicate_plugins(
    catalog: &HashMap<String, cc_adapter::CatalogPlugin>,
    dev_symlinks: &HashMap<String, PathBuf>,
    dirs: &Dirs,
    issues: &mut Vec<Issue>,
) {
    for name in catalog.keys() {
        let mut locations = Vec::new();
        if dev_symlinks.contains_key(name) {
            locations.push("dev symlink".to_string());
        }
        let emporium_cache = dirs.cc_cache_dir().join("emporium").join(name);
        if emporium_cache.exists() {
            locations.push("emporium cache".to_string());
        }
        let nex_devtools = dirs
            .claude_plugins
            .join("marketplaces")
            .join("nex-devtools")
            .join("plugins")
            .join(name);
        if nex_devtools.exists() {
            locations.push("nex-devtools".to_string());
        }
        if locations.len() > 1 {
            issues.push(Issue {
                plugin: name.clone(),
                check: "duplicate",
                severity: Severity::Warn,
                message: format!(
                    "found in {} locations: {}",
                    locations.len(),
                    locations.join(", ")
                ),
                fix: "remove duplicates, keep emporium as primary".to_string(),
                fix_action: FixAction::None,
            });
        }
    }
}

fn check_stale_dev_symlinks(
    dev_symlinks: &HashMap<String, PathBuf>,
    dirs: &Dirs,
    issues: &mut Vec<Issue>,
) {
    for (name, target) in dev_symlinks {
        if !target.exists() {
            let link_path = dirs.claude_plugins.join(name);
            issues.push(Issue {
                plugin: name.clone(),
                check: "stale_symlink",
                severity: Severity::Warn,
                message: format!("dev symlink target missing: {}", target.display()),
                fix: format!("rm ~/.claude/plugins/{name}"),
                fix_action: FixAction::RemoveFile(link_path),
            });
        }
    }
}

fn check_orphan_cache(
    catalog: &HashMap<String, cc_adapter::CatalogPlugin>,
    dirs: &Dirs,
    issues: &mut Vec<Issue>,
) {
    let emporium_cache = dirs.cc_cache_dir().join("emporium");
    let Ok(entries) = fs::read_dir(&emporium_cache) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if !catalog.contains_key(&name) {
            let orphan_path = entry.path();
            issues.push(Issue {
                plugin: name.clone(),
                check: "orphan_cache",
                severity: Severity::Warn,
                message: "in CC cache but not in emporium catalog".to_string(),
                fix: format!("rm -rf ~/.claude/plugins/cache/emporium/{name}"),
                fix_action: FixAction::RemoveDir(orphan_path),
            });
        }
    }
}
