use crate::core::{dirs::Dirs, registry::Registry, state::{InstalledPlugin, InstalledState, Status}};
use std::fs;
use std::time::{Duration, SystemTime};

#[allow(dead_code)]
enum Severity { Ok, Warn, Error }

struct Issue {
    plugin: String,
    check: &'static str,
    severity: Severity,
    message: String,
    fix: String,
}

pub fn run(deep: bool) -> anyhow::Result<()> {
    let dirs = Dirs::new()?;
    let state = InstalledState::load(&dirs.installed_path())?;
    let registry = Registry::load(&dirs.registry_path(), false)?;

    let mut issues: Vec<Issue> = Vec::new();

    for (name, plugin) in &state.plugins {
        check_skill_dir(name, &dirs, &mut issues);
        check_cc_symlinks(name, plugin, &dirs, &mut issues);
        check_agentskills_symlinks(name, plugin, &dirs, &mut issues);
        check_registry_orphan(name, &registry, &mut issues);
        if deep {
            check_sha256(name, plugin, &dirs, &mut issues);
        }
    }
    check_stale_lock(&dirs, &mut issues);

    if state.plugins.is_empty() {
        println!("No plugins installed. Nothing to check.");
        return Ok(());
    }

    let total = state.plugins.len();
    let issue_count = issues.iter()
        .filter(|i| !matches!(i.severity, Severity::Ok))
        .count();

    println!("Checking {} installed plugin{}...\n",
        total, if total == 1 { "" } else { "s" });

    // Per-plugin summary
    let mut names: Vec<&String> = state.plugins.keys().collect();
    names.sort();
    for name in &names {
        let plugin_issues: Vec<&Issue> = issues.iter()
            .filter(|i| i.plugin == **name && !matches!(i.severity, Severity::Ok))
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

    // Non-plugin issues (stale lock)
    for issue in issues.iter().filter(|i| i.plugin.is_empty() && !matches!(i.severity, Severity::Ok)) {
        println!("\x1b[33m[WARN]\x1b[0m  {}: {}", issue.check, issue.message);
    }

    // Fix suggestions
    let fixable: Vec<&Issue> = issues.iter()
        .filter(|i| !i.fix.is_empty() && !matches!(i.severity, Severity::Ok))
        .collect();
    if !fixable.is_empty() {
        println!("\nSuggested fixes:");
        for issue in &fixable {
            let target = if issue.plugin.is_empty() { "—".to_string() } else { issue.plugin.clone() };
            println!("  {target}: {}", issue.fix);
        }
    }

    if issue_count == 0 {
        println!("\nAll checks passed.");
        Ok(())
    } else {
        println!("\n{issue_count} issue{} found.",
            if issue_count == 1 { "" } else { "s" });
        anyhow::bail!("doctor found {issue_count} issue(s)");
    }
}

fn check_skill_dir(name: &str, dirs: &Dirs, issues: &mut Vec<Issue>) {
    let skill_dir = dirs.skills_store.join(name);
    if !skill_dir.exists() {
        issues.push(Issue {
            plugin: name.to_string(),
            check: "skill_dir",
            severity: Severity::Error,
            message: format!("~/.skills/{name}/ missing"),
            fix: format!("nex install {name}"),
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
        let link = dirs.claude_plugins
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
            });
        }
    }
}

fn check_agentskills_symlinks(name: &str, plugin: &InstalledPlugin, dirs: &Dirs, issues: &mut Vec<Issue>) {
    let has_agent_platform = plugin.platforms.iter()
        .any(|(p, s)| (p == "codex" || p == "gemini") && s.status == Status::Ok);

    if !has_agent_platform {
        return;
    }

    let link = dirs.agents_skills.join(name);
    if !link.exists() {
        issues.push(Issue {
            plugin: name.to_string(),
            check: "agentskills",
            severity: Severity::Warn,
            message: "~/.agents/skills/ symlink missing".to_string(),
            fix: format!("nex install {name}"),
        });
    } else if link.is_symlink() && fs::metadata(&link).is_err() {
        issues.push(Issue {
            plugin: name.to_string(),
            check: "agentskills",
            severity: Severity::Warn,
            message: "symlink target does not resolve".to_string(),
            fix: format!("nex install {name}"),
        });
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
        });
    }
}

fn check_stale_lock(dirs: &Dirs, issues: &mut Vec<Issue>) {
    let lock_path = dirs.lock_path();
    if !lock_path.exists() {
        return;
    }
    let Ok(meta) = fs::metadata(&lock_path) else { return };
    let Ok(modified) = meta.modified() else { return };
    let age = SystemTime::now()
        .duration_since(modified)
        .unwrap_or(Duration::ZERO);
    if age > Duration::from_secs(300) {
        issues.push(Issue {
            plugin: String::new(),
            check: "stale_lock",
            severity: Severity::Warn,
            message: format!("lock file is {} min old (process may have died)",
                age.as_secs() / 60),
            fix: format!("rm {}", lock_path.display()),
        });
    }
}

fn check_sha256(name: &str, plugin: &InstalledPlugin, dirs: &Dirs, _issues: &mut Vec<Issue>) {
    let skill_dir = dirs.skills_store.join(name);
    if !skill_dir.exists() {
        return; // already caught by check_skill_dir
    }
    // Full SHA256 re-verify requires extracting compute_sha256 to core
    // For now, log that deep check was requested but not yet implemented
    let _ = plugin;
    eprintln!("  {name}: SHA256 deep check not yet implemented (planned for v0.7.0)");
}
