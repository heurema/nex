use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::time::{Duration, SystemTime};

const REGISTRY_URL: &str = "https://raw.githubusercontent.com/heurema/nex/master/registry-v2.json";
const CACHE_TTL: Duration = Duration::from_secs(7 * 24 * 3600); // 7 days

#[derive(Debug, Deserialize, Serialize)]
pub struct Registry {
    pub version: u32,
    pub packages: HashMap<String, Package>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Package {
    pub repo: String,
    pub version: String,
    pub sha256: String,
    pub description: String,
    pub platforms: Vec<String>,
    pub category: String,
    #[serde(default)]
    pub rubric_score: Option<u32>,
    #[serde(default)]
    pub rubric_max: Option<u32>,
}

impl Registry {
    pub fn load(cache_path: &Path, force_refresh: bool) -> anyhow::Result<Self> {
        if !force_refresh && cache_path.exists() {
            if let Ok(meta) = fs::metadata(cache_path) {
                if let Ok(modified) = meta.modified() {
                    let age = SystemTime::now()
                        .duration_since(modified)
                        .unwrap_or(Duration::MAX);
                    if age < CACHE_TTL {
                        let content = fs::read_to_string(cache_path)?;
                        let reg: Registry = serde_json::from_str(&content)?;
                        return Ok(reg);
                    } else {
                        let days = age.as_secs() / 86400;
                        eprintln!("Registry is {days} days old. Refreshing...");
                    }
                }
            }
        }

        // ac-011: fallback to stale cache if fetch fails
        match Self::fetch_and_cache(cache_path) {
            Ok(reg) => Ok(reg),
            Err(e) => {
                eprintln!("Warning: registry fetch failed ({e})");
                if cache_path.exists() {
                    eprintln!("Using stale cache as fallback.");
                    let content = fs::read_to_string(cache_path)?;
                    Ok(serde_json::from_str(&content)?)
                } else {
                    Err(e)
                }
            }
        }
    }

    fn fetch_and_cache(cache_path: &Path) -> anyhow::Result<Self> {
        eprintln!("Fetching registry from {REGISTRY_URL}...");
        // ac-011: 30 second timeout on registry fetch
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()?;
        let resp = client.get(REGISTRY_URL).send()?;
        if !resp.status().is_success() {
            anyhow::bail!("Failed to fetch registry: HTTP {}", resp.status());
        }
        let body = resp.text()?;
        let reg: Registry = serde_json::from_str(&body)?;

        let cache_dir = if let Some(parent) = cache_path.parent() {
            fs::create_dir_all(parent)?;
            parent.to_path_buf()
        } else {
            std::path::PathBuf::from(".")
        };
        // Security: use NamedTempFile to prevent symlink-follow attack on predictable cache path
        let mut tmp = tempfile::NamedTempFile::new_in(&cache_dir)?;
        tmp.write_all(body.as_bytes())?;
        tmp.flush()?;
        tmp.persist(cache_path)
            .map_err(|e| anyhow::anyhow!("failed to persist registry cache: {}", e.error))?;
        eprintln!("Registry cached ({} packages)", reg.packages.len());
        Ok(reg)
    }

    pub fn get(&self, name: &str) -> Option<&Package> {
        self.packages.get(name)
    }
}
