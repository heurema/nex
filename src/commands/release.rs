use crate::core::{
    changelog,
    config::{self, expand_placeholders, ResolvedConfig},
    docs_sync,
    marketplace::{self, MarketplaceRef},
};
use chrono::Datelike;
use semver::Version;
use std::path::{Path, PathBuf};
use std::process::Command;

/// bump level for the release
#[derive(Debug, Clone, Copy)]
pub enum BumpLevel {
    Major,
    Minor,
    Patch,
}

impl BumpLevel {
    pub fn parse(s: &str) -> anyhow::Result<Self> {
        match s.to_lowercase().as_str() {
            "major" => Ok(Self::Major),
            "minor" => Ok(Self::Minor),
            "patch" => Ok(Self::Patch),
            other => anyhow::bail!(
                "unknown bump level '{}'; use major, minor, or patch",
                other
            ),
        }
    }

    /// Detect bump level from conventional commit messages since last tag.
    /// Returns (level, reason) for display in dry-run.
    pub fn detect(plugin_root: &Path) -> (Self, String) {
        let prev_tag = changelog::find_previous_tag(plugin_root);
        let commits = changelog::collect_commits(plugin_root, prev_tag.as_deref());

        if commits.is_empty() {
            return (Self::Patch, "no commits since last tag".to_string());
        }

        let mut has_breaking = false;
        let mut has_feat = false;
        let mut reasons: Vec<String> = Vec::new();

        for msg in &commits {
            let lower = msg.to_lowercase();
            if lower.starts_with("breaking:") || lower.contains("breaking change") || msg.starts_with("!:") {
                has_breaking = true;
                reasons.push(format!("BREAKING: {}", &msg[..msg.len().min(60)]));
            } else if lower.starts_with("feat:") || lower.starts_with("feat(") {
                has_feat = true;
                if reasons.len() < 3 {
                    reasons.push(msg[..msg.len().min(60)].to_string());
                }
            }
        }

        if has_breaking {
            (Self::Major, format!("detected BREAKING ({})", reasons.join("; ")))
        } else if has_feat {
            (Self::Minor, format!("detected feat commits ({})", reasons.join("; ")))
        } else {
            let sample = commits.first().map(|s| s[..s.len().min(50)].to_string()).unwrap_or_default();
            (Self::Patch, format!("{} commit(s), no feat/breaking (e.g. {})", commits.len(), sample))
        }
    }

