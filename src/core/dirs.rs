use std::fs;
use std::path::PathBuf;

pub struct Dirs {
    pub nex_home: PathBuf,       // ~/.nex/
    pub skills_store: PathBuf,   // ~/.skills/
    pub claude_plugins: PathBuf, // ~/.claude/plugins/
    pub codex_skills: PathBuf,   // ~/.codex/skills/
    pub agents_skills: PathBuf,  // ~/.agents/skills/ (Gemini)
}

impl Dirs {
    pub fn new() -> anyhow::Result<Self> {
        let home =
            dirs::home_dir().ok_or_else(|| anyhow::anyhow!("cannot determine home directory"))?;
        let d = Self {
            nex_home: home.join(".nex"),
            skills_store: home.join(".skills"),
            claude_plugins: home.join(".claude").join("plugins"),
            codex_skills: home.join(".codex").join("skills"),
            agents_skills: home.join(".agents").join("skills"),
        };
        Ok(d)
    }

    pub fn ensure_dirs(&self) -> anyhow::Result<()> {
        fs::create_dir_all(&self.nex_home)?;
        fs::create_dir_all(&self.skills_store)?;
        fs::create_dir_all(&self.codex_skills)?;
        fs::create_dir_all(&self.agents_skills)?;
        Ok(())
    }

    pub fn registry_path(&self) -> PathBuf {
        self.nex_home.join("registry.json")
    }

    pub fn installed_path(&self) -> PathBuf {
        self.nex_home.join("installed.json")
    }

    pub fn lock_path(&self) -> PathBuf {
        self.nex_home.join("nex.lock")
    }

    // ac-002: validate category against [a-z0-9-]+ to prevent path traversal
    pub fn marketplace_dir(&self, category: &str) -> anyhow::Result<PathBuf> {
        validate_name(category)?;
        Ok(self
            .claude_plugins
            .join("marketplaces")
            .join(format!("nex-{category}")))
    }

    pub fn cc_installed_plugins_path(&self) -> PathBuf {
        self.claude_plugins.join("installed_plugins.json")
    }

    pub fn cc_settings_path(&self) -> PathBuf {
        self.claude_plugins
            .parent()
            .unwrap_or(&self.claude_plugins)
            .join("settings.json")
    }

    pub fn cc_profile_settings_path(&self, profile_name: &str) -> PathBuf {
        dirs::home_dir()
            .unwrap_or_default()
            .join(".claude-profiles")
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
}

/// Validate that a name matches [a-z0-9-]+ (no slashes, dots, or special chars)
pub fn validate_name(name: &str) -> anyhow::Result<()> {
    if name.is_empty() {
        anyhow::bail!("name must not be empty");
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        anyhow::bail!(
            "invalid name '{}': only lowercase letters, digits, and hyphens are allowed [a-z0-9-]",
            name
        );
    }
    Ok(())
}
