use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Profile {
    #[serde(default)]
    pub plugins: PluginsSection,
    #[serde(default)]
    pub dev: HashMap<String, String>,
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

fn default_true() -> bool {
    true
}

impl Default for PlatformsSection {
    fn default() -> Self {
        Self {
            claude_code: true,
            codex: true,
            gemini: true,
        }
    }
}

impl Profile {
    pub fn load(path: &Path) -> Result<Self> {
        let content = fs::read_to_string(path)?;
        Ok(toml::from_str(&content)?)
    }
}

/// List all profile names from ~/.nex/profiles/*.toml
pub fn list_profiles(profiles_dir: &Path) -> Vec<String> {
    let Ok(entries) = fs::read_dir(profiles_dir) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .flatten()
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
    fs::read_to_string(path)
        .ok()
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