    fn apply(&self, v: &Version) -> Version {
        match self {
            Self::Major => Version::new(v.major + 1, 0, 0),
            Self::Minor => Version::new(v.major, v.minor + 1, 0),
            Self::Patch => Version::new(v.major, v.minor, v.patch + 1),
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub fn run(
    level: &str,
    execute: bool,
    explicit_version: Option<&str>,
    cli_marketplace: Option<&str>,
    cli_tag_format: Option<&str>,
    no_propagate: bool,
    no_changelog: bool,
    plugin_path: Option<&str>,
    verbose: bool,
) -> anyhow::Result<()> {
    let dry_run = !execute;

    // Resolve plugin root
    let plugin_root: PathBuf = if let Some(p) = plugin_path {
        PathBuf::from(p).canonicalize().map_err(|e| {
            anyhow::anyhow!("plugin path '{}' is invalid: {e}", p)
        })?
    } else {
        std::env::current_dir()?
    };

    // Load configs
    let nex_home = dirs::home_dir()
        .ok_or_else(|| anyhow::anyhow!("cannot determine home directory"))?
        .join(".nex");
    let global_cfg = config::load_global(&nex_home)?;
    let plugin_cfg = config::load_plugin(&plugin_root)?;

    let resolved = config::resolve(
        &global_cfg,
        &plugin_cfg,
        cli_tag_format,
        cli_marketplace,
        no_propagate,
        no_changelog,
        Some(&plugin_root),
    )?;

    // ── PREFLIGHT ─────────────────────────────────────────────────────────

    let plugin_name = read_plugin_name(&plugin_root)?;

    // Open repo
    let repo = git2::Repository::open(&plugin_root)
        .map_err(|e| anyhow::anyhow!("not a git repository: {e}"))?;

    // Working tree must be clean
    preflight_clean_tree(&repo)?;

    // Resolve version files and read current version
    let current_version = preflight_version_files(&plugin_root, &resolved)?;

    // Compute next version
    let next_version: Version = if let Some(v) = explicit_version {
        v.parse::<Version>()
            .map_err(|e| anyhow::anyhow!("invalid explicit version '{}': {e}", v))?
    } else if level == "auto" {
        let (detected, reason) = BumpLevel::detect(&plugin_root);
        println!("  Auto-detect: {detected:?} ({reason})");
        detected.apply(&current_version)
    } else {
        let level = BumpLevel::parse(level)?;
        level.apply(&current_version)
    };

    // Next > current (no downgrade without --force)
    if next_version <= current_version {
        anyhow::bail!(
            "next version {} is not greater than current {} (use --version to override)",
            next_version,
            current_version
        );
    }

    // Resolve tag string
    let tag = expand_placeholders(
        &resolved.tag_format,
        &plugin_name,
        &next_version.to_string(),
        "",
        "",
    );

    // Tag must not already exist
    preflight_tag_absent(&repo, &tag)?;

    // Remote must be accessible
    preflight_remote_accessible(&repo, &resolved.git_remote)?;

    // Resolve branch
    let branch = if resolved.git_branch.is_empty() {
        detect_branch_from_repo(&repo, &resolved.git_remote)
    } else {
        resolved.git_branch.clone()
    };

    // Marketplace resolution
    let mp_ref: Option<MarketplaceRef> = if let Some(ref mp_name) = resolved.marketplace {
        match &resolved.marketplace_config {
            Some(mp_cfg) => {
                let entry = resolved
                    .marketplace_entry
                    .as_deref()
                    .unwrap_or(&plugin_name);
                let mp = MarketplaceRef::new(mp_name.clone(), mp_cfg.clone(), resolved.git_remote.clone());
                if !dry_run {
                    // Skip validate — propagate handles both update and auto-add for new plugins
                    // Marketplace must be clean — warn and skip PROPAGATE if dirty
                    if !marketplace::is_clean(&mp)? {
                        eprintln!(
                            "warning: marketplace '{}' has uncommitted changes; PROPAGATE skipped",
                            mp_name
                        );
                        None
                    } else {
                        Some(mp)
                    }
                } else {
                    Some(mp)
                }
            }
            None => {
                // Marketplace name configured but not in global config
                eprintln!(
                    "info: marketplace '{}' not found in ~/.nex/config.toml; PROPAGATE skipped",
                    mp_name
                );
                None
            }
        }
    } else {
        None
    };

    // ── Print plan (always) ────────────────────────────────────────────────

    let mode_label = if dry_run { "(dry-run)" } else { "" };
    println!();
    println!("nex release {next_version} {mode_label}");
    println!();
    println!("  Plugin:    {plugin_name}");
    println!("  Version:   {current_version} -> {next_version}");
    println!("  Config:    {}", resolved_config_source(&plugin_root));
    if let Some(ref mp) = mp_ref {
        println!("  Marketplace: {} ({})", mp.name, mp.config.path);
    } else if resolved.marketplace.is_some() {
        println!("  Marketplace: (skipped — not in global config)");
    } else {
        println!("  Marketplace: (none)");
    }
    println!();
    println!("  Steps:");
    println!("    ✓ PREFLIGHT   clean tree, version_files ok");

    if !resolved.pre_release_hooks.is_empty() {
        println!("    → HOOKS(pre)  {} command(s)", resolved.pre_release_hooks.len());
    }
    for vf in &resolved.version_files {
        println!("    → BUMP        {}", vf.path);
    }
    if resolved.changelog_mode != "skip" {
        println!(
            "    → CHANGELOG   {} [{}]",
            resolved.changelog_filename, resolved.changelog_mode
        );
    }

    println!("    → DOCS        README.md version, SKILL.md description sync");

    let commit_msg = expand_placeholders(
        &resolved.commit_format,
        &plugin_name,
        &next_version.to_string(),
        &tag,
        resolved.marketplace.as_deref().unwrap_or(""),
    );
    println!("    → COMMIT      \"{}\"", commit_msg);
    println!("    → TAG         {tag}");
    println!(
        "    → PUSH        {}/{} (exact refs)",
        resolved.git_remote, branch
    );
    if mp_ref.is_some() {
        println!(
            "    → PROPAGATE   {} marketplace ref -> {}",
            resolved.marketplace.as_deref().unwrap_or("?"),
            tag
        );
    }
    println!("    → PUBLISH     update local registry");
    if !resolved.post_release_hooks.is_empty() {
        println!(
            "    → HOOKS(post) {} command(s)",
            resolved.post_release_hooks.len()
        );
    }

    if dry_run {
        println!();
        println!("  Pass --execute to run.");
        println!();
        return Ok(());
    }

    // ── Execute pipeline ───────────────────────────────────────────────────

    println!();

    // HOOKS (pre_release)
    run_hooks(&resolved.pre_release_hooks, &plugin_root, &plugin_name, &next_version.to_string(), &tag,
        resolved.marketplace.as_deref().unwrap_or(""), true, verbose)?;

    // BUMP
    let mut modified_files: Vec<String> = Vec::new();
    for vf in &resolved.version_files {
        bump_version_file(&plugin_root.join(&vf.path), &vf.format, &current_version, &next_version,
            vf.pattern.as_deref(), vf.replace.as_deref())?;
        modified_files.push(vf.path.clone());
        println!("  [OK] BUMP        {}", vf.path);
    }

    // CHANGELOG
    if resolved.changelog_mode != "skip" {
        let changelog_path = plugin_root.join(&resolved.changelog_filename);
        let today = {
            let now = chrono::Local::now();
            format!("{}-{:02}-{:02}", now.year(), now.month(), now.day())
        };
        let changed = if resolved.changelog_mode == "auto" {
            changelog::insert_auto_section(
                &changelog_path,
                &plugin_root,
                &next_version.to_string(),
                &today,
            )?
        } else {
            changelog::insert_template_section(
                &changelog_path,
                &next_version.to_string(),
                &today,
            )?
        };
        if changed {
            modified_files.push(resolved.changelog_filename.clone());
            println!("  [OK] CHANGELOG   inserted [{}] section", next_version);
        } else if !changelog_path.exists() {
            println!("  --   CHANGELOG   {} not found, skipped", resolved.changelog_filename);
        } else {
            println!("  --   CHANGELOG   section already exists");
        }
    }

    // DOCS SYNC
    if docs_sync::sync_readme_version(&plugin_root, &current_version.to_string(), &next_version.to_string())? {
        modified_files.push("README.md".to_string());
        println!("  [OK] DOCS        README.md version updated");
    }
    if docs_sync::sync_skill_descriptions(&plugin_root)? {
        // Find which SKILL.md files were modified
        let skills_dir = plugin_root.join("skills");
        if skills_dir.exists() {
            for entry in std::fs::read_dir(&skills_dir).into_iter().flatten().flatten() {
                let skill_md = entry.path().join("SKILL.md");
                if skill_md.exists() {
                    let rel = skill_md.strip_prefix(&plugin_root)
                        .map(|p| p.to_string_lossy().to_string())
                        .unwrap_or_default();
                    if !rel.is_empty() && !modified_files.contains(&rel) {
                        modified_files.push(rel);
                    }
                }
            }
        }
        println!("  [OK] DOCS        SKILL.md descriptions synced");
    }

    // COMMIT
    git_stage_and_commit(&repo, &plugin_root, &modified_files, &commit_msg, verbose)?;
    println!("  [OK] COMMIT      \"{}\"", commit_msg);

    // TAG
    git_create_tag(&repo, &tag, resolved.tag_annotated, &resolved.tag_message,
        &plugin_name, &next_version.to_string())?;
    println!("  [OK] TAG         {tag}");

    // PUSH — exact refs, never --tags
    let tag_refspec = format!("refs/tags/{tag}");
    let branch_refspec = format!("HEAD:refs/heads/{branch}");
    git_push_exact(&repo, &resolved.git_remote, &branch_refspec, &tag_refspec, verbose)?;
    println!("  [OK] PUSH        {}/{}", resolved.git_remote, branch);

    // PROPAGATE
    if let Some(ref mp) = mp_ref {
        let entry = resolved
            .marketplace_entry
            .as_deref()
            .unwrap_or(&plugin_name);
        match marketplace::propagate(
            mp,
            &plugin_name,
            entry,
            &next_version.to_string(),
            &tag,
            false,
            Some(&plugin_root),
        ) {
            Ok(()) => {}
            Err(e) => {
                eprintln!("warning: PROPAGATE failed ({e}); tag is already pushed");
            }
        }
    } else {
        println!("  --   PROPAGATE   skipped");
    }

    // PUBLISH — update local registry (plugins only)
    let has_plugin_json = plugin_root.join(".claude-plugin/plugin.json").exists();
    let has_skill_md = plugin_root.join("SKILL.md").exists();
    if has_plugin_json || has_skill_md {
        match crate::commands::publish::compute_entry(&plugin_name, &plugin_root, Some(&tag)) {
            Ok(pub_entry) => {
                let reg_path = nex_home.join("registry.json");
                match crate::commands::publish::write_to_registry(&pub_entry, &reg_path) {
                    Ok(()) => println!("  [OK] PUBLISH     registry updated"),
                    Err(e) => eprintln!("  --   PUBLISH     failed: {e}"),
                }
            }
            Err(e) => eprintln!("  --   PUBLISH     failed: {e}"),
        }
    } else {
        println!("  --   PUBLISH     skipped (not a plugin)");
    }

    // HOOKS (post_release) — non-zero is a warning only
    if let Err(e) = run_hooks(
        &resolved.post_release_hooks,
        &plugin_root,
        &plugin_name,
        &next_version.to_string(),
        &tag,
        resolved.marketplace.as_deref().unwrap_or(""),
        false,
        verbose,
    ) {
        eprintln!("warning: post_release hook failed ({e})");
    }

    println!();
    println!("Released {} {next_version}", plugin_name);
    Ok(())
}

// ── PREFLIGHT helpers ─────────────────────────────────────────────────────────

fn preflight_clean_tree(repo: &git2::Repository) -> anyhow::Result<()> {
    let mut opts = git2::StatusOptions::new();
    opts.include_untracked(false)
        .include_ignored(false);
    let statuses = repo
        .statuses(Some(&mut opts))
        .map_err(|e| anyhow::anyhow!("cannot read git status: {e}"))?;
    if !statuses.is_empty() {
        let mut dirty: Vec<String> = Vec::new();
        for entry in statuses.iter() {
            if let Some(path) = entry.path() {
                dirty.push(path.to_string());
            }
        }
        anyhow::bail!(
            "dirty working tree — commit or stash changes first:\n  {}",
            dirty.join("\n  ")
        );
    }
    Ok(())
}

fn preflight_version_files(
    plugin_root: &Path,
    resolved: &ResolvedConfig,
) -> anyhow::Result<Version> {
    let mut versions: Vec<(String, Version)> = Vec::new();
    let mut errors: Vec<String> = Vec::new();

    for vf in &resolved.version_files {
        let path = plugin_root.join(&vf.path);
        if !path.exists() {
            errors.push(format!("missing: {}", vf.path));
            continue;
        }
        match read_version_from_file(&path, &vf.format, vf.pattern.as_deref()) {
            Ok(v) => versions.push((vf.path.clone(), v)),
            Err(e) => errors.push(format!("{}: {e}", vf.path)),
        }
    }

    if !errors.is_empty() {
        anyhow::bail!(
            "PREFLIGHT failed — version file issues:\n  {}",
            errors.join("\n  ")
        );
    }

    if versions.is_empty() {
        // No version files configured — check plugin.json as fallback
        anyhow::bail!("no version_files configured and no plugin.json found");
    }

    // All carriers must agree
    let (first_path, first_ver) = &versions[0];
    let mismatches: Vec<String> = versions
        .iter()
        .skip(1)
        .filter(|(_, v)| v != first_ver)
        .map(|(p, v)| format!("{} says {} but {} says {}", p, v, first_path, first_ver))
        .collect();

    if !mismatches.is_empty() {
        anyhow::bail!(
            "PREFLIGHT failed — version carriers disagree:\n  {}",
            mismatches.join("\n  ")
        );
    }

    Ok(first_ver.clone())
}

fn read_version_from_file(
    path: &Path,
    format: &str,
    pattern: Option<&str>,
) -> anyhow::Result<Version> {
    let content = std::fs::read_to_string(path)?;
    match format {
        "json" => {
            let v: serde_json::Value = serde_json::from_str(&content)
                .map_err(|e| anyhow::anyhow!("JSON parse error: {e}"))?;
            let ver_str = v
                .get("version")
                .and_then(|x| x.as_str())
                .ok_or_else(|| anyhow::anyhow!("no 'version' string field"))?;
            ver_str
                .parse::<Version>()
                .map_err(|e| anyhow::anyhow!("invalid semver '{}': {e}", ver_str))
        }
        "toml" => {
            let table: toml::Value = content
                .parse::<toml::Value>()
                .map_err(|e| anyhow::anyhow!("TOML parse error: {e}"))?;
            let ver_str = table
                .get("package")
                .and_then(|p| p.get("version"))
                .and_then(|v| v.as_str())
                .or_else(|| table.get("version").and_then(|v| v.as_str()))
                .ok_or_else(|| anyhow::anyhow!("no 'version' field in TOML"))?;
            ver_str
                .parse::<Version>()
                .map_err(|e| anyhow::anyhow!("invalid semver '{}': {e}", ver_str))
        }
        "regex" => {
            let pat_str = pattern
                .ok_or_else(|| anyhow::anyhow!("format=regex requires 'pattern' field"))?;
            let re = regex_version_extract(pat_str, &content)?;
            re.parse::<Version>()
                .map_err(|e| anyhow::anyhow!("invalid semver '{}': {e}", re))
        }
        other => anyhow::bail!("unknown version_file format '{}'; use json, toml, or regex", other),
    }
}

fn regex_version_extract(pattern: &str, content: &str) -> anyhow::Result<String> {
    // Use pattern as a substring filter: only scan lines containing the pattern string.
    // Extract the first semver (d.d.d) from the first matching line.
    for line in content.lines() {
        if line.contains(pattern) {
            if let Some(v) = extract_semver_from_line(line) {
                return Ok(v);
            }
        }
    }
    anyhow::bail!("no semver found in file matching pattern '{}'", pattern)
}

fn extract_semver_from_line(line: &str) -> Option<String> {
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i].is_ascii_digit() {
            let start = i;
            while i < bytes.len() && bytes[i].is_ascii_digit() { i += 1; }
            if i < bytes.len() && bytes[i] == b'.' {
                i += 1;
                let d2_start = i;
                while i < bytes.len() && bytes[i].is_ascii_digit() { i += 1; }
                if i > d2_start && i < bytes.len() && bytes[i] == b'.' {
                    i += 1;
                    let d3_start = i;
                    while i < bytes.len() && bytes[i].is_ascii_digit() { i += 1; }
                    if i > d3_start {
                        return Some(line[start..i].to_string());
                    }
                }
            }
        } else {
            i += 1;
        }
    }
    None
}

fn preflight_tag_absent(repo: &git2::Repository, tag: &str) -> anyhow::Result<()> {
    let tag_ref = format!("refs/tags/{tag}");
    if repo.find_reference(&tag_ref).is_ok() {
        anyhow::bail!(
            "tag '{}' already exists; bump version or delete the tag first",
            tag
        );
    }
    Ok(())
}

fn preflight_remote_accessible(
    repo: &git2::Repository,
    remote_name: &str,
) -> anyhow::Result<()> {
    repo.find_remote(remote_name)
        .map_err(|e| anyhow::anyhow!("remote '{}' not found: {e}", remote_name))?;
    // We don't do a full ls-remote (requires network); the push step will catch unreachable remotes.
    Ok(())
}

fn detect_branch_from_repo(repo: &git2::Repository, remote: &str) -> String {
    // Try refs/remotes/{remote}/HEAD symbolic target
    let ref_name = format!("refs/remotes/{remote}/HEAD");
    if let Ok(reference) = repo.find_reference(&ref_name) {
        if let Some(target) = reference.symbolic_target() {
            if let Some(branch) = target.rsplit('/').next() {
                if !branch.is_empty() {
                    return branch.to_string();
                }
            }
        }
    }
    // Fallback: current HEAD shorthand
    if let Ok(head) = repo.head() {
        if let Some(name) = head.shorthand() {
            if !name.is_empty() && name != "HEAD" {
                return name.to_string();
            }
        }
    }
    "main".to_string()
}

// ── BUMP helpers ──────────────────────────────────────────────────────────────

fn bump_version_file(
    path: &Path,
    format: &str,
    current: &Version,
    next: &Version,
    pattern: Option<&str>,
    replace: Option<&str>,
) -> anyhow::Result<()> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| anyhow::anyhow!("failed to read {}: {e}", path.display()))?;

    let updated = match format {
        "json" => bump_json(&content, &next.to_string())?,
        "toml" => bump_toml(&content, &next.to_string())?,
        "regex" => bump_regex(&content, current, next, pattern, replace)?,
        other => anyhow::bail!("unknown format '{}'", other),
    };

    std::fs::write(path, updated)
        .map_err(|e| anyhow::anyhow!("failed to write {}: {e}", path.display()))?;
    Ok(())
}

