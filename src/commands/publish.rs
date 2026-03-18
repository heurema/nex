use crate::core::dirs::{validate_name, Dirs};
use crate::core::registry::{Package, Registry};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;

pub fn run(name: &str, tag: Option<&str>) -> anyhow::Result<()> {
    validate_name(name)?;
    let cwd = std::env::current_dir()?;
    let entry = compute_entry(name, &cwd, tag)?;

    let json = serde_json::json!({
        "name": entry.name,
        "repo": entry.repo,
        "version": entry.version,
        "sha256": entry.sha256,
        "description": entry.description,
        "platforms": entry.platforms,
        "category": entry.category,
    });
    println!("{}", serde_json::to_string_pretty(&json)?);

    // Auto-write to local registry
    let dirs = Dirs::new()?;
    write_to_registry(&entry, &dirs.registry_path())?;
    eprintln!("Registry updated: {} v{}", entry.name, entry.version);

    Ok(())
}

/// Computed publish entry — all metadata needed for registry.
pub struct PublishEntry {
    pub name: String,
    pub repo: String,
    pub version: String,
    pub sha256: String,
    pub description: String,
    pub platforms: Vec<String>,
    pub category: String,
    pub release_class: String,
}

/// Compute publish entry from a plugin directory.
pub fn compute_entry(name: &str, plugin_dir: &Path, tag: Option<&str>) -> anyhow::Result<PublishEntry> {
    validate_name(name)?;
    let format = detect_format(plugin_dir)?;

    let repo = git2::Repository::open(plugin_dir)
        .map_err(|e| anyhow::anyhow!("not a git repository: {e}"))?;

    let repo_url = extract_origin_url(&repo)?;
    let version = resolve_version(&repo, plugin_dir, tag)?;
    let sha256 = compute_sha256_git_tree(&repo)?;
    let (description, category) = extract_metadata(plugin_dir)?;
    let platforms = match format {
        PluginFormat::Universal => detect_platforms(plugin_dir),
        PluginFormat::ClaudeCodeOnly => vec!["claude-code".to_string()],
    };

    let release_class = match format {
        PluginFormat::ClaudeCodeOnly => "legacy".to_string(),
        PluginFormat::Universal => {
            let all_three = ["claude-code", "codex", "gemini"]
                .iter()
                .all(|p| platforms.iter().any(|pl| pl == p));
            if all_three {
                "universal".to_string()
            } else {
                "partial".to_string()
            }
        }
    };

    Ok(PublishEntry {
        name: name.to_string(),
        repo: repo_url,
        version,
        sha256,
        description,
        platforms,
        category,
        release_class,
    })
}

/// Write a publish entry to the local registry (upsert).
/// Loads existing registry to preserve all packages — never overwrites with partial data.
pub fn write_to_registry(entry: &PublishEntry, registry_path: &Path) -> anyhow::Result<()> {
    // Load existing registry (local file or fetched cache) to preserve all packages
    let mut reg = if registry_path.exists() {
        Registry::load_local(registry_path)?
    } else {
        // Try network fetch to bootstrap with full package list
        Registry::load(registry_path, false).unwrap_or_else(|_| Registry::load_local(registry_path).unwrap_or_else(|_| Registry { version: 2, packages: std::collections::HashMap::new() }))
    };
    reg.upsert(
        entry.name.clone(),
        Package {
            repo: entry.repo.clone(),
            version: entry.version.clone(),
            sha256: entry.sha256.clone(),
            description: entry.description.clone(),
            platforms: entry.platforms.clone(),
            category: entry.category.clone(),
            release_class: Some(entry.release_class.clone()),
            rubric_score: None,
            rubric_max: None,
        },
    );
    reg.save(registry_path)?;
    Ok(())
}

/// Detect plugin format: Universal (SKILL.md + platforms/) or CC-only (.claude-plugin/)
enum PluginFormat {
    /// SKILL.md root + platforms/ with at least one recognized subdir
    Universal,
    /// Claude Code only: .claude-plugin/plugin.json present, no platforms/
    ClaudeCodeOnly,
}

