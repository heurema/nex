use crate::core::dirs::Dirs;
use std::collections::{BTreeSet, HashMap, HashSet};
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

pub fn load_emporium_catalog(
    marketplace_path: &Path,
) -> anyhow::Result<HashMap<String, CatalogPlugin>> {
    let content = match fs::read_to_string(marketplace_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!(
                "warning: cannot read emporium catalog {}: {e}",
                marketplace_path.display()
            );
            return Ok(HashMap::new());
        }
    };

    let parsed: MarketplaceJson = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(e) => {
            eprintln!(
                "warning: malformed emporium catalog {}: {e}",
                marketplace_path.display()
            );
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
            let version = version_entry
                .file_name()
                .to_string_lossy()
                .trim_start_matches('v')
                .to_string();
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
            eprintln!(
                "warning: cannot read installed_plugins.json {}: {e}",
                path.display()
            );
            return HashMap::new();
        }
    };
    match serde_json::from_str::<InstalledPluginsJson>(&content) {
        Ok(v) => v.plugins,
        Err(e) => {
            eprintln!(
                "warning: malformed installed_plugins.json {}: {e}",
                path.display()
            );
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

// ── Agent skill scanners ──────────────────────────────────────────────────────

fn scan_skill_entries(skills_dir: &Path) -> HashMap<String, PathBuf> {
    let entries = match fs::read_dir(skills_dir) {
        Ok(rd) => rd,
        Err(_) => return HashMap::new(),
    };

    let mut result = HashMap::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }

        let path = entry.path();
        let meta = match path.symlink_metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };

        if meta.file_type().is_symlink() {
            if let Ok(target) = fs::read_link(&path) {
                result.insert(name, target);
            }
            continue;
        }

        if meta.is_dir() && path.join("SKILL.md").is_file() {
            result.insert(name, path);
        }
    }
    result
}

/// Scan Codex skills from ~/.codex/skills/.
/// Includes both symlinks managed by nex and plain skill directories installed manually.
pub fn scan_codex_skills(codex_skills_dir: &Path) -> HashMap<String, PathBuf> {
    scan_skill_entries(codex_skills_dir)
}

/// Scan Gemini skills from ~/.agents/skills/.
/// Includes both symlinks managed by nex and plain skill directories.
pub fn scan_gemini_skills(gemini_skills_dir: &Path) -> HashMap<String, PathBuf> {
    scan_skill_entries(gemini_skills_dir)
}

fn matches_plugin_key(key: &str, name: &str) -> bool {
    key == name || key.starts_with(&format!("{name}@"))
}

fn has_codex_adapter(path: &Path) -> bool {
    path.join("platforms").join("codex").is_dir() || path.join("SKILL.md").is_file()
}

fn has_gemini_adapter(path: &Path) -> bool {
    path.join("platforms").join("gemini").is_dir() || path.join("SKILL.md").is_file()
}

fn has_claude_adapter(path: &Path) -> bool {
    path.join(".claude-plugin").join("plugin.json").is_file()
        || path
            .join("platforms")
            .join("claude-code")
            .join(".claude-plugin")
            .join("plugin.json")
            .is_file()
}

fn supports_codex_skills(name: &str, cc_installed: &HashMap<String, Vec<CcInstallRecord>>) -> bool {
    cc_installed
        .iter()
        .filter(|(key, _)| matches_plugin_key(key, name))
        .flat_map(|(_, records)| records.iter())
        .filter_map(|record| record.install_path.as_deref())
        .map(Path::new)
        .any(has_codex_adapter)
}

fn install_paths_for_plugin(
    name: &str,
    cc_installed: &HashMap<String, Vec<CcInstallRecord>>,
) -> Vec<PathBuf> {
    cc_installed
        .iter()
        .filter(|(key, _)| matches_plugin_key(key, name))
        .flat_map(|(_, records)| records.iter())
        .filter_map(|record| record.install_path.as_deref())
        .map(PathBuf::from)
        .collect()
}