fn bump_json(content: &str, next_version: &str) -> anyhow::Result<String> {
    let mut v: serde_json::Value = serde_json::from_str(content)
        .map_err(|e| anyhow::anyhow!("JSON parse error: {e}"))?;
    if let Some(obj) = v.as_object_mut() {
        obj.insert(
            "version".to_string(),
            serde_json::Value::String(next_version.to_string()),
        );
    } else {
        anyhow::bail!("JSON root is not an object");
    }
    Ok(serde_json::to_string_pretty(&v)? + "\n")
}

fn bump_toml(content: &str, next_version: &str) -> anyhow::Result<String> {
    // Line-based replacement to preserve formatting and comments.
    // Finds [package] section first, then replaces the first `version = "..."` within it.
    let mut lines: Vec<String> = content.lines().map(|l| l.to_string()).collect();
    let mut in_package = false;
    let mut replaced = false;

    for line in &mut lines {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            if trimmed == "[package]" {
                in_package = true;
            } else if in_package {
                // Entered a new section — stop searching
                break;
            }
            continue;
        }
        if in_package && !replaced && trimmed.starts_with("version") {
            if let Some(eq_pos) = trimmed.find('=') {
                let after_eq = trimmed[eq_pos + 1..].trim();
                if after_eq.starts_with('"') {
                    *line = format!("version = \"{next_version}\"");
                    replaced = true;
                    break;
                }
            }
        }
    }

    if !replaced {
        anyhow::bail!("no 'version = \"...\"' line found under [package] in TOML file");
    }

    let mut result = lines.join("\n");
    if content.ends_with('\n') {
        result.push('\n');
    }
    Ok(result)
}

