# nex v0.8.0 — Marketplace Management Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make nex aware of all 12 emporium plugins across Claude Code, Codex, and Gemini with read-only CC integration, profile management, and drift detection.

**Architecture:** Layered SSoT — emporium = catalog, CC adapter = read-only reader, profiles = desired state in TOML. New modules: `cc_adapter.rs`, `profiles.rs`. Rewrite: `list.rs`, `check.rs`, `update.rs`. Extend: `doctor.rs`. New commands: `status`, `profile`.

**Tech Stack:** Rust, serde_json, toml, clap (existing), anyhow (existing)

**Spec:** `docs/plans/2026-03-17-nex-marketplace-design.md`

---

## File Structure

### New files
- `src/core/cc_adapter.rs` — Read-only CC state reader (installed_plugins.json, cache/, settings.json, emporium marketplace.json)
- `src/core/profiles.rs` — Profile TOML read/write, activation, drift computation
- `src/commands/status.rs` — Cross-platform health view
- `src/commands/profile.rs` — Profile list/show/apply/activate
- `tests/cc_adapter_test.rs` — Unit tests for CC adapter with mock filesystem
- `tests/profiles_test.rs` — Unit tests for profile manager

### Modified files
- `src/core/mod.rs` — Add `pub mod cc_adapter; pub mod profiles;`
- `src/core/dirs.rs` — Add CC path helpers, profile paths, emporium marketplace path
- `src/commands/mod.rs` — Add `pub mod status; pub mod profile;`
- `src/commands/list.rs` — Rewrite to use cc_adapter + emporium catalog
- `src/commands/check.rs` — Rewrite to use cc_adapter for drift detection
- `src/commands/update.rs` — Rewrite to differentiate CC (no-op) vs Codex/Gemini (symlink refresh)
- `src/commands/doctor.rs` — Add 6 new checks using cc_adapter
- `src/main.rs` — Add `Status` and `Profile` subcommands to clap

---

## Chunk 1: CC Adapter + Dirs Extension

### Task 1: Extend Dirs with CC and profile paths

**Files:**
- Modify: `src/core/dirs.rs`

- [ ] **Step 1: Add CC path helpers to Dirs**

