use crate::core::config::{expand_placeholders, expand_tilde, MarketplaceConfig};
use std::path::PathBuf;

pub struct MarketplaceRef {
    pub name: String,
    pub config: MarketplaceConfig,
    pub resolved_path: PathBuf,
    pub git_remote: String,
}

impl MarketplaceRef {
    pub fn new(name: String, config: MarketplaceConfig, git_remote: String) -> Self {
        let resolved_path = expand_tilde(&config.path);
        Self { name, config, resolved_path, git_remote }
    }

    /// Validate the marketplace repo: exists, is a git repo, and manifest contains plugin entry.
    pub fn validate(&self, plugin_entry: &str) -> anyhow::Result<()> {
        if !self.resolved_path.exists() {
            anyhow::bail!(
                "marketplace '{}' path does not exist: {}",
                self.name,
                self.resolved_path.display()
            );
        }
        // Must be a git repo
        git2::Repository::open(&self.resolved_path).map_err(|e| {
            anyhow::anyhow!(
                "marketplace '{}' is not a git repository ({}): {e}",
                self.name,
                self.resolved_path.display()
            )
        })?;

        // Manifest must exist and contain an entry for the plugin
        let manifest_path = self.resolved_path.join(&self.config.manifest);
        if !manifest_path.exists() {
            anyhow::bail!(
                "marketplace '{}' manifest not found: {}",
                self.name,
                manifest_path.display()
            );
        }

        let content = std::fs::read_to_string(&manifest_path)
            .map_err(|e| anyhow::anyhow!("failed to read manifest {}: {e}", manifest_path.display()))?;
        let manifest: serde_json::Value = serde_json::from_str(&content)
            .map_err(|e| anyhow::anyhow!("failed to parse manifest {}: {e}", manifest_path.display()))?;

        let found = find_plugin_entry(&manifest, plugin_entry);
        if !found {
            anyhow::bail!(
                "plugin '{}' not found in marketplace '{}' manifest ({})",
                plugin_entry,
                self.name,
                manifest_path.display()
            );
        }

        Ok(())
    }
}

/// Check that the marketplace working tree is clean.
pub fn is_clean(mp: &MarketplaceRef) -> anyhow::Result<bool> {
    let repo = git2::Repository::open(&mp.resolved_path)
        .map_err(|e| anyhow::anyhow!("cannot open marketplace repo: {e}"))?;
    let statuses = repo
        .statuses(None)
        .map_err(|e| anyhow::anyhow!("cannot read marketplace status: {e}"))?;
    Ok(statuses.is_empty())
}

/// Find the entry for `plugin_entry` in the marketplace manifest (JSON).
/// The manifest is expected to be an array of objects, each with a "name" field,
/// or a map where keys are plugin names.
fn find_plugin_entry(manifest: &serde_json::Value, plugin_entry: &str) -> bool {
    if let Some(arr) = manifest.as_array() {
        return arr.iter().any(|item| {
            item.get("name")
                .and_then(|n| n.as_str())
                .map(|n| n == plugin_entry)
                .unwrap_or(false)
        });
    }
    if let Some(obj) = manifest.as_object() {
        // Could be {"packages": [...]} or a direct map
        if obj.contains_key(plugin_entry) {
            return true;
        }
        // Try nested arrays
        for val in obj.values() {
            if val.is_array() && find_plugin_entry(val, plugin_entry) {
                return true;
            }
        }
    }
    false
}