fn bump_regex(
    content: &str,
    current: &Version,
    next: &Version,
    pattern: Option<&str>,
    replace: Option<&str>,
) -> anyhow::Result<String> {
    let current_str = current.to_string();
    let next_str = next.to_string();
    let repl = replace
        .map(|r| r.replace("{version}", &next_str))
        .unwrap_or_else(|| next_str.clone());

    if let Some(pat) = pattern {
        // Use pattern as substring filter: find the first matching line and replace
        // only the version within that line, consistent with regex_version_extract.
        let mut result = String::with_capacity(content.len());
        let mut replaced = false;
        for line in content.lines() {
            if !replaced && line.contains(pat) && line.contains(&current_str) {
                result.push_str(&line.replacen(&current_str, &repl, 1));
                replaced = true;
            } else {
                result.push_str(line);
            }
            result.push('\n');
        }
        if !content.ends_with('\n') && result.ends_with('\n') {
            result.pop();
        }
        if !replaced {
            anyhow::bail!(
                "version string '{}' not found on any line matching pattern '{}' (regex mode)",
                current_str, pat
            );
        }
        Ok(result)
    } else {
        // No pattern: replace first occurrence anywhere in file
        if !content.contains(&current_str) {
            anyhow::bail!(
                "version string '{}' not found in file (regex mode)",
                current_str
            );
        }
        Ok(content.replacen(&current_str, &repl, 1))
    }
}

