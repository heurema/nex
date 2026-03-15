use crate::core::dirs::validate_name;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;

pub fn run(name: &str, tag: Option<&str>) -> anyhow::Result<()> {
    validate_name(name)?;

    let cwd = std::env::current_dir()?;
    validate_structure(&cwd)?;

    let repo = git2::Repository::open(&cwd)
        .map_err(|e| anyhow::anyhow!("not a git repository: {e}"))?;

    let repo_url = extract_origin_url(&repo)?;
    let version = resolve_version(&repo, &cwd, tag)?;
    let sha256 = compute_sha256_git_tree(&repo)?;
    let (description, category) = extract_metadata(&cwd)?;
    let platforms = detect_platforms(&cwd);

    let entry = serde_json::json!({
        "name": name,
        "repo": repo_url,
        "version": version,
        "sha256": sha256,
        "description": description,
        "platforms": platforms,
        "category": category,
    });

    println!("{}", serde_json::to_string_pretty(&entry)?);
    Ok(())
}

fn validate_structure(dir: &Path) -> anyhow::Result<()> {
    let skill_md = dir.join("SKILL.md");
    if !skill_md.exists() {
        anyhow::bail!("SKILL.md not found at plugin root — required for publish");
    }
    let platforms_dir = dir.join("platforms");
    if !platforms_dir.exists() || !platforms_dir.is_dir() {
        anyhow::bail!("platforms/ directory not found — required for publish");
    }
    // Require at least one recognized platform subdirectory
    let recognized = ["claude-code", "codex", "gemini"];
    let has_platform = recognized.iter().any(|p| platforms_dir.join(p).is_dir());
    if !has_platform {
        anyhow::bail!(
            "platforms/ has no recognized subdirectory (claude-code, codex, gemini) — required for publish"
        );
    }
    Ok(())
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

fn strip_credentials(url: &str) -> String {
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
