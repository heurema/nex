use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

// ── Global config: ~/.nex/config.toml ──────────────────────────────────────

#[derive(Debug, Default, Deserialize, Serialize)]
pub struct GlobalConfig {
    #[serde(default)]
    pub schema_version: u32,
    #[serde(default)]
    pub git: GitConfig,
    #[serde(default)]
    pub tag: TagConfig,
    #[serde(default)]
    pub commit: CommitConfig,
    #[serde(default)]
    pub changelog: ChangelogConfig,
    #[serde(default)]
    pub marketplaces: HashMap<String, MarketplaceConfig>,
    #[serde(default)]
    pub hooks: HooksConfig,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct GitConfig {
    #[serde(default = "default_remote")]
    pub remote: String,
    #[serde(default)]
    pub branch: String,
}

impl Default for GitConfig {
    fn default() -> Self {
        Self { remote: default_remote(), branch: String::new() }
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub struct TagConfig {
    #[serde(default = "default_tag_format")]
    pub format: String,
    #[serde(default)]
    pub annotated: bool,
    #[serde(default = "default_tag_message")]
    pub message: String,
}

impl Default for TagConfig {
    fn default() -> Self {
        Self {
            format: default_tag_format(),
            annotated: false,
            message: default_tag_message(),
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub struct CommitConfig {
    #[serde(default = "default_commit_format")]
    pub format: String,
}

impl Default for CommitConfig {
    fn default() -> Self {
        Self { format: default_commit_format() }
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ChangelogConfig {
    #[serde(default = "default_changelog_mode")]
    pub mode: String,
    #[serde(default = "default_changelog_filename")]
    pub filename: String,
}

impl Default for ChangelogConfig {
    fn default() -> Self {
        Self {
            mode: default_changelog_mode(),
            filename: default_changelog_filename(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MarketplaceConfig {
    pub path: String,
    #[serde(default = "default_marketplace_manifest")]
    pub manifest: String,
    #[serde(default = "default_commit_format_mp")]
    pub commit_format: String,
}

#[derive(Debug, Default, Deserialize, Serialize)]
pub struct HooksConfig {
    #[serde(default)]
    pub pre_release: Vec<String>,
    #[serde(default)]
    pub post_release: Vec<String>,
}

// ── Per-plugin config: .nex/release.toml ───────────────────────────────────

#[derive(Debug, Default, Deserialize, Serialize)]
pub struct PluginReleaseConfig {
    #[serde(default)]
    pub schema_version: u32,
    /// Project name override (for non-plugin projects without plugin.json).
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub marketplace: String,
    #[serde(default)]
    pub marketplace_entry: Option<String>,
    #[serde(default)]
    pub version_files: Vec<VersionFile>,
    #[serde(default)]
    pub git: Option<GitConfig>,
    #[serde(default)]
    pub tag: Option<TagConfig>,
    #[serde(default)]
    pub commit: Option<CommitConfig>,
    #[serde(default)]
    pub changelog: Option<ChangelogConfig>,
    #[serde(default)]
    pub hooks: HooksConfig,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct VersionFile {
    pub path: String,
    pub format: String,
    /// Only used when format = "regex"
    #[serde(default)]
    pub pattern: Option<String>,
    /// Only used when format = "regex"
    #[serde(default)]
    pub replace: Option<String>,
}

// ── Merged / resolved config ────────────────────────────────────────────────

/// All settings resolved by merging global < plugin < CLI overrides.
#[derive(Debug)]
pub struct ResolvedConfig {
    pub git_remote: String,
    pub git_branch: String,
    pub tag_format: String,
    pub tag_annotated: bool,
    pub tag_message: String,
    pub commit_format: String,
    pub changelog_mode: String,
    pub changelog_filename: String,
    pub version_files: Vec<VersionFile>,
    pub marketplace: Option<String>,
    pub marketplace_entry: Option<String>,
    pub marketplace_config: Option<MarketplaceConfig>,
    pub pre_release_hooks: Vec<String>,
    pub post_release_hooks: Vec<String>,
}

// ── Loaders ─────────────────────────────────────────────────────────────────

pub fn load_global(nex_home: &Path) -> anyhow::Result<GlobalConfig> {
    let path = nex_home.join("config.toml");
    if !path.exists() {
        return Ok(GlobalConfig::default());
    }
    let content = std::fs::read_to_string(&path)
        .map_err(|e| anyhow::anyhow!("failed to read {}: {e}", path.display()))?;
    let cfg: GlobalConfig = toml::from_str(&content)
        .map_err(|e| anyhow::anyhow!("failed to parse {}: {e}", path.display()))?;
    Ok(cfg)
}

pub fn load_plugin(plugin_root: &Path) -> anyhow::Result<PluginReleaseConfig> {
    let path = plugin_root.join(".nex/release.toml");
    if !path.exists() {
        return Ok(PluginReleaseConfig::default());
    }
    let content = std::fs::read_to_string(&path)
        .map_err(|e| anyhow::anyhow!("failed to read {}: {e}", path.display()))?;
    let cfg: PluginReleaseConfig = toml::from_str(&content)
        .map_err(|e| anyhow::anyhow!("failed to parse {}: {e}", path.display()))?;
    Ok(cfg)
}

/// Expand ~ in a path string.
pub fn expand_tilde(s: &str) -> PathBuf {
    if let Some(rest) = s.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
    }
    PathBuf::from(s)
}

/// Merge global + plugin + CLI overrides into a single ResolvedConfig.
///
/// Precedence (highest wins):
///   CLI flags > .nex/release.toml > ~/.nex/config.toml > builtins
pub fn resolve(
    global: &GlobalConfig,
    plugin: &PluginReleaseConfig,
    // CLI override arguments (None = not provided)
    cli_tag_format: Option<&str>,
    cli_marketplace: Option<&str>,
    cli_no_propagate: bool,
    cli_no_changelog: bool,
    // Plugin root for smart defaults (version_files auto-detection)
    plugin_root: Option<&Path>,
) -> anyhow::Result<ResolvedConfig> {
    // git.remote: plugin > global > "origin"
    let git_remote = plugin
        .git
        .as_ref()
        .map(|g| g.remote.clone())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| global.git.remote.clone());

    // git.branch: plugin > global > "" (auto-detect at runtime)
    let git_branch = plugin
        .git
        .as_ref()
        .map(|g| g.branch.clone())
        .unwrap_or_else(|| global.git.branch.clone());

    // tag.format: CLI > plugin > global > "v{version}"
    let tag_format = cli_tag_format
        .map(|s| s.to_string())
        .or_else(|| plugin.tag.as_ref().map(|t| t.format.clone()))
        .unwrap_or_else(|| global.tag.format.clone());

    let tag_annotated = plugin
        .tag
        .as_ref()
        .map(|t| t.annotated)
        .unwrap_or(global.tag.annotated);

    let tag_message = plugin
        .tag
        .as_ref()
        .map(|t| t.message.clone())
        .unwrap_or_else(|| global.tag.message.clone());

    // commit.format: plugin > global > "release: v{version}"
    let commit_format = plugin
        .commit
        .as_ref()
        .map(|c| c.format.clone())
        .unwrap_or_else(|| global.commit.format.clone());

    // changelog: plugin > global
    let changelog_mode = if cli_no_changelog {
        "skip".to_string()
    } else {
        plugin
            .changelog
            .as_ref()
            .map(|c| c.mode.clone())
            .unwrap_or_else(|| global.changelog.mode.clone())
    };

    let changelog_filename = plugin
        .changelog
        .as_ref()
        .map(|c| c.filename.clone())
        .unwrap_or_else(|| global.changelog.filename.clone());

    // version_files: plugin > auto-detect
    let version_files = if plugin.version_files.is_empty() {
        let has_plugin_json = plugin_root
            .map(|r| r.join(".claude-plugin/plugin.json").exists())
            .unwrap_or(true); // assume plugin.json if no root provided
        let has_cargo_toml = plugin_root
            .map(|r| r.join("Cargo.toml").exists())
            .unwrap_or(false);

        if has_plugin_json {
            vec![VersionFile {
                path: ".claude-plugin/plugin.json".to_string(),
                format: "json".to_string(),
                pattern: None,
                replace: None,
            }]
        } else if has_cargo_toml {
            vec![VersionFile {
                path: "Cargo.toml".to_string(),
                format: "toml".to_string(),
                pattern: None,
                replace: None,
            }]
        } else {
            vec![VersionFile {
                path: ".claude-plugin/plugin.json".to_string(),
                format: "json".to_string(),
                pattern: None,
                replace: None,
            }]
        }
    } else {
        plugin.version_files.clone()
    };

    // marketplace: CLI > plugin > (none)
    let marketplace_name: Option<String> = if cli_no_propagate {
        None
    } else {
        cli_marketplace
            .map(|s| s.to_string())
            .or_else(|| {
                if plugin.marketplace.is_empty() {
                    None
                } else {
                    Some(plugin.marketplace.clone())
                }
            })
    };

    let marketplace_config = marketplace_name
        .as_ref()
        .and_then(|name| global.marketplaces.get(name).cloned());

    let marketplace_entry = plugin.marketplace_entry.clone();

    // hooks: plugin hooks run first, then global hooks (post_release)
    let pre_release_hooks = plugin.hooks.pre_release.clone();
    let mut post_release_hooks = plugin.hooks.post_release.clone();
    post_release_hooks.extend(global.hooks.post_release.clone());

    Ok(ResolvedConfig {
        git_remote,
        git_branch,
        tag_format,
        tag_annotated,
        tag_message,
        commit_format,
        changelog_mode,
        changelog_filename,
        version_files,
        marketplace: marketplace_name,
        marketplace_entry,
        marketplace_config,
        pre_release_hooks,
        post_release_hooks,
    })
}

// ── Placeholder expansion ────────────────────────────────────────────────────

pub fn expand_placeholders(
    template: &str,
    name: &str,
    version: &str,
    tag: &str,
    marketplace: &str,
) -> String {
    template
        .replace("{name}", name)
        .replace("{version}", version)
        .replace("{tag}", tag)
        .replace("{marketplace}", marketplace)
}

// ── Defaults ─────────────────────────────────────────────────────────────────

fn default_remote() -> String { "origin".to_string() }
fn default_tag_format() -> String { "v{version}".to_string() }
fn default_tag_message() -> String { "Release {name} v{version}".to_string() }
fn default_commit_format() -> String { "release: v{version}".to_string() }
fn default_changelog_mode() -> String { "template".to_string() }
fn default_changelog_filename() -> String { "CHANGELOG.md".to_string() }
fn default_marketplace_manifest() -> String { ".claude-plugin/marketplace.json".to_string() }
fn default_commit_format_mp() -> String { "bump {name} ref to v{version}".to_string() }
