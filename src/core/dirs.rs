use std::path::PathBuf;
use std::fs;

pub struct Dirs {
    pub skill7_home: PathBuf,     // ~/.skill7/
    pub skills_store: PathBuf,    // ~/.skills/
    pub claude_plugins: PathBuf,  // ~/.claude/plugins/
    pub agents_skills: PathBuf,   // ~/.agents/skills/
}

impl Dirs {
    pub fn new() -> anyhow::Result<Self> {
        let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("cannot determine home directory"))?;
        let d = Self {
            skill7_home: home.join(".skill7"),
            skills_store: home.join(".skills"),
            claude_plugins: home.join(".claude").join("plugins"),
            agents_skills: home.join(".agents").join("skills"),
        };
        Ok(d)
    }

    pub fn ensure_dirs(&self) -> anyhow::Result<()> {
        fs::create_dir_all(&self.skill7_home)?;
        fs::create_dir_all(&self.skills_store)?;
        fs::create_dir_all(&self.agents_skills)?;
        Ok(())
    }

    pub fn registry_path(&self) -> PathBuf {
        self.skill7_home.join("registry.json")
    }

    pub fn installed_path(&self) -> PathBuf {
        self.skill7_home.join("installed.json")
    }

    pub fn lock_path(&self) -> PathBuf {
        self.skill7_home.join("skill7.lock")
    }

    // ac-002: validate category against [a-z0-9-]+ to prevent path traversal
    pub fn marketplace_dir(&self, category: &str) -> anyhow::Result<PathBuf> {
        validate_name(category)?;
        Ok(self.claude_plugins
            .join("marketplaces")
            .join(format!("skill7-{category}")))
    }
}

/// Validate that a name matches [a-z0-9-]+ (no slashes, dots, or special chars)
pub fn validate_name(name: &str) -> anyhow::Result<()> {
    if name.is_empty() {
        anyhow::bail!("name must not be empty");
    }
    if !name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-') {
        anyhow::bail!(
            "invalid name '{}': only lowercase letters, digits, and hyphens are allowed [a-z0-9-]",
            name
        );
    }
    Ok(())
}