fn detect_format(dir: &Path) -> anyhow::Result<PluginFormat> {
    let has_skill_md = dir.join("SKILL.md").exists();
    let has_platforms = dir.join("platforms").is_dir();
    let has_cc_plugin = dir.join(".claude-plugin/plugin.json").exists();

    if has_skill_md && has_platforms {
        // Verify at least one recognized platform
        let recognized = ["claude-code", "codex", "gemini"];
        let has_any = recognized.iter().any(|p| dir.join("platforms").join(p).is_dir());
        if !has_any {
            anyhow::bail!(
                "platforms/ has no recognized subdirectory (claude-code, codex, gemini)"
            );
        }
        Ok(PluginFormat::Universal)
    } else if has_cc_plugin {
        eprintln!("Detected Claude Code-only plugin (no SKILL.md or platforms/)");
        eprintln!("Tip: run `nex convert` to migrate to universal format");
        Ok(PluginFormat::ClaudeCodeOnly)
    } else {
        anyhow::bail!(
            "Not a valid plugin. Need either:\n  \
             - SKILL.md + platforms/ (universal format)\n  \
             - .claude-plugin/plugin.json (Claude Code format)"
        );
    }
}

fn extract_origin_url(repo: &git2::Repository) -> anyhow::Result<String> {
    let remote = repo.find_remote("origin")
        .map_err(|_| anyhow::anyhow!("no 'origin' remote found in git repository"))?;
    let raw = remote.url()
        .ok_or_else(|| anyhow::anyhow!("'origin' remote has no URL"))?
        .to_string();
    // Strip embedded credentials from HTTPS URLs (e.g. https://token@github.com/...)
    let url = strip_credentials(&raw);
    Ok(url)
}

pub fn strip_credentials(url: &str) -> String {
    // Strip userinfo from any scheme://user@host URL
    for scheme in &["https://", "http://", "ssh://"] {
        if let Some(rest) = url.strip_prefix(scheme) {
            if let Some(at_pos) = rest.find('@') {
                return format!("{scheme}{}", &rest[at_pos + 1..]);
            }
        }
    }
    // git@host:path SSH URLs have no embedded credentials — keep as-is
    url.to_string()
}

fn resolve_version(repo: &git2::Repository, dir: &Path, tag: Option<&str>) -> anyhow::Result<String> {
    // Priority: plugin.json version > explicit tag > HEAD tag > HEAD commit
    let plugin_json_path = dir.join(".claude-plugin/plugin.json");
    if plugin_json_path.exists() {
        let content = fs::read_to_string(&plugin_json_path)?;
        let v: serde_json::Value = serde_json::from_str(&content)
            .map_err(|e| anyhow::anyhow!("failed to parse plugin.json: {e}"))?;
        if let Some(ver) = v.get("version").and_then(|x| x.as_str()) {
            if !ver.is_empty() {
                return Ok(ver.to_string());
            }
        }
    }

    if let Some(t) = tag {
        return Ok(t.to_string());
    }

    // Try to find a tag pointing at HEAD
    let head = repo.head()?.peel_to_commit()?;
    let head_id = head.id();
    let mut found_tag: Option<String> = None;
    repo.tag_foreach(|id, name_bytes| {
        if found_tag.is_some() {
            return true;
        }
        if let Ok(name) = std::str::from_utf8(name_bytes) {
            let tag_name = name.trim_start_matches("refs/tags/");
            if let Ok(obj) = repo.find_object(id, None) {
                let commit_id = match obj.into_tag() {
                    Ok(tag) => match tag.target() {
                        Ok(t) => Some(t.id()),
                        Err(_) => None,
                    },
                    Err(obj) => Some(obj.id()),
                };
                if commit_id == Some(head_id) {
                    found_tag = Some(tag_name.to_string());
                }
            }
        }
        true
    })?;

    if let Some(t) = found_tag {
        return Ok(t);
    }

    Ok(format!("{}", &head_id.to_string()[..8]))
}