/// Propagate a new tag reference to the marketplace manifest.
///
/// Steps:
///   1. git pull --ff-only
///   2. Update "ref" field for the plugin entry
///   3. git add <manifest>
///   4. git commit -m <message>
///   5. git push origin HEAD:refs/heads/<branch>
pub fn propagate(
    mp: &MarketplaceRef,
    plugin_name: &str,
    plugin_entry: &str,
    next_version: &str,
    tag: &str,
    dry_run: bool,
) -> anyhow::Result<()> {
    let repo = git2::Repository::open(&mp.resolved_path)
        .map_err(|e| anyhow::anyhow!("cannot open marketplace repo: {e}"))?;

    let manifest_path = mp.resolved_path.join(&mp.config.manifest);

    if dry_run {
        println!(
            "  -> PROPAGATE  {} marketplace: ref -> {}",
            mp.name, tag
        );
        return Ok(());
    }

    // 1. git pull --ff-only (best-effort; warn on failure)
    if let Err(e) = git_pull_ff(&repo, &mp.git_remote) {
        eprintln!("warning: marketplace pull failed ({e}); continuing");
    }

    // 2. Update manifest
    let content = std::fs::read_to_string(&manifest_path)
        .map_err(|e| anyhow::anyhow!("failed to read manifest: {e}"))?;
    let mut manifest: serde_json::Value = serde_json::from_str(&content)
        .map_err(|e| anyhow::anyhow!("failed to parse manifest: {e}"))?;

    update_plugin_ref(&mut manifest, plugin_entry, tag)?;

    let updated = serde_json::to_string_pretty(&manifest)? + "\n";
    std::fs::write(&manifest_path, &updated)
        .map_err(|e| anyhow::anyhow!("failed to write manifest: {e}"))?;

    // 3. git add manifest
    let manifest_rel = pathdiff::diff_paths(&manifest_path, &mp.resolved_path)
        .unwrap_or_else(|| manifest_path.clone());
    let manifest_rel_str = manifest_rel.to_string_lossy().to_string();
    git_add(&repo, &manifest_rel_str)?;

    // 4. git commit
    let commit_msg = expand_placeholders(
        &mp.config.commit_format,
        plugin_name,
        next_version,
        tag,
        &mp.name,
    );
    git_commit(&repo, &commit_msg)?;

    // 5. git push
    let branch = detect_branch(&repo, &mp.git_remote).unwrap_or_else(|_| "main".to_string());
    let push_ref = format!("HEAD:refs/heads/{branch}");
    git_push_ref(&repo, &mp.git_remote, &push_ref)?;

    println!("  [OK] PROPAGATE  {} marketplace updated to {}", mp.name, tag);
    Ok(())
}

fn git_pull_ff(repo: &git2::Repository, remote_name: &str) -> anyhow::Result<()> {
    let head = repo.head()?;
    let branch_name = head.shorthand().unwrap_or("main").to_string();
    let mut remote = repo.find_remote(remote_name)?;
    let refspec = format!("refs/heads/{branch_name}:refs/heads/{branch_name}");
    remote.fetch(&[&refspec], None, None)?;

    // Fast-forward: find FETCH_HEAD and reset
    let fetch_head = repo.find_reference("FETCH_HEAD")?;
    let fetch_commit = fetch_head.peel_to_commit()?;
    let target_oid = fetch_commit.id();

    let mut reference = repo.find_reference(&format!("refs/heads/{branch_name}"))?;
    reference.set_target(target_oid, "fast-forward")?;
    repo.set_head(&format!("refs/heads/{branch_name}"))?;
    repo.checkout_head(Some(git2::build::CheckoutBuilder::default().force()))?;
    Ok(())
}

fn git_add(repo: &git2::Repository, path: &str) -> anyhow::Result<()> {
    let mut index = repo.index()?;
    index.add_path(std::path::Path::new(path))
        .map_err(|e| anyhow::anyhow!("git add '{}' failed: {e}", path))?;
    index.write()?;
    Ok(())
}

fn git_commit(repo: &git2::Repository, message: &str) -> anyhow::Result<()> {
    let mut index = repo.index()?;
    let tree_id = index.write_tree()?;
    let tree = repo.find_tree(tree_id)?;

    let sig = repo.signature().map_err(|_| {
        anyhow::anyhow!("git signature not configured; set user.name and user.email")
    })?;

    let parent_commit = if let Ok(head) = repo.head() {
        Some(head.peel_to_commit()?)
    } else {
        None
    };

    let parents: Vec<&git2::Commit> = parent_commit.iter().collect();
    repo.commit(Some("HEAD"), &sig, &sig, message, &tree, &parents)
        .map_err(|e| anyhow::anyhow!("git commit failed: {e}"))?;
    Ok(())
}