fn latest_installed_version(
    name: &str,
    cc_installed: &HashMap<String, Vec<CcInstallRecord>>,
) -> Option<String> {
    cc_installed
        .iter()
        .filter(|(key, _)| matches_plugin_key(key, name))
        .flat_map(|(_, records)| records.iter())
        .filter_map(|record| record.version.as_ref())
        .find(|version| !version.is_empty())
        .cloned()
}

fn platforms_from_plugin_root(path: &Path) -> BTreeSet<String> {
    let mut platforms = BTreeSet::new();
    if has_claude_adapter(path) {
        platforms.insert("claude-code".to_string());
    }
    if has_codex_adapter(path) {
        platforms.insert("codex".to_string());
    }
    if has_gemini_adapter(path) {
        platforms.insert("gemini".to_string());
    }
    platforms
}

#[derive(Debug, Clone)]
pub struct LivePlugin {
    pub name: String,
    pub version: Option<String>,
    pub description: String,
    pub category: String,
    pub repo: String,
    pub platforms: Vec<String>,
    pub cc_installed: bool,
    pub codex_linked: bool,
    pub gemini_linked: bool,
    pub dev_override: Option<PathBuf>,
}

impl LivePlugin {
    pub fn is_installed(&self) -> bool {
        self.cc_installed || self.codex_linked || self.gemini_linked || self.dev_override.is_some()
    }
}

// ── PluginView aggregator ─────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct PluginView {
    pub name: String,
    pub catalog_version: Option<String>,
    pub cc_cache_version: Option<String>,
    pub cc_installed: bool,
    pub codex_linked: bool,
    pub gemini_linked: bool,
    pub dev_override: Option<PathBuf>,
    pub drift: Vec<String>,
    /// true if managed by nex (present in installed.json), false if external/live-discovered
    pub is_managed: bool,
}

impl PluginView {
    pub fn is_live_discovered(&self) -> bool {
        self.cc_installed
            || self.cc_cache_version.is_some()
            || self.codex_linked
            || self.gemini_linked
            || self.dev_override.is_some()
    }
}

pub fn build_plugin_views(
    catalog: &HashMap<String, CatalogPlugin>,
    cc_cache: &HashMap<String, String>,
    cc_installed: &HashMap<String, Vec<CcInstallRecord>>,
    dev_symlinks: &HashMap<String, PathBuf>,
    codex_skills: &HashMap<String, PathBuf>,
    gemini_skills: &HashMap<String, PathBuf>,
) -> Vec<PluginView> {
    // Collect all plugin names from all sources
    // cc_installed keys are like "arbiter@emporium" — extract bare name before '@'
    let cc_installed_names: Vec<String> = cc_installed
        .keys()
        .map(|k| k.split('@').next().unwrap_or(k).to_string())
        .collect();

    let mut names: HashSet<&str> = HashSet::new();
    for k in catalog.keys() {
        names.insert(k.as_str());
    }
    for k in cc_cache.keys() {
        names.insert(k.as_str());
    }
    for k in dev_symlinks.keys() {
        names.insert(k.as_str());
    }
    for k in &cc_installed_names {
        names.insert(k.as_str());
    }
    for k in codex_skills.keys() {
        names.insert(k.as_str());
    }
    for k in gemini_skills.keys() {
        names.insert(k.as_str());
    }

    let mut views: Vec<PluginView> = names
        .into_iter()
        .map(|name| {
            let catalog_version = catalog
                .get(name)
                .map(|c| c.version.clone())
                .filter(|v| !v.is_empty());
            let cc_cache_version = cc_cache.get(name).cloned();

            // cc_installed: check if any key in installed map matches the plugin name
            // Keys are like "arbiter@emporium"
            let cc_installed_flag = cc_installed
                .keys()
                .any(|k| k == name || k.starts_with(&format!("{name}@")));

            let codex_linked = codex_skills.keys().any(|k| matches_plugin_key(k, name));
            let gemini_linked = gemini_skills.keys().any(|k| matches_plugin_key(k, name));

            let dev_override = dev_symlinks.get(name).cloned();
            let codex_capable = supports_codex_skills(name, cc_installed);

            let mut drift: Vec<String> = Vec::new();

            // Drift: cache version differs from catalog version
            if let (Some(cat_ver), Some(cache_ver)) = (&catalog_version, &cc_cache_version) {
                if cat_ver != cache_ver {
                    drift.push(format!(
                        "cache version {cache_ver} != catalog version {cat_ver}"
                    ));
                }
            }

            // Drift: dev symlink overrides installed version
            if dev_override.is_some() {
                drift.push("dev symlink overrides installed version".to_string());
            }

            if cc_cache_version.is_some() && codex_capable && !codex_linked {
                drift.push("installed in CC but no codex skill symlink".to_string());
            }

            PluginView {
                name: name.to_string(),
                catalog_version,
                cc_cache_version,
                cc_installed: cc_installed_flag,
                codex_linked,
                gemini_linked,
                dev_override,
                drift,
                is_managed: false, // set by load_plugin_views after state check
            }
        })
        .collect();

    views.sort_by(|a, b| a.name.cmp(&b.name));
    views
}