// ── GIT helpers ───────────────────────────────────────────────────────────────

/// Read project name. Priority: .nex/release.toml name > plugin.json > Cargo.toml
fn read_plugin_name(plugin_root: &Path) -> anyhow::Result<String> {
    // 1. .nex/release.toml "name" field
    let release_toml = plugin_root.join(".nex/release.toml");
    if release_toml.exists() {
        if let Ok(content) = std::fs::read_to_string(&release_toml) {
            if let Ok(cfg) = toml::from_str::<config::PluginReleaseConfig>(&content) {
                if let Some(name) = cfg.name {
                    if !name.is_empty() {
                        return Ok(name);
                    }
                }
            }
        }
    }

    // 2. .claude-plugin/plugin.json "name" field
    let plugin_json = plugin_root.join(".claude-plugin/plugin.json");
    if plugin_json.exists() {
        let content = std::fs::read_to_string(&plugin_json)?;
        let v: serde_json::Value = serde_json::from_str(&content)
            .map_err(|e| anyhow::anyhow!("failed to parse plugin.json: {e}"))?;
        if let Some(name) = v.get("name").and_then(|n| n.as_str()) {
            if !name.is_empty() {
                return Ok(name.to_string());
            }
        }
    }

    // 3. Cargo.toml [package].name
    let cargo_toml = plugin_root.join("Cargo.toml");
    if cargo_toml.exists() {
        let content = std::fs::read_to_string(&cargo_toml)?;
        let table: toml::Value = content.parse::<toml::Value>()
            .map_err(|e| anyhow::anyhow!("failed to parse Cargo.toml: {e}"))?;
        if let Some(name) = table.get("package")
            .and_then(|p| p.get("name"))
            .and_then(|n| n.as_str())
        {
            return Ok(name.to_string());
        }
    }

    anyhow::bail!(
        "cannot determine project name in {}\n  Add 'name' to .nex/release.toml, or provide plugin.json or Cargo.toml",
        plugin_root.display()
    )
}

