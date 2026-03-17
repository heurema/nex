use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

// ── Catalog ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
pub struct CatalogPlugin {
    pub name: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub repo: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub category: String,
}

#[derive(Deserialize)]
struct MarketplaceJson {
    #[serde(default)]
    plugins: Vec<CatalogPluginRaw>,
}

#[derive(Deserialize)]
struct CatalogPluginRaw {
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    category: String,
    #[serde(default)]
    source: SourceRaw,
}

#[derive(Deserialize, Default)]
struct SourceRaw {
    #[serde(default, rename = "ref")]
    git_ref: String,
    #[serde(default)]
    url: String,
}

pub fn load_emporium_catalog(marketplace_path: &Path) -> anyhow::Result<HashMap<String, CatalogPlugin>> {
    let content = match fs::read_to_string(marketplace_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("warning: cannot read emporium catalog {}: {e}", marketplace_path.display());
            return Ok(HashMap::new());
        }
    };

    let parsed: MarketplaceJson = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("warning: malformed emporium catalog {}: {e}", marketplace_path.display());
            return Ok(HashMap::new());
        }
    };

    let mut map = HashMap::new();
    for raw in parsed.plugins {
        // Strip leading 'v' from ref to get bare version string
        let version = raw.source.git_ref.trim_start_matches('v').to_string();
        let repo = raw.source.url;
        map.insert(
            raw.name.clone(),
            CatalogPlugin {
                name: raw.name,
                version,
                repo,
                description: raw.description,
                category: raw.category,
            },
        );
    }
    Ok(map)
}

// ── CC cache scanner ──────────────────────────────────────────────────────────

/// Scan cache_dir/emporium/<plugin>/<version>/ (3-level deep).
/// Returns latest version per plugin name (lexicographic sort, last wins).
pub fn scan_cc_cache(cache_dir: &Path) -> HashMap<String, String> {
    let emporium_dir = cache_dir.join("emporium");
    let mut result: HashMap<String, Vec<String>> = HashMap::new();

    let plugin_entries = match fs::read_dir(&emporium_dir) {
        Ok(rd) => rd,
        Err(_) => return HashMap::new(),
    };

    for plugin_entry in plugin_entries.flatten() {
        let plugin_path = plugin_entry.path();
        if !plugin_path.is_dir() {
            continue;
        }
        let plugin_name = plugin_entry.file_name().to_string_lossy().to_string();

        let version_entries = match fs::read_dir(&plugin_path) {
            Ok(rd) => rd,
            Err(_) => continue,
        };

        for version_entry in version_entries.flatten() {
            let version_path = version_entry.path();
            if !version_path.is_dir() {
                continue;
            }
            let version = version_entry.file_name().to_string_lossy()
                .trim_start_matches('v').to_string();
            result.entry(plugin_name.clone()).or_default().push(version);
        }
    }

    result
        .into_iter()
        .filter_map(|(name, mut versions)| {
            if versions.is_empty() {
                return None;
            }
            // Semver-aware sort: split by '.', compare parts as integers, fall back to string
            versions.sort_by(|a, b| {
                let parse = |s: &str| -> Vec<u64> {
                    s.trim_start_matches('v')
                        .split('.')
                        .map(|part| part.parse::<u64>().unwrap_or(0))
                        .collect()
                };
                parse(a).cmp(&parse(b))
            });
            Some((name, versions.into_iter().last().unwrap()))
        })
        .collect()
}

// ── CC installed_plugins.json reader ─────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
pub struct CcInstallRecord {
    pub scope: Option<String>,
    pub version: Option<String>,
    #[serde(rename = "installPath")]
    pub install_path: Option<String>,
    #[serde(rename = "gitCommitSha")]
    pub git_commit_sha: Option<String>,
}

#[derive(Deserialize)]
struct InstalledPluginsJson {
    #[serde(default)]
    plugins: HashMap<String, Vec<CcInstallRecord>>,
}

pub fn load_cc_installed(path: &Path) -> HashMap<String, Vec<CcInstallRecord>> {
    let content = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("warning: cannot read installed_plugins.json {}: {e}", path.display());
            return HashMap::new();
        }
    };
    match serde_json::from_str::<InstalledPluginsJson>(&content) {
        Ok(v) => v.plugins,
        Err(e) => {
            eprintln!("warning: malformed installed_plugins.json {}: {e}", path.display());
            HashMap::new()
        }
    }
}

// ── CC settings.json enabled plugins reader ───────────────────────────────────

pub fn load_cc_enabled_plugins(settings_path: &Path) -> HashSet<String> {
    let content = match fs::read_to_string(settings_path) {
        Ok(c) => c,
        Err(_) => return HashSet::new(),
    };
    let parsed: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(_) => return HashSet::new(),
    };

    let mut enabled = HashSet::new();
    if let Some(obj) = parsed.get("enabledPlugins").and_then(|v| v.as_object()) {
        for (key, val) in obj {
            if val.as_bool().unwrap_or(false) {
                enabled.insert(key.clone());
            }
        }
    }
    enabled
}