fn git_push_ref(
    repo: &git2::Repository,
    remote_name: &str,
    refspec: &str,
) -> anyhow::Result<()> {
    // Verify remote exists via git2
    repo.find_remote(remote_name)
        .map_err(|e| anyhow::anyhow!("remote '{}' not found: {e}", remote_name))?;

    // Use system git for push — better SSH auth on macOS (keychain, agent)
    let workdir = repo
        .workdir()
        .ok_or_else(|| anyhow::anyhow!("bare repository"))?;

    let status = std::process::Command::new("git")
        .args(["push", remote_name, refspec])
        .current_dir(workdir)
        .status()
        .map_err(|e| anyhow::anyhow!("failed to run git push: {e}"))?;

    if !status.success() {
        anyhow::bail!(
            "push '{}' failed (exit {})",
            refspec,
            status.code().unwrap_or(-1)
        );
    }
    Ok(())
}

pub fn detect_branch(repo: &git2::Repository, remote_name: &str) -> anyhow::Result<String> {
    // Try symbolic-ref for the remote tracking HEAD
    let remote_head = format!("refs/remotes/{remote_name}/HEAD");
    if let Ok(reference) = repo.find_reference(&remote_head) {
        if let Some(target) = reference.symbolic_target() {
            // e.g. "refs/remotes/origin/main"
            let branch = target.rsplit('/').next().unwrap_or("main");
            return Ok(branch.to_string());
        }
    }
    // Fallback: use current HEAD shorthand
    if let Ok(head) = repo.head() {
        if let Some(name) = head.shorthand() {
            if !name.is_empty() && name != "HEAD" {
                return Ok(name.to_string());
            }
        }
    }
    Ok("main".to_string())
}

fn update_plugin_ref(
    manifest: &mut serde_json::Value,
    plugin_entry: &str,
    tag: &str,
) -> anyhow::Result<()> {
    if let Some(arr) = manifest.as_array_mut() {
        for item in arr.iter_mut() {
            if item
                .get("name")
                .and_then(|n| n.as_str())
                .map(|n| n == plugin_entry)
                .unwrap_or(false)
            {
                if let Some(obj) = item.as_object_mut() {
                    obj.insert("ref".to_string(), serde_json::Value::String(tag.to_string()));
                }
                return Ok(());
            }
        }
        anyhow::bail!("plugin '{}' not found in manifest array", plugin_entry);
    }

    if let Some(obj) = manifest.as_object_mut() {
        if let Some(entry) = obj.get_mut(plugin_entry) {
            if let Some(entry_obj) = entry.as_object_mut() {
                entry_obj.insert("ref".to_string(), serde_json::Value::String(tag.to_string()));
                return Ok(());
            }
        }
        // Try nested packages array
        for val in obj.values_mut() {
            if let Some(arr) = val.as_array_mut() {
                for item in arr.iter_mut() {
                    if item
                        .get("name")
                        .and_then(|n| n.as_str())
                        .map(|n| n == plugin_entry)
                        .unwrap_or(false)
                    {
                        if let Some(item_obj) = item.as_object_mut() {
                            item_obj.insert(
                                "ref".to_string(),
                                serde_json::Value::String(tag.to_string()),
                            );
                        }
                        return Ok(());
                    }
                }
            }
        }
    }

    anyhow::bail!("plugin '{}' not found in manifest", plugin_entry);
}

fn credential_callback(
    _url: &str,
    username_from_url: Option<&str>,
    _allowed_types: git2::CredentialType,
) -> Result<git2::Cred, git2::Error> {
    // Try SSH agent first
    if let Some(user) = username_from_url {
        if let Ok(cred) = git2::Cred::ssh_key_from_agent(user) {
            return Ok(cred);
        }
    }
    // Fallback to default credentials
    git2::Cred::default()
}