fn git_stage_and_commit(
    repo: &git2::Repository,
    plugin_root: &Path,
    files: &[String],
    message: &str,
    _verbose: bool,
) -> anyhow::Result<()> {
    let mut index = repo.index()
        .map_err(|e| anyhow::anyhow!("cannot open git index: {e}"))?;

    for file in files {
        let abs = plugin_root.join(file);
        if abs.exists() {
            // path relative to repo workdir
            let rel = pathdiff::diff_paths(&abs, plugin_root)
                .unwrap_or_else(|| abs.clone());
            index.add_path(&rel)
                .map_err(|e| anyhow::anyhow!("git add '{}' failed: {e}", file))?;
        }
    }
    index.write()
        .map_err(|e| anyhow::anyhow!("cannot write index: {e}"))?;

    let tree_id = index.write_tree()
        .map_err(|e| anyhow::anyhow!("cannot write tree: {e}"))?;
    let tree = repo.find_tree(tree_id)
        .map_err(|e| anyhow::anyhow!("cannot find tree: {e}"))?;

    let sig = repo.signature().map_err(|_| {
        anyhow::anyhow!("git signature not configured; set user.name and user.email")
    })?;

    let parent_commit = repo
        .head()
        .and_then(|h| h.peel_to_commit())
        .ok();

    let parents: Vec<&git2::Commit> = parent_commit.iter().collect();
    repo.commit(Some("HEAD"), &sig, &sig, message, &tree, &parents)
        .map_err(|e| anyhow::anyhow!("git commit failed: {e}"))?;

    Ok(())
}