fn extract_metadata(dir: &Path) -> anyhow::Result<(String, String)> {
    let plugin_json_path = dir.join(".claude-plugin/plugin.json");
    if plugin_json_path.exists() {
        let content = fs::read_to_string(&plugin_json_path)?;
        let v: serde_json::Value = serde_json::from_str(&content)
            .map_err(|e| anyhow::anyhow!("failed to parse plugin.json: {e}"))?;
        let desc = v.get("description")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string();
        let cat = v.get("category")
            .and_then(|x| x.as_str())
            .unwrap_or("general")
            .to_string();
        return Ok((desc, cat));
    }

    // Fallback: read first non-empty line from SKILL.md after the title
    let skill_md = dir.join("SKILL.md");
    let content = fs::read_to_string(&skill_md)?;
    let desc = content.lines()
        .skip(1)
        .find(|l| !l.trim().is_empty() && !l.starts_with('#'))
        .unwrap_or("")
        .trim()
        .to_string();

    Ok((desc, "general".to_string()))
}

fn detect_platforms(dir: &Path) -> Vec<String> {
    let mut platforms = Vec::new();
    for (subdir, label) in &[
        ("claude-code", "claude-code"),
        ("codex", "codex"),
        ("gemini", "gemini"),
    ] {
        if dir.join("platforms").join(subdir).is_dir() {
            platforms.push(label.to_string());
        }
    }
    platforms
}

/// Compute a reproducible SHA-256 over the git tree at HEAD.
/// Uses the git object database (not working-tree files) so uncommitted changes
/// do not contaminate the hash.
/// Each file entry is hashed as:
///   path_len (u64 LE) || path_bytes || content_len (u64 LE) || content_bytes
/// This length-prefixed encoding prevents path/content boundary collisions.
fn compute_sha256_git_tree(repo: &git2::Repository) -> anyhow::Result<String> {
    let head_commit = repo
        .head()
        .map_err(|e| anyhow::anyhow!("cannot read HEAD: {e}"))?
        .peel_to_commit()
        .map_err(|e| anyhow::anyhow!("HEAD is not a commit: {e}"))?;
    let root_tree = head_commit
        .tree()
        .map_err(|e| anyhow::anyhow!("cannot read commit tree: {e}"))?;

    // Collect all blob entries (path, oid) excluding .signum/
    let mut entries: Vec<(String, git2::Oid)> = Vec::new();
    collect_tree_blobs(repo, &root_tree, "", &mut entries)?;
    entries.sort_by(|a, b| a.0.cmp(&b.0));

    let mut hasher = Sha256::new();
    for (path, oid) in entries {
        let blob = repo
            .find_blob(oid)
            .map_err(|e| anyhow::anyhow!("cannot read blob {oid}: {e}"))?;
        let content = blob.content();
        let path_bytes = path.as_bytes();
        // Length-prefixed: prevents path||content boundary collisions
        hasher.update((path_bytes.len() as u64).to_le_bytes());
        hasher.update(path_bytes);
        // blob.content() is already in-memory from libgit2 — hash directly
        hasher.update((content.len() as u64).to_le_bytes());
        hasher.update(content);
    }

    Ok(format!("{:x}", hasher.finalize()))
}

fn collect_tree_blobs(
    repo: &git2::Repository,
    tree: &git2::Tree,
    prefix: &str,
    out: &mut Vec<(String, git2::Oid)>,
) -> anyhow::Result<()> {
    for entry in tree.iter() {
        let entry_name = entry
            .name()
            .ok_or_else(|| anyhow::anyhow!("tree entry has non-UTF8 name"))?;
        // Skip .signum/ directory
        if entry_name == ".signum" {
            continue;
        }
        let path = if prefix.is_empty() {
            entry_name.to_string()
        } else {
            format!("{prefix}/{entry_name}")
        };
        match entry.kind() {
            Some(git2::ObjectType::Blob) => {
                out.push((path, entry.id()));
            }
            Some(git2::ObjectType::Tree) => {
                let sub_tree = repo
                    .find_tree(entry.id())
                    .map_err(|e| anyhow::anyhow!("cannot read subtree {}: {e}", entry.id()))?;
                collect_tree_blobs(repo, &sub_tree, &path, out)?;
            }
            _ => {}
        }
    }
    Ok(())
}