// ── Dev symlink scanner ───────────────────────────────────────────────────────

/// Scan claude_plugins_dir for symlinks only, skipping 'cache', 'marketplaces', and dotfiles.
/// Returns name -> symlink target map.
pub fn scan_dev_symlinks(claude_plugins_dir: &Path) -> HashMap<String, PathBuf> {
    let entries = match fs::read_dir(claude_plugins_dir) {
        Ok(rd) => rd,
        Err(_) => return HashMap::new(),
    };

    let mut result = HashMap::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();

        // Skip dotfiles, cache, and marketplaces
        if name.starts_with('.') || name == "cache" || name == "marketplaces" {
            continue;
        }

        let meta = match entry.path().symlink_metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };

        if meta.file_type().is_symlink() {
            if let Ok(target) = fs::read_link(entry.path()) {
                result.insert(name, target);
            }
        }
    }
    result
}

// ── Agent skills scanner ──────────────────────────────────────────────────────

/// Scan agents_skills_dir for symlinks (Codex/Gemini skill links).
/// Returns name -> target map. Empty map if dir unreadable.
pub fn scan_agent_skills(agents_skills_dir: &Path) -> HashMap<String, PathBuf> {
    let entries = match fs::read_dir(agents_skills_dir) {
        Ok(rd) => rd,
        Err(_) => return HashMap::new(),
    };

    let mut result = HashMap::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        let meta = match entry.path().symlink_metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        if meta.file_type().is_symlink() {
            if let Ok(target) = fs::read_link(entry.path()) {
                result.insert(name, target);
            }
        }
    }
    result
}

// ── PluginView aggregator ─────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct PluginView {
    pub name: String,
    pub catalog_version: Option<String>,
    pub cc_cache_version: Option<String>,
    pub cc_installed: bool,
    pub codex_linked: bool,
    pub dev_override: Option<PathBuf>,
    pub drift: Vec<String>,
}

pub fn build_plugin_views(
    catalog: &HashMap<String, CatalogPlugin>,
    cc_cache: &HashMap<String, String>,
    cc_installed: &HashMap<String, Vec<CcInstallRecord>>,
    dev_symlinks: &HashMap<String, PathBuf>,
    agent_skills: &HashMap<String, PathBuf>,
) -> Vec<PluginView> {
    // Collect all plugin names from all sources
    // cc_installed keys are like "arbiter@emporium" — extract bare name before '@'
    let cc_installed_names: Vec<String> = cc_installed
        .keys()
        .map(|k| k.split('@').next().unwrap_or(k).to_string())
        .collect();

    let mut names: HashSet<&str> = HashSet::new();
    for k in catalog.keys() { names.insert(k.as_str()); }
    for k in cc_cache.keys() { names.insert(k.as_str()); }
    for k in dev_symlinks.keys() { names.insert(k.as_str()); }
    for k in &cc_installed_names { names.insert(k.as_str()); }

    let mut views: Vec<PluginView> = names
        .into_iter()
        .map(|name| {
            let catalog_version = catalog.get(name).map(|c| c.version.clone()).filter(|v| !v.is_empty());
            let cc_cache_version = cc_cache.get(name).cloned();

            // cc_installed: check if any key in installed map matches the plugin name
            // Keys are like "arbiter@emporium"
            let cc_installed = cc_installed
                .keys()
                .any(|k| k == name || k.starts_with(&format!("{name}@")));

            // codex_linked: check agent_skills
            let codex_linked = agent_skills
                .keys()
                .any(|k| k == name || k.starts_with(&format!("{name}@")));

            let dev_override = dev_symlinks.get(name).cloned();

            let mut drift: Vec<String> = Vec::new();

            // Drift: cache version differs from catalog version
            if let (Some(cat_ver), Some(cache_ver)) = (&catalog_version, &cc_cache_version) {
                if cat_ver != cache_ver {
                    drift.push(format!("cache version {cache_ver} != catalog version {cat_ver}"));
                }
            }

            // Drift: dev symlink overrides installed version
            if dev_override.is_some() {
                drift.push("dev symlink overrides installed version".to_string());
            }

            // Drift: plugin in CC cache but no Codex/agent skill symlink
            if cc_cache_version.is_some() && !codex_linked {
                drift.push("installed in CC but no agent skill symlink".to_string());
            }

            PluginView {
                name: name.to_string(),
                catalog_version,
                cc_cache_version,
                cc_installed,
                codex_linked,
                dev_override,
                drift,
            }
        })
        .collect();

    views.sort_by(|a, b| a.name.cmp(&b.name));
    views
}