fn git_create_tag(
    repo: &git2::Repository,
    tag: &str,
    annotated: bool,
    tag_message_template: &str,
    plugin_name: &str,
    version: &str,
) -> anyhow::Result<()> {
    let head = repo.head()
        .map_err(|e| anyhow::anyhow!("cannot read HEAD: {e}"))?;
    let obj = head.peel(git2::ObjectType::Commit)
        .map_err(|e| anyhow::anyhow!("HEAD is not a commit: {e}"))?;

    if annotated {
        let msg = expand_placeholders(tag_message_template, plugin_name, version, tag, "");
        let sig = repo.signature()?;
        repo.tag(tag, &obj, &sig, &msg, false)
            .map_err(|e| anyhow::anyhow!("git tag '{}' failed: {e}", tag))?;
    } else {
        repo.tag_lightweight(tag, &obj, false)
            .map_err(|e| anyhow::anyhow!("git tag '{}' failed: {e}", tag))?;
    }
    Ok(())
}

/// Push exact refs: never uses --tags.
/// Pushes `branch_refspec` (HEAD:refs/heads/{branch}) AND `refs/tags/{tag}`.
fn git_push_exact(
    repo: &git2::Repository,
    remote_name: &str,
    branch_refspec: &str,
    tag_refspec: &str,
    _verbose: bool,
) -> anyhow::Result<()> {
    // Verify remote exists via git2
    repo.find_remote(remote_name)
        .map_err(|e| anyhow::anyhow!("remote '{}' not found: {e}", remote_name))?;

    // Use system git for push — better SSH auth on macOS (keychain, agent)
    let workdir = repo
        .workdir()
        .ok_or_else(|| anyhow::anyhow!("bare repository"))?;

    let status = Command::new("git")
        .args(["push", remote_name, branch_refspec, tag_refspec])
        .current_dir(workdir)
        .status()
        .map_err(|e| anyhow::anyhow!("failed to run git push: {e}"))?;

    if !status.success() {
        anyhow::bail!(
            "git push failed (exit {})",
            status.code().unwrap_or(-1)
        );
    }
    Ok(())
}

