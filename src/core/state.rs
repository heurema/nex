use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::Path;

// ac-015: Typed enums for Platform and Status
#[allow(dead_code)] // Typed enum for future use; state currently uses String keys
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Platform {
    ClaudeCode,
    Codex,
    Gemini,
}

#[allow(dead_code)]
impl Platform {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ClaudeCode => "claude-code",
            Self::Codex => "codex",
            Self::Gemini => "gemini",
        }
    }
}

impl std::fmt::Display for Platform {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for Platform {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> anyhow::Result<Self> {
        match s {
            "claude-code" => Ok(Self::ClaudeCode),
            "codex" => Ok(Self::Codex),
            "gemini" => Ok(Self::Gemini),
            other => Err(anyhow::anyhow!("unknown platform: {other}")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Ok,
    Failed,
    Skipped,
}

impl Status {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Failed => "failed",
            Self::Skipped => "skipped",
        }
    }
}

impl std::fmt::Display for Status {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PlatformStatus {
    pub status: Status,
    pub r#ref: String,  // "signum@nex-devtools" or "~/.agents/skills/signum"
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>, // CC install scope: "user", "project", "local"
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct InstalledPlugin {
    pub version: String,
    pub sha256: String,
    pub installed_at: String,
    pub source: String,
    pub platforms: HashMap<String, PlatformStatus>,
}

#[derive(Debug, Default, Deserialize, Serialize)]
pub struct InstalledState {
    #[serde(flatten)]
    pub plugins: HashMap<String, InstalledPlugin>,
}

impl InstalledState {
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let content = fs::read_to_string(path)?;
        if content.trim().is_empty() {
            return Ok(Self::default());
        }
        match serde_json::from_str(&content) {
            Ok(state) => Ok(state),
            Err(e) => {
                // Backup corrupted file before resetting
                let backup = path.with_extension("json.corrupted");
                let _ = std::fs::copy(path, &backup);
                eprintln!("ERROR: corrupted installed.json ({e})");
                eprintln!("  Backed up to: {}", backup.display());
                eprintln!("  Starting with empty state. Run `nex install` to re-add plugins.");
                Ok(Self::default())
            }
        }
    }

    // ac-010: Atomic write via temp file + rename
    // Security: use NamedTempFile (not a predictable path) to prevent symlink-follow attacks
    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        let json = serde_json::to_string_pretty(&self)?;
        let parent = if let Some(p) = path.parent() {
            fs::create_dir_all(p)?;
            p
        } else {
            std::path::Path::new(".")
        };
        let mut tmp = tempfile::NamedTempFile::new_in(parent)?;
        tmp.write_all(json.as_bytes())?;
        tmp.flush()?;
        tmp.persist(path)
            .map_err(|e| anyhow::anyhow!("failed to persist installed.json: {}", e.error))?;
        Ok(())
    }

    pub fn get(&self, name: &str) -> Option<&InstalledPlugin> {
        self.plugins.get(name)
    }

    pub fn set(&mut self, name: String, plugin: InstalledPlugin) {
        self.plugins.insert(name, plugin);
    }

    pub fn remove(&mut self, name: &str) -> Option<InstalledPlugin> {
        self.plugins.remove(name)
    }
}
