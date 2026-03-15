// ac-014: use which crate instead of shelling out
// which crate provides portable binary detection across platforms

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Platform {
    ClaudeCode,
    Codex,
    Gemini,
}

impl Platform {
    pub fn label(&self) -> &'static str {
        match self {
            Self::ClaudeCode => "claude-code",
            Self::Codex => "codex",
            Self::Gemini => "gemini",
        }
    }
}

impl std::fmt::Display for Platform {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

pub fn detect_platforms() -> Vec<Platform> {
    let mut platforms = Vec::new();
    if has_binary("claude") {
        platforms.push(Platform::ClaudeCode);
    }
    if has_binary("codex") {
        platforms.push(Platform::Codex);
    }
    if has_binary("gemini") {
        platforms.push(Platform::Gemini);
    }
    platforms
}

pub fn filter_platforms(
    detected: &[Platform],
    claude_code: bool,
    codex: bool,
    gemini: bool,
) -> Vec<Platform> {
    if !claude_code && !codex && !gemini {
        return detected.to_vec();
    }
    let mut result = Vec::new();
    if claude_code && detected.contains(&Platform::ClaudeCode) {
        result.push(Platform::ClaudeCode);
    }
    if codex && detected.contains(&Platform::Codex) {
        result.push(Platform::Codex);
    }
    if gemini && detected.contains(&Platform::Gemini) {
        result.push(Platform::Gemini);
    }
    result
}

fn has_binary(name: &str) -> bool {
    // ac-014: which crate replaces `which` subprocess call
    which::which(name).is_ok()
}