// ── Hooks ─────────────────────────────────────────────────────────────────────

fn has_shell_metachar(s: &str) -> bool {
    s.chars().any(|c| matches!(c, ';' | '|' | '&' | '$' | '`' | '\\' | '"' | '\'' | '<' | '>' | '(' | ')' | '{' | '}'))
}

fn run_hooks(
    hooks: &[String],
    plugin_root: &Path,
    plugin_name: &str,
    version: &str,
    tag: &str,
    marketplace: &str,
    abort_on_failure: bool,
    verbose: bool,
) -> anyhow::Result<()> {
    // Prevent shell injection via placeholders — reject shell metacharacters only
    if has_shell_metachar(plugin_name) {
        eprintln!("warning: skipping hooks — plugin name contains shell metacharacters");
        return Ok(());
    }
    // Validate version matches semver pattern (digits.digits.digits prefix)
    if !version.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false)
        || version.splitn(3, '.').count() < 3
    {
        eprintln!("warning: skipping hooks — invalid version string: '{version}'");
        return Ok(());
    }
    let hook_type = if abort_on_failure { "pre_release" } else { "post_release" };
    for hook in hooks {
        let expanded = expand_placeholders(hook, plugin_name, version, tag, marketplace);
        if verbose {
            eprintln!("  [hook] {}", expanded);
        }
        let status = Command::new("sh")
            .arg("-c")
            .arg(&expanded)
            .current_dir(plugin_root)
            .status()
            .map_err(|e| anyhow::anyhow!("failed to spawn hook '{}': {e}", expanded))?;

        if !status.success() {
            let msg = format!(
                "{} hook failed (exit {}): {}",
                hook_type,
                status.code().unwrap_or(-1),
                expanded
            );
            if abort_on_failure {
                anyhow::bail!("{}", msg);
            } else {
                eprintln!("warning: {msg}");
            }
        }
    }
    Ok(())
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn resolved_config_source(plugin_root: &Path) -> String {
    let release_toml = plugin_root.join(".nex/release.toml");
    if release_toml.exists() {
        ".nex/release.toml (found)".to_string()
    } else {
        ".nex/release.toml (not found, using defaults)".to_string()
    }
}