```rust
// Add to Dirs struct (after existing fields):
// No new struct fields needed — derive from existing claude_plugins

// Add methods to impl Dirs:
pub fn cc_installed_plugins_path(&self) -> PathBuf {
    self.claude_plugins.join("installed_plugins.json")
}

pub fn cc_settings_path(&self) -> PathBuf {
    let home = dirs::home_dir().unwrap();
    home.join(".claude").join("settings.json")
}

pub fn cc_profile_settings_path(&self, profile_name: &str) -> PathBuf {
    let home = dirs::home_dir().unwrap();
    home.join(".claude-profiles")
        .join(profile_name)
        .join("config")
        .join("settings.json")
}

pub fn cc_cache_dir(&self) -> PathBuf {
    self.claude_plugins.join("cache")
}

pub fn emporium_marketplace_path(&self) -> PathBuf {
    self.claude_plugins
        .join("marketplaces")
        .join("emporium")
        .join(".claude-plugin")
        .join("marketplace.json")
}

pub fn nex_profiles_dir(&self) -> PathBuf {
    self.nex_home.join("profiles")
}

pub fn active_profile_path(&self) -> PathBuf {
    self.nex_home.join("active_profile")
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cd ~/personal/skill7/nex && cargo check`
Expected: OK (no errors, methods are unused but that's fine)

- [ ] **Step 3: Commit**

```bash
git add src/core/dirs.rs
git commit -m "feat: add CC and profile path helpers to Dirs"
```

---

### Task 2: CC Adapter — emporium catalog reader

**Files:**
- Create: `src/core/cc_adapter.rs`
- Modify: `src/core/mod.rs`

- [ ] **Step 1: Create cc_adapter.rs with CatalogPlugin struct and emporium reader**

```rust
// src/core/cc_adapter.rs
use anyhow::Result;
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

/// A plugin entry from emporium marketplace.json
#[derive(Debug, Clone)]
pub struct CatalogPlugin {
    pub name: String,
    pub version: String,      // from source.ref, e.g. "v2.1.0"
    pub repo: String,         // from source.url
    pub description: String,
    pub category: String,
}

// Raw serde types for marketplace.json
#[derive(Deserialize)]
struct MarketplaceJson {
    plugins: Vec<MarketplaceEntry>,
}

#[derive(Deserialize)]
struct MarketplaceEntry {
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    category: String,
    #[serde(default)]
    source: Option<SourceEntry>,
}

#[derive(Deserialize)]
struct SourceEntry {
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    r#ref: Option<String>,
}

/// Read emporium marketplace.json and return catalog plugins keyed by name.
pub fn load_emporium_catalog(marketplace_path: &Path) -> Result<HashMap<String, CatalogPlugin>> {
    let content = match fs::read_to_string(marketplace_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("warn: cannot read emporium marketplace.json: {e}");
            return Ok(HashMap::new());
        }
    };
    let mkt: MarketplaceJson = match serde_json::from_str(&content) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("warn: cannot parse emporium marketplace.json: {e}");
            return Ok(HashMap::new());
        }
    };

    let mut catalog = HashMap::new();
    for entry in mkt.plugins {
        let source = entry.source.unwrap_or(SourceEntry { url: None, r#ref: None });
        let version = source.r#ref.unwrap_or_default()
            .trim_start_matches('v').to_string();
        let repo = source.url.unwrap_or_default();
        catalog.insert(entry.name.clone(), CatalogPlugin {
            name: entry.name,
            version,
            repo,
            description: entry.description,
            category: entry.category,
        });
    }
    Ok(catalog)
}
```

- [ ] **Step 2: Register module in core/mod.rs**

Add `pub mod cc_adapter;` to `src/core/mod.rs`.

- [ ] **Step 3: Verify it compiles**

Run: `cargo check`
Expected: OK

- [ ] **Step 4: Commit**

```bash
git add src/core/cc_adapter.rs src/core/mod.rs
git commit -m "feat: cc_adapter — emporium catalog reader"
```

---

### Task 3: CC Adapter — CC cache scanner

**Files:**
- Modify: `src/core/cc_adapter.rs`

- [ ] **Step 1: Add CC cache version scanner**

Append to `cc_adapter.rs`:

```rust
/// Scan ~/.claude/plugins/cache/emporium/{plugin}/{version}/ and return
/// the latest cached version per plugin name.
pub fn scan_cc_cache(cache_dir: &Path) -> HashMap<String, String> {
    let mut result = HashMap::new();
    let emporium_cache = cache_dir.join("emporium");
    let Ok(plugins) = fs::read_dir(&emporium_cache) else {
        return result;
    };
    for plugin_entry in plugins.flatten() {
        if !plugin_entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            continue;
        }
        let plugin_name = plugin_entry.file_name().to_string_lossy().to_string();
        // Find latest version dir (lexicographic sort, last = latest semver usually)
        let Ok(versions) = fs::read_dir(plugin_entry.path()) else { continue };
        let mut vers: Vec<String> = versions.flatten()
            .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        vers.sort();
        if let Some(latest) = vers.last() {
            result.insert(plugin_name, latest.trim_start_matches('v').to_string());
        }
    }
    result
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo check`

- [ ] **Step 3: Commit**

```bash
git add src/core/cc_adapter.rs
git commit -m "feat: cc_adapter — CC cache version scanner"
```

---

### Task 4: CC Adapter — CC installed plugins reader + enabled plugins reader

**Files:**
- Modify: `src/core/cc_adapter.rs`

- [ ] **Step 1: Add installed_plugins.json reader**

Append to `cc_adapter.rs`:

```rust
/// A single install record from CC installed_plugins.json
#[derive(Debug, Clone, Deserialize)]
pub struct CcInstallRecord {
    #[serde(default)]
    pub scope: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(rename = "installPath", default)]
    pub install_path: Option<String>,
    #[serde(rename = "gitCommitSha", default)]
    pub git_commit_sha: Option<String>,
}

/// Read CC installed_plugins.json. Returns map of "name@marketplace" -> Vec<CcInstallRecord>.
pub fn load_cc_installed(path: &Path) -> HashMap<String, Vec<CcInstallRecord>> {
    #[derive(Deserialize)]
    struct CcInstalled {
        #[serde(default)]
        version: Option<u32>,
        #[serde(default)]
        plugins: HashMap<String, Vec<CcInstallRecord>>,
    }

    let Ok(content) = fs::read_to_string(path) else {
        return HashMap::new();
    };
    match serde_json::from_str::<CcInstalled>(&content) {
        Ok(data) => data.plugins,
        Err(e) => {
            eprintln!("warn: cannot parse installed_plugins.json: {e}");
            HashMap::new()
        }
    }
}

/// Read enabled plugins from a CC settings.json file.
/// Returns set of plugin keys like "herald@emporium" that are set to true.
pub fn load_cc_enabled_plugins(settings_path: &Path) -> std::collections::HashSet<String> {
    let mut result = std::collections::HashSet::new();
    let Ok(content) = fs::read_to_string(settings_path) else {
        return result;
    };
    let Ok(val) = serde_json::from_str::<serde_json::Value>(&content) else {
        return result;
    };
    if let Some(plugins) = val.get("enabledPlugins").and_then(|v| v.as_object()) {
        for (key, enabled) in plugins {
            if enabled.as_bool().unwrap_or(false) {
                result.insert(key.clone());
            }
        }
    }
    result
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo check`

- [ ] **Step 3: Commit**

```bash
git add src/core/cc_adapter.rs
git commit -m "feat: cc_adapter — installed plugins + enabled plugins readers"
```

---

### Task 5: CC Adapter — dev symlink scanner + unified PluginView

**Files:**
- Modify: `src/core/cc_adapter.rs`

- [ ] **Step 1: Add dev symlink scanner and PluginView aggregator**

Append to `cc_adapter.rs`:

```rust
use std::path::PathBuf;

/// Scan ~/.claude/plugins/ for dev symlinks (direct symlinks, not in marketplaces/ or cache/)
pub fn scan_dev_symlinks(claude_plugins_dir: &Path) -> HashMap<String, PathBuf> {
    let mut result = HashMap::new();
    let Ok(entries) = fs::read_dir(claude_plugins_dir) else {
        return result;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        // Skip directories that aren't symlinks, and skip known subdirs
        let name = entry.file_name().to_string_lossy().to_string();
        if name == "cache" || name == "marketplaces" || name.starts_with('.') {
            continue;
        }
        if path.is_symlink() {
            if let Ok(target) = fs::read_link(&path) {
                result.insert(name, target);
            }
        }
    }
    result
}

/// Scan ~/.agents/skills/ for Codex/Gemini symlinks
pub fn scan_agent_skills(agents_skills_dir: &Path) -> HashMap<String, PathBuf> {
    let mut result = HashMap::new();
    let Ok(entries) = fs::read_dir(agents_skills_dir) else {
        return result;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if path.is_symlink() {
            if let Ok(target) = fs::read_link(&path) {
                result.insert(name, target);
            }
        }
    }
    result
}

/// Unified view of a plugin across all data sources
#[derive(Debug, Clone)]
pub struct PluginView {
    pub name: String,
    pub catalog_version: Option<String>,    // from emporium ref
    pub cc_cache_version: Option<String>,   // from CC cache dir
    pub cc_installed: bool,                 // in CC installed_plugins.json
    pub codex_linked: bool,                 // symlink in ~/.agents/skills/
    pub dev_override: Option<PathBuf>,      // dev symlink target
    pub drift: Vec<String>,                 // human-readable drift notes
}

/// Build unified plugin views from all data sources
pub fn build_plugin_views(
    catalog: &HashMap<String, CatalogPlugin>,
    cc_cache: &HashMap<String, String>,
    cc_installed: &HashMap<String, Vec<CcInstallRecord>>,
    dev_symlinks: &HashMap<String, PathBuf>,
    agent_skills: &HashMap<String, PathBuf>,
) -> Vec<PluginView> {
    let mut views = Vec::new();

    for (name, cat) in catalog {
        let cache_ver = cc_cache.get(name).cloned();
        let is_cc_installed = cc_installed.keys()
            .any(|k| k.starts_with(&format!("{name}@")));
        let is_codex = agent_skills.contains_key(name);
        let dev = dev_symlinks.get(name).cloned();

        let mut drift = Vec::new();
        if let Some(ref cv) = cache_ver {
            if !cat.version.is_empty() && *cv != cat.version {
                drift.push(format!("CC cache={cv} but emporium={}", cat.version));
            }
        }
        if dev.is_some() {
            drift.push("dev override active".to_string());
        }
        if !is_codex && is_cc_installed {
            drift.push("missing Codex/Gemini symlink".to_string());
        }

        views.push(PluginView {
            name: name.clone(),
            catalog_version: if cat.version.is_empty() { None } else { Some(cat.version.clone()) },
            cc_cache_version: cache_ver,
            cc_installed: is_cc_installed,
            codex_linked: is_codex,
            dev_override: dev,
            drift,
        });
    }

    views.sort_by(|a, b| a.name.cmp(&b.name));
    views
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo check`

- [ ] **Step 3: Commit**

```bash
git add src/core/cc_adapter.rs
git commit -m "feat: cc_adapter — dev symlinks, agent skills, unified PluginView"
```

---

## Chunk 2: Profile Manager

### Task 6: Profile TOML reader/writer

**Files:**
- Create: `src/core/profiles.rs`
- Modify: `src/core/mod.rs`

- [ ] **Step 1: Create profiles.rs with Profile struct and load/save**

```rust
// src/core/profiles.rs
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Profile {
    #[serde(default)]
    pub plugins: PluginsSection,
    #[serde(default)]
    pub dev: HashMap<String, String>,   // plugin_name -> source path
    #[serde(default)]
    pub platforms: PlatformsSection,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct PluginsSection {
    #[serde(default)]
    pub enable: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PlatformsSection {
    #[serde(default = "default_true", rename = "claude-code")]
    pub claude_code: bool,
    #[serde(default = "default_true")]
    pub codex: bool,
    #[serde(default = "default_true")]
    pub gemini: bool,
}

fn default_true() -> bool { true }

impl Default for PlatformsSection {
    fn default() -> Self {
        Self { claude_code: true, codex: true, gemini: true }
    }
}

impl Profile {
    pub fn load(path: &Path) -> Result<Self> {
        let content = fs::read_to_string(path)?;
        Ok(toml::from_str(&content)?)
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let content = toml::to_string_pretty(self)?;
        fs::write(path, content)?;
        Ok(())
    }
}

/// List all profile names from ~/.nex/profiles/*.toml
pub fn list_profiles(profiles_dir: &Path) -> Vec<String> {
    let Ok(entries) = fs::read_dir(profiles_dir) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries.flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            name.strip_suffix(".toml").map(|n| n.to_string())
        })
        .collect();
    names.sort();
    names
}

/// Read active profile name from ~/.nex/active_profile
pub fn get_active_profile(path: &Path) -> Option<String> {
    fs::read_to_string(path).ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Set active profile
pub fn set_active_profile(path: &Path, name: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, format!("{name}\n"))?;
    Ok(())
}
```

- [ ] **Step 2: Register module in core/mod.rs**

Add `pub mod profiles;` to `src/core/mod.rs`.

- [ ] **Step 3: Verify it compiles**

Run: `cargo check`

- [ ] **Step 4: Commit**

```bash
git add src/core/profiles.rs src/core/mod.rs
git commit -m "feat: profile manager — TOML read/write, activation"
```

---

## Chunk 3: Rewrite Commands (list, check, update)

### Task 7: Rewrite `nex list`

**Files:**
- Modify: `src/commands/list.rs`

- [ ] **Step 1: Rewrite list.rs to use cc_adapter**

Replace entire file:

```rust
use crate::core::{cc_adapter, dirs::Dirs};

pub fn run() -> anyhow::Result<()> {
    let dirs = Dirs::new()?;

    let catalog = cc_adapter::load_emporium_catalog(&dirs.emporium_marketplace_path())?;
    if catalog.is_empty() {
        println!("No emporium plugins found. Check ~/.claude/plugins/marketplaces/emporium/");
        return Ok(());
    }

    let cc_cache = cc_adapter::scan_cc_cache(&dirs.cc_cache_dir());
    let cc_installed = cc_adapter::load_cc_installed(&dirs.cc_installed_plugins_path());
    let dev_symlinks = cc_adapter::scan_dev_symlinks(&dirs.claude_plugins);
    let agent_skills = cc_adapter::scan_agent_skills(&dirs.agents_skills);

    let views = cc_adapter::build_plugin_views(
        &catalog, &cc_cache, &cc_installed, &dev_symlinks, &agent_skills,
    );

    println!("{:<16} {:<10} {:<10} {:<6} {:<6} {}",
        "PLUGIN", "VERSION", "EMPORIUM", "CC", "CODEX", "DEV");
    println!("{}", "\u{2500}".repeat(68));

    for v in &views {
        let ver = v.catalog_version.as_deref().unwrap_or("—");
        let emp = v.catalog_version.as_ref()
            .map(|v| format!("v{v}")).unwrap_or_else(|| "—".to_string());
        let cc = if v.cc_installed { "\x1b[32m\u{2713}\x1b[0m" } else { "—" };
        let codex = if v.codex_linked { "\x1b[32m\u{2713}\x1b[0m" } else { "—" };
        let dev = match &v.dev_override {
            Some(p) => {
                let s = p.to_string_lossy();
                let short = if s.len() > 25 {
                    format!("dev\u{2192}~/{}", &s[s.find("personal").unwrap_or(0)..])
                } else {
                    format!("dev\u{2192}{s}")
                };
                short
            }
            None => "—".to_string(),
        };

        println!("{:<16} {:<10} {:<10} {:<6} {:<6} {}",
            v.name, ver, emp, cc, codex, dev);
    }

    let drift_count = views.iter().filter(|v| !v.drift.is_empty()).count();
    if drift_count > 0 {
        println!("\n{drift_count} plugin(s) with drift. Run `nex check` for details.");
    }

    Ok(())
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo check`

- [ ] **Step 3: Run `nex list` and verify output shows emporium plugins**

Run: `cargo run -- list`
Expected: Table with 12 emporium plugins, CC/Codex columns, dev overrides

- [ ] **Step 4: Commit**

```bash
git add src/commands/list.rs
git commit -m "feat: rewrite nex list to show emporium plugins across platforms"
```

---

### Task 8: Rewrite `nex check`

**Files:**
- Modify: `src/commands/check.rs`

- [ ] **Step 1: Rewrite check.rs to detect drift**

Replace entire file:

```rust
use crate::core::{cc_adapter, dirs::Dirs};

pub fn run(refresh: bool) -> anyhow::Result<()> {
    let dirs = Dirs::new()?;

    let catalog = cc_adapter::load_emporium_catalog(&dirs.emporium_marketplace_path())?;
    let cc_cache = cc_adapter::scan_cc_cache(&dirs.cc_cache_dir());
    let cc_installed = cc_adapter::load_cc_installed(&dirs.cc_installed_plugins_path());
    let dev_symlinks = cc_adapter::scan_dev_symlinks(&dirs.claude_plugins);
    let agent_skills = cc_adapter::scan_agent_skills(&dirs.agents_skills);

    let views = cc_adapter::build_plugin_views(
        &catalog, &cc_cache, &cc_installed, &dev_symlinks, &agent_skills,
    );

    println!("{:<16} {:<12} {:<12} {:<10} {}",
        "PLUGIN", "EMPORIUM", "CC CACHE", "CODEX", "STATUS");
    println!("{}", "\u{2500}".repeat(62));

    let mut update_count = 0;
    let mut drift_count = 0;

    for v in &views {
        let emp = v.catalog_version.as_ref()
            .map(|v| format!("v{v}")).unwrap_or_else(|| "—".to_string());
        let cache = v.cc_cache_version.as_ref()
            .map(|v| format!("v{v}")).unwrap_or_else(|| "—".to_string());
        let codex = if v.codex_linked { "linked" } else { "—" };

        let status = if v.drift.is_empty() {
            "\x1b[32mOK\x1b[0m".to_string()
        } else if v.drift.iter().any(|d| d.contains("CC cache=")) {
            update_count += 1;
            "\x1b[33mUPDATE \u{2191}\x1b[0m".to_string()
        } else if v.dev_override.is_some() {
            "OK (dev override)".to_string()
        } else {
            drift_count += 1;
            "\x1b[33mDRIFT\x1b[0m".to_string()
        };

        println!("{:<16} {:<12} {:<12} {:<10} {}",
            v.name, emp, cache, codex, status);
    }

    if update_count > 0 {
        println!("\n{update_count} update(s) available. Restart `claude` to pull updated cache.");
    }
    if drift_count > 0 {
        println!("{drift_count} drift(s) detected. Run `nex doctor` for details.");
    }
    if update_count == 0 && drift_count == 0 {
        println!("\nAll plugins in sync.");
    }

    Ok(())
}
```

- [ ] **Step 2: Verify it compiles and run**

Run: `cargo check && cargo run -- check`
Expected: Table showing herald as UPDATE (v2.1.0 vs v2.0.0 in cache)

- [ ] **Step 3: Commit**

```bash
git add src/commands/check.rs
git commit -m "feat: rewrite nex check with emporium drift detection"
```

---

### Task 9: Rewrite `nex update`

**Files:**
- Modify: `src/commands/update.rs`

- [ ] **Step 1: Read current update.rs to understand lock/install_inner pattern**

Read: `src/commands/update.rs` — note the lock acquisition and `install_inner` call.

- [ ] **Step 2: Add CC-aware logic**

The existing update flow (lock → clone → symlink) stays for Codex/Gemini. For CC: check if emporium ref matches CC cache. If behind, print instruction to restart claude. Don't touch CC cache directly.

Add at the top of `run()` after lock acquisition:

```rust
// For CC: compare emporium ref vs CC cache
let catalog = cc_adapter::load_emporium_catalog(&dirs.emporium_marketplace_path())?;
let cc_cache = cc_adapter::scan_cc_cache(&dirs.cc_cache_dir());

if let Some(cat) = catalog.get(&name) {
    if let Some(cached) = cc_cache.get(&name) {
        if cached == &cat.version {
            println!("{name}: CC cache already at v{} (emporium ref matches)", cat.version);
        } else {
            println!("{name}: emporium ref=v{} but CC cache=v{cached}", cat.version);
            println!("  Restart `claude` to pull the updated version.");
        }
    }
}
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo check`

- [ ] **Step 4: Commit**

```bash
git add src/commands/update.rs
git commit -m "feat: nex update shows CC cache drift status"
```

---

## Chunk 4: New Commands (status, profile)

### Task 10: Add `nex status` command

**Files:**
- Create: `src/commands/status.rs`
- Modify: `src/commands/mod.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Create status.rs**

```rust
use crate::core::{cc_adapter, dirs::Dirs, profiles};

pub fn run() -> anyhow::Result<()> {
    let dirs = Dirs::new()?;

    let catalog = cc_adapter::load_emporium_catalog(&dirs.emporium_marketplace_path())?;
    let cc_cache = cc_adapter::scan_cc_cache(&dirs.cc_cache_dir());
    let cc_installed = cc_adapter::load_cc_installed(&dirs.cc_installed_plugins_path());
    let dev_symlinks = cc_adapter::scan_dev_symlinks(&dirs.claude_plugins);
    let agent_skills = cc_adapter::scan_agent_skills(&dirs.agents_skills);

    let views = cc_adapter::build_plugin_views(
        &catalog, &cc_cache, &cc_installed, &dev_symlinks, &agent_skills,
    );

    let active = profiles::get_active_profile(&dirs.active_profile_path());

    // CC profiles
    let cc_profiles = vec![
        ("main", dirs.cc_settings_path()),
        ("personal", dirs.cc_profile_settings_path("personal")),
        ("work", dirs.cc_profile_settings_path("work")),
    ];

    for (profile_name, settings_path) in &cc_profiles {
        let is_active = active.as_deref() == Some(*profile_name);
        let marker = if is_active { " (active)" } else { "" };
        println!("PROFILE: {profile_name}{marker}\n");

        let enabled = cc_adapter::load_cc_enabled_plugins(settings_path);
        let heurema_enabled = enabled.iter()
            .filter(|k| k.contains("@emporium") || k.contains("@local"))
            .count();
        let official_enabled = enabled.iter()
            .filter(|k| k.contains("@claude-plugins-official"))
            .count();

        println!("  CC plugins enabled:  {} ({} heurema, {} official)",
            enabled.len(), heurema_enabled, official_enabled);
        println!("  Codex skills:        {}", agent_skills.len());
        println!("  Dev overrides:       {}", dev_symlinks.len());

        let drift: Vec<_> = views.iter()
            .filter(|v| !v.drift.is_empty() && v.drift.iter().any(|d| d.contains("CC cache=")))
            .collect();
        if !drift.is_empty() {
            for d in &drift {
                println!("  Drift:               {}", d.drift.join(", "));
            }
        }
        println!();
    }

    Ok(())
}
```

- [ ] **Step 2: Register in mod.rs**

Add `pub mod status;` to `src/commands/mod.rs`.

- [ ] **Step 3: Add Status subcommand to main.rs clap**

In `src/main.rs`, add to the `Commands` enum:

```rust
/// Cross-platform plugin health view
Status,
```

And in the match block:

```rust
Commands::Status => commands::status::run(),
```

- [ ] **Step 4: Verify it compiles and run**

Run: `cargo check && cargo run -- status`
Expected: Three profile sections with CC plugin counts and drift info

- [ ] **Step 5: Commit**

```bash
git add src/commands/status.rs src/commands/mod.rs src/main.rs
git commit -m "feat: add nex status command — cross-platform health view"
```

---

### Task 11: Add `nex profile` command

**Files:**
- Create: `src/commands/profile.rs`
- Modify: `src/commands/mod.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Create profile.rs with list/show/apply/activate subcommands**

```rust
use crate::core::{cc_adapter, dirs::Dirs, profiles};

pub fn run_list() -> anyhow::Result<()> {
    let dirs = Dirs::new()?;
    let names = profiles::list_profiles(&dirs.nex_profiles_dir());
    let active = profiles::get_active_profile(&dirs.active_profile_path());

    if names.is_empty() {
        println!("No profiles found. Create one at ~/.nex/profiles/<name>.toml");
        return Ok(());
    }

    for name in &names {
        let marker = if active.as_deref() == Some(name.as_str()) { " *" } else { "" };
        println!("  {name}{marker}");
    }
    Ok(())
}

pub fn run_show(name: &str) -> anyhow::Result<()> {
    let dirs = Dirs::new()?;
    let path = dirs.nex_profiles_dir().join(format!("{name}.toml"));
    if !path.exists() {
        anyhow::bail!("profile '{name}' not found at {}", path.display());
    }
    let profile = profiles::Profile::load(&path)?;

    println!("Profile: {name}\n");
    println!("Plugins ({}):", profile.plugins.enable.len());
    for p in &profile.plugins.enable {
        println!("  {p}");
    }
    if !profile.dev.is_empty() {
        println!("\nDev overrides:");
        for (name, path) in &profile.dev {
            println!("  {name} -> {path}");
        }
    }
    println!("\nPlatforms: CC={} Codex={} Gemini={}",
        profile.platforms.claude_code, profile.platforms.codex, profile.platforms.gemini);
    Ok(())
}

pub fn run_apply(name: &str) -> anyhow::Result<()> {
    let dirs = Dirs::new()?;
    let path = dirs.nex_profiles_dir().join(format!("{name}.toml"));
    if !path.exists() {
        anyhow::bail!("profile '{name}' not found at {}", path.display());
    }
    let profile = profiles::Profile::load(&path)?;

    println!("Applying profile: {name}\n");

    // Codex/Gemini: create/remove symlinks in ~/.agents/skills/
    if profile.platforms.codex || profile.platforms.gemini {
        let agent_dir = &dirs.agents_skills;
        std::fs::create_dir_all(agent_dir)?;

        let current_skills = cc_adapter::scan_agent_skills(agent_dir);

        for plugin_name in &profile.plugins.enable {
            if current_skills.contains_key(plugin_name) {
                println!("  [OK] {plugin_name} — Codex/Gemini symlink exists");
                continue;
            }
            // Find source: dev override or skills store
            let source = if let Some(dev_path) = profile.dev.get(plugin_name) {
                let expanded = shellexpand::tilde(dev_path).to_string();
                std::path::PathBuf::from(expanded)
            } else {
                dirs.skills_store.join(plugin_name)
            };

            // Look for platforms/codex/ subdir
            let codex_dir = source.join("platforms").join("codex");
            let link_target = if codex_dir.exists() { codex_dir } else { source.clone() };
            let link_path = agent_dir.join(plugin_name);

            if link_target.exists() {
                std::os::unix::fs::symlink(&link_target, &link_path)?;
                println!("  [NEW] {plugin_name} — symlink created: {} -> {}",
                    link_path.display(), link_target.display());
            } else {
                println!("  [SKIP] {plugin_name} — source not found: {}", source.display());
            }
        }

        // Remove symlinks not in profile
        for (existing, _) in &current_skills {
            if !profile.plugins.enable.contains(existing) {
                let link = agent_dir.join(existing);
                if link.is_symlink() {
                    std::fs::remove_file(&link)?;
                    println!("  [DEL] {existing} — symlink removed (not in profile)");
                }
            }
        }
    }

    // CC: show drift report (read-only)
    if profile.platforms.claude_code {
        println!("\nCC drift report (read-only):");
        let catalog = cc_adapter::load_emporium_catalog(&dirs.emporium_marketplace_path())?;
        let cc_cache = cc_adapter::scan_cc_cache(&dirs.cc_cache_dir());

        for plugin_name in &profile.plugins.enable {
            let emp_ver = catalog.get(plugin_name).map(|c| c.version.as_str());
            let cache_ver = cc_cache.get(plugin_name).map(|s| s.as_str());
            match (emp_ver, cache_ver) {
                (Some(e), Some(c)) if e == c => println!("  [OK] {plugin_name} v{e}"),
                (Some(e), Some(c)) => println!("  [DRIFT] {plugin_name} emporium=v{e} cache=v{c}"),
                (Some(e), None) => println!("  [MISSING] {plugin_name} v{e} — not in CC cache"),
                _ => println!("  [?] {plugin_name} — not in emporium"),
            }
        }
    }

    // Set as active
    profiles::set_active_profile(&dirs.active_profile_path(), name)?;
    println!("\nProfile '{name}' applied and set as active.");

    Ok(())
}

pub fn run_activate(name: &str) -> anyhow::Result<()> {
    let dirs = Dirs::new()?;
    let path = dirs.nex_profiles_dir().join(format!("{name}.toml"));
    if !path.exists() {
        anyhow::bail!("profile '{name}' not found at {}", path.display());
    }
    profiles::set_active_profile(&dirs.active_profile_path(), name)?;
    println!("Active profile set to: {name}");
    Ok(())
}
```

- [ ] **Step 2: Register in mod.rs and main.rs**

Add `pub mod profile;` to `src/commands/mod.rs`.

In `src/main.rs`, add to the `Commands` enum:

```rust
/// Manage nex profiles
Profile {
    #[command(subcommand)]
    action: ProfileAction,
},
```

Add the `ProfileAction` enum:

```rust
#[derive(Subcommand)]
enum ProfileAction {
    /// List all profiles
    List,
    /// Show profile details
    Show { name: String },
    /// Apply profile (create/remove Codex/Gemini symlinks)
    Apply { name: String },
    /// Set active profile without applying
    Activate { name: String },
}
```

And in the match block:

```rust
Commands::Profile { action } => match action {
    ProfileAction::List => commands::profile::run_list(),
    ProfileAction::Show { name } => commands::profile::run_show(&name),
    ProfileAction::Apply { name } => commands::profile::run_apply(&name),
    ProfileAction::Activate { name } => commands::profile::run_activate(&name),
},
```

- [ ] **Step 3: Add shellexpand dependency**

Run: `cd ~/personal/skill7/nex && cargo add shellexpand`

- [ ] **Step 4: Verify it compiles**

Run: `cargo check`

- [ ] **Step 5: Create initial profile TOMLs for testing**

```bash
mkdir -p ~/.nex/profiles
cat > ~/.nex/profiles/work.toml << 'EOF'
[plugins]
enable = ["signum", "herald", "delve", "arbiter", "content-ops", "anvil", "forge", "genesis", "glyph", "reporter", "sentinel"]

[dev]
herald = "~/personal/skill7/devtools/herald"
delve = "~/personal/skill7/devtools/delve"
arbiter = "~/personal/skill7/devtools/arbiter"
content-ops = "~/personal/skill7/publishing/content-ops"
numerai = "~/personal/skill7/trading/numerai"

[platforms]
claude-code = true
codex = true
gemini = true
EOF

cat > ~/.nex/profiles/personal.toml << 'EOF'
[plugins]
enable = ["signum", "delve"]

[platforms]
claude-code = true
codex = true
gemini = true
EOF

echo "work" > ~/.nex/active_profile
```

- [ ] **Step 6: Test profile commands**

Run: `cargo run -- profile list`
Run: `cargo run -- profile show work`
Expected: Lists profiles, shows work profile details

- [ ] **Step 7: Commit**

```bash
git add src/commands/profile.rs src/commands/mod.rs src/main.rs Cargo.toml Cargo.lock
git commit -m "feat: add nex profile command — list/show/apply/activate"
```

---

## Chunk 5: Doctor Extensions

### Task 12: Extend `nex doctor` with 6 new checks

**Files:**
- Modify: `src/commands/doctor.rs`

- [ ] **Step 1: Add cc_adapter import and new checks**

Add at the top of doctor.rs:

```rust
use crate::core::cc_adapter;
```

Add new check functions and call them from `run()`. After the existing per-plugin loop, add:

```rust
// New checks using cc_adapter
let catalog = cc_adapter::load_emporium_catalog(&dirs.emporium_marketplace_path()).unwrap_or_default();
let cc_cache = cc_adapter::scan_cc_cache(&dirs.cc_cache_dir());
let dev_symlinks = cc_adapter::scan_dev_symlinks(&dirs.claude_plugins);

check_nex_devtools(&dirs, &mut issues);
check_emporium_drift(&catalog, &cc_cache, &mut issues);
check_duplicate_plugins(&catalog, &dev_symlinks, &dirs, &mut issues);
check_stale_dev_symlinks(&dev_symlinks, &mut issues);
check_orphan_cache(&catalog, &dirs, &mut issues);
```

Add the check functions:

```rust
fn check_nex_devtools(dirs: &Dirs, issues: &mut Vec<Issue>) {
    let nex_devtools = dirs.claude_plugins
        .join("marketplaces").join("nex-devtools");
    if nex_devtools.exists() {
        issues.push(Issue {
            plugin: String::new(),
            check: "nex-devtools",
            severity: Severity::Warn,
            message: "nex-devtools marketplace exists (deprecated)".to_string(),
            fix: "rm -rf ~/.claude/plugins/marketplaces/nex-devtools".to_string(),
        });
    }
}

fn check_emporium_drift(
    catalog: &HashMap<String, cc_adapter::CatalogPlugin>,
    cc_cache: &HashMap<String, String>,
    issues: &mut Vec<Issue>,
) {
    for (name, cat) in catalog {
        if cat.version.is_empty() { continue; }
        if let Some(cached) = cc_cache.get(name) {
            if *cached != cat.version {
                issues.push(Issue {
                    plugin: name.clone(),
                    check: "emporium_drift",
                    severity: Severity::Warn,
                    message: format!("emporium=v{} but CC cache=v{cached}", cat.version),
                    fix: "restart `claude` to pull updated cache".to_string(),
                });
            }
        }
    }
}

fn check_duplicate_plugins(
    catalog: &HashMap<String, cc_adapter::CatalogPlugin>,
    dev_symlinks: &HashMap<String, std::path::PathBuf>,
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
        let nex_devtools = dirs.claude_plugins
            .join("marketplaces").join("nex-devtools").join("plugins").join(name);
        if nex_devtools.exists() {
            locations.push("nex-devtools".to_string());
        }
        if locations.len() > 1 {
            issues.push(Issue {
                plugin: name.clone(),
                check: "duplicate",
                severity: Severity::Warn,
                message: format!("found in {} locations: {}", locations.len(), locations.join(", ")),
                fix: "remove duplicates, keep emporium as primary".to_string(),
            });
        }
    }
}

fn check_stale_dev_symlinks(
    dev_symlinks: &HashMap<String, std::path::PathBuf>,
    issues: &mut Vec<Issue>,
) {
    for (name, target) in dev_symlinks {
        if !target.exists() {
            issues.push(Issue {
                plugin: name.clone(),
                check: "stale_symlink",
                severity: Severity::Warn,
                message: format!("dev symlink target missing: {}", target.display()),
                fix: format!("rm ~/.claude/plugins/{name}"),
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
    let Ok(entries) = fs::read_dir(&emporium_cache) else { return };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if !catalog.contains_key(&name) {
            issues.push(Issue {
                plugin: name.clone(),
                check: "orphan_cache",
                severity: Severity::Warn,
                message: "in CC cache but not in emporium catalog".to_string(),
                fix: format!("rm -rf ~/.claude/plugins/cache/emporium/{name}"),
            });
        }
    }
}
```

- [ ] **Step 2: Add HashMap import**

Add `use std::collections::HashMap;` at the top if not present.

- [ ] **Step 3: Verify it compiles and run**

Run: `cargo check && cargo run -- doctor`
Expected: Shows existing checks + new nex-devtools warning + any drift

- [ ] **Step 4: Commit**

```bash
git add src/commands/doctor.rs
git commit -m "feat: extend nex doctor with 6 emporium-aware checks"
```

---

## Chunk 6: Version Bump + Build + Test

### Task 13: Bump version, build, full test

**Files:**
- Modify: `Cargo.toml` (version bump to 0.8.0)

- [ ] **Step 1: Bump version in Cargo.toml**

Change `version = "0.7.0"` to `version = "0.8.0"`.

- [ ] **Step 2: Full build**

Run: `cargo build --release`
Expected: Compiles without errors or warnings

- [ ] **Step 3: Install new binary**

Run: `cp target/release/nex ~/.local/bin/nex`

- [ ] **Step 4: Smoke test all new commands**

```bash
nex list
nex check
nex status
nex profile list
nex profile show work
nex doctor
```

Expected: All commands work, show emporium plugins, detect drift

- [ ] **Step 5: Commit and tag**

```bash
git add Cargo.toml Cargo.lock
git commit -m "chore: bump version to 0.8.0"
```

---

## Chunk 7: Migration (nex-devtools cleanup)

### Task 14: Delete nex-devtools and clean up

This is a manual migration step, not code. Run AFTER verifying all commands work.

- [ ] **Step 1: Uninstall signum from nex-devtools in CC**

Run: `jq 'del(.["signum@nex-devtools"])' ~/.claude/plugins/installed_plugins.json > /tmp/ip.json && mv /tmp/ip.json ~/.claude/plugins/installed_plugins.json`

- [ ] **Step 2: Delete nex-devtools marketplace**

```bash
rm -rf ~/.claude/plugins/marketplaces/nex-devtools
jq 'del(.["nex-devtools"])' ~/.claude/plugins/known_marketplaces.json > /tmp/km.json && mv /tmp/km.json ~/.claude/plugins/known_marketplaces.json
rm -rf ~/.claude/plugins/cache/nex-devtools
```

- [ ] **Step 3: Verify nex doctor shows no nex-devtools warning**

Run: `nex doctor`
Expected: No "nex-devtools" warning

- [ ] **Step 4: Verify nex list still shows all plugins**

Run: `nex list`
Expected: 12 emporium plugins visible

- [ ] **Step 5: Commit cleanup notes**

No code changes — migration is filesystem-only.