pub fn load_plugin_views(dirs: &Dirs) -> anyhow::Result<Vec<PluginView>> {
    let catalog = load_emporium_catalog(&dirs.emporium_marketplace_path())?;
    let cc_cache = scan_cc_cache(&dirs.cc_cache_dir());
    let cc_installed = load_cc_installed(&dirs.cc_installed_plugins_path());
    let dev_symlinks = scan_dev_symlinks(&dirs.claude_plugins);
    let codex_skills = scan_codex_skills(&dirs.codex_skills);
    let gemini_skills = scan_gemini_skills(&dirs.agents_skills);

    let mut views = build_plugin_views(
        &catalog,
        &cc_cache,
        &cc_installed,
        &dev_symlinks,
        &codex_skills,
        &gemini_skills,
    );

    // Mark managed plugins from installed.json
    let state = crate::core::state::InstalledState::load(&dirs.installed_path())
        .unwrap_or_default();
    for view in &mut views {
        view.is_managed = state.get(&view.name).is_some();
    }

    Ok(views)
}

pub fn load_live_plugins(dirs: &Dirs) -> anyhow::Result<HashMap<String, LivePlugin>> {
    let catalog = load_emporium_catalog(&dirs.emporium_marketplace_path())?;
    let cc_cache = scan_cc_cache(&dirs.cc_cache_dir());
    let cc_installed = load_cc_installed(&dirs.cc_installed_plugins_path());
    let dev_symlinks = scan_dev_symlinks(&dirs.claude_plugins);
    let codex_skills = scan_codex_skills(&dirs.codex_skills);
    let gemini_skills = scan_gemini_skills(&dirs.agents_skills);
    let views = build_plugin_views(
        &catalog,
        &cc_cache,
        &cc_installed,
        &dev_symlinks,
        &codex_skills,
        &gemini_skills,
    );

    let mut plugins = HashMap::new();

    for view in views {
        let name = view.name.clone();
        let install_paths = install_paths_for_plugin(&name, &cc_installed);
        let mut platforms = BTreeSet::new();

        if catalog.contains_key(&name) || view.cc_installed {
            platforms.insert("claude-code".to_string());
        }
        for path in &install_paths {
            platforms.extend(platforms_from_plugin_root(path));
        }
        if codex_skills.contains_key(&name) {
            platforms.insert("codex".to_string());
        }
        if gemini_skills.contains_key(&name) {
            platforms.insert("gemini".to_string());
        }

        let catalog_entry = catalog.get(&name);
        let version = view
            .catalog_version
            .clone()
            .or(view.cc_cache_version.clone())
            .or_else(|| latest_installed_version(&name, &cc_installed));

        plugins.insert(
            name.clone(),
            LivePlugin {
                name,
                version,
                description: catalog_entry
                    .map(|entry| entry.description.clone())
                    .unwrap_or_default(),
                category: catalog_entry
                    .map(|entry| entry.category.clone())
                    .unwrap_or_default(),
                repo: catalog_entry
                    .map(|entry| entry.repo.clone())
                    .unwrap_or_default(),
                platforms: platforms.into_iter().collect(),
                cc_installed: view.cc_installed,
                codex_linked: view.codex_linked,
                gemini_linked: view.gemini_linked,
                dev_override: view.dev_override.clone(),
            },
        );
    }

    Ok(plugins)
}

// ── CC cache invalidation ────────────────────────────────────────────────────

/// Invalidate Claude Code plugin cache for a specific marketplace + plugin.
/// Removes the entire plugin subtree from cache (all versions).
/// CC will re-clone from marketplace on next load.
pub fn invalidate_cc_cache(dirs: &Dirs, marketplace: &str, plugin: &str) -> bool {
    let cache_dir = dirs.claude_plugins.join("cache").join(marketplace).join(plugin);
    if cache_dir.exists() {
        if let Err(e) = fs::remove_dir_all(&cache_dir) {
            eprintln!("warning: failed to invalidate CC cache at {}: {e}", cache_dir.display());
            return false;
        }
        true
    } else {
        false
    }
}

/// Garbage-collect stale nex-owned marketplaces.
/// Removes nex-* marketplace dirs (not emporium), their cache entries,
/// and known_marketplaces.json entries.
/// Returns list of removed marketplace names.
pub fn gc_nex_marketplaces(dirs: &Dirs) -> Vec<String> {
    let mut removed = Vec::new();
    let marketplaces_dir = dirs.claude_plugins.join("marketplaces");
    let cache_dir = dirs.claude_plugins.join("cache");
    let known_path = dirs.claude_plugins.join("known_marketplaces.json");

    // Find nex-* marketplace dirs
    let Ok(entries) = fs::read_dir(&marketplaces_dir) else { return removed };
    let nex_dirs: Vec<String> = entries
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with("nex-") { Some(name) } else { None }
        })
        .collect();

    for mp_name in &nex_dirs {
        // Check if marketplace has any plugins
        let plugins_dir = marketplaces_dir.join(mp_name).join("plugins");
        let has_plugins = plugins_dir.exists() && fs::read_dir(&plugins_dir)
            .map(|entries| entries.count() > 0)
            .unwrap_or(false);

        if has_plugins {
            continue; // still in use
        }

        // Remove empty marketplace dir
        let _ = fs::remove_dir_all(marketplaces_dir.join(mp_name));
        // Remove matching cache
        let _ = fs::remove_dir_all(cache_dir.join(mp_name));
        removed.push(mp_name.clone());
    }

    // Clean known_marketplaces.json
    if !removed.is_empty() && known_path.exists() {
        if let Ok(content) = fs::read_to_string(&known_path) {
            if let Ok(mut data) = serde_json::from_str::<serde_json::Value>(&content) {
                if let Some(obj) = data.as_object_mut() {
                    for name in &removed {
                        obj.remove(name);
                    }
                    if let Ok(json) = serde_json::to_string_pretty(&data) {
                        let _ = fs::write(&known_path, format!("{json}\n"));
                    }
                }
            }
        }
    }

    removed
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn install_record(path: &Path) -> CcInstallRecord {
        CcInstallRecord {
            scope: Some("user".to_string()),
            version: Some("0.1.0".to_string()),
            install_path: Some(path.to_string_lossy().to_string()),
            git_commit_sha: None,
        }
    }

    #[test]
    fn agent_skill_only_plugins_are_included_in_views() {
        let mut codex_skills = HashMap::new();
        codex_skills.insert("signum".to_string(), PathBuf::from("/tmp/signum"));

        let views = build_plugin_views(
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
            &codex_skills,
            &HashMap::new(),
        );

        assert_eq!(views.len(), 1);
        assert_eq!(views[0].name, "signum");
        assert!(views[0].codex_linked);
        assert!(!views[0].gemini_linked);
        assert!(views[0].drift.is_empty());
    }

    #[test]
    fn cc_only_plugins_do_not_report_missing_agent_link() {
        let tmp = tempdir().unwrap();
        let plugin_root = tmp.path().join("delve");
        fs::create_dir_all(plugin_root.join("skills/delve")).unwrap();
        fs::write(plugin_root.join("skills/delve/SKILL.md"), "# delve\n").unwrap();

        let mut cc_cache = HashMap::new();
        cc_cache.insert("delve".to_string(), "0.8.1".to_string());

        let mut cc_installed = HashMap::new();
        cc_installed.insert(
            "delve@emporium".to_string(),
            vec![install_record(&plugin_root)],
        );

        let views = build_plugin_views(
            &HashMap::new(),
            &cc_cache,
            &cc_installed,
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
        );

        let delve = views.iter().find(|view| view.name == "delve").unwrap();
        assert!(
            !delve
                .drift
                .iter()
                .any(|drift| drift == "installed in CC but no codex skill symlink")
        );
    }

    #[test]
    fn agent_capable_plugins_report_missing_agent_link() {
        let tmp = tempdir().unwrap();
        let plugin_root = tmp.path().join("signum");
        fs::create_dir_all(plugin_root.join("platforms/codex")).unwrap();
        fs::write(plugin_root.join("platforms/codex/SKILL.md"), "# signum\n").unwrap();

        let mut cc_cache = HashMap::new();
        cc_cache.insert("signum".to_string(), "4.8.0".to_string());

        let mut cc_installed = HashMap::new();
        cc_installed.insert(
            "signum@emporium".to_string(),
            vec![install_record(&plugin_root)],
        );

        let views = build_plugin_views(
            &HashMap::new(),
            &cc_cache,
            &cc_installed,
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
        );

        let signum = views.iter().find(|view| view.name == "signum").unwrap();
        assert!(
            signum
                .drift
                .iter()
                .any(|drift| drift == "installed in CC but no codex skill symlink")
        );
    }

    #[test]
    fn live_plugin_for_cc_only_catalog_entry_reports_claude_code_platform() {
        let tmp = tempdir().unwrap();
        let plugin_root = tmp.path().join("delve");
        fs::create_dir_all(plugin_root.join(".claude-plugin")).unwrap();
        fs::write(
            plugin_root.join(".claude-plugin/plugin.json"),
            "{\"name\":\"delve\",\"version\":\"0.8.1\"}\n",
        )
        .unwrap();

        let mut catalog = HashMap::new();
        catalog.insert(
            "delve".to_string(),
            CatalogPlugin {
                name: "delve".to_string(),
                version: "0.8.1".to_string(),
                repo: "https://example.com/delve.git".to_string(),
                description: "deep research".to_string(),
                category: "research".to_string(),
            },
        );

        let mut cc_cache = HashMap::new();
        cc_cache.insert("delve".to_string(), "0.8.1".to_string());

        let mut cc_installed = HashMap::new();
        cc_installed.insert(
            "delve@emporium".to_string(),
            vec![install_record(&plugin_root)],
        );

        let views = build_plugin_views(
            &catalog,
            &cc_cache,
            &cc_installed,
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
        );
        let live = {
            let mut result = HashMap::new();
            for plugin in views {
                result.insert(plugin.name.clone(), plugin);
            }
            result
        };

        let delve = live.get("delve").unwrap();
        assert!(delve.cc_installed);
        assert!(!delve.codex_linked);
        assert!(!delve.gemini_linked);
    }

    #[test]
    fn root_skill_supports_both_agent_platforms() {
        let tmp = tempdir().unwrap();
        let plugin_root = tmp.path().join("oracle");
        fs::create_dir_all(&plugin_root).unwrap();
        fs::write(plugin_root.join("SKILL.md"), "# oracle\n").unwrap();

        let platforms = platforms_from_plugin_root(&plugin_root);
        assert!(platforms.contains("codex"));
        assert!(platforms.contains("gemini"));
    }

    #[test]
    fn scan_codex_skills_includes_plain_skill_directories() {
        let tmp = tempdir().unwrap();
        let skills_dir = tmp.path().join("skills");
        let delve_dir = skills_dir.join("delve");
        fs::create_dir_all(&delve_dir).unwrap();
        fs::write(delve_dir.join("SKILL.md"), "# delve\n").unwrap();

        let scanned = scan_codex_skills(&skills_dir);
        assert_eq!(scanned.get("delve"), Some(&delve_dir));
    }
}
