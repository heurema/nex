use crate::core::{platform::Platform, profiles::Profile, state::PlatformStatus, state::Status};
use std::collections::HashMap;

/// Compute target platforms for a plugin, applying precedence:
///   CLI flags > active profile > all detected CLIs
/// Then intersect with package-declared platform support.
pub fn resolve_targets(
    pkg_platforms: &[String],
    cli_cc: bool,
    cli_codex: bool,
    cli_gemini: bool,
    profile: Option<&Profile>,
) -> Vec<Platform> {
    let detected = crate::core::platform::detect_platforms();

    // Precedence 1: explicit CLI flags (if any set)
    let has_cli_flags = cli_cc || cli_codex || cli_gemini;
    let enabled = if has_cli_flags {
        crate::core::platform::filter_platforms(&detected, cli_cc, cli_codex, cli_gemini)
    } else if let Some(prof) = profile {
        // Precedence 2: active profile platform config
        crate::core::platform::filter_platforms(
            &detected,
            prof.platforms.claude_code,
            prof.platforms.codex,
            prof.platforms.gemini,
        )
    } else {
        // Precedence 3: all detected CLIs
        detected
    };

    // Intersect with package-declared platform support
    enabled
        .into_iter()
        .filter(|t| pkg_platforms.iter().any(|p| p == t.label()))
        .collect()
}

/// After install/update, detect platforms that should be present but aren't OK.
/// Returns list of platform labels that are in drift (missing or failed).
pub fn detect_drift(
    targets: &[Platform],
    realized: &HashMap<String, PlatformStatus>,
) -> Vec<String> {
    let mut drifted = Vec::new();
    for target in targets {
        match realized.get(target.label()) {
            Some(ps) if ps.status == Status::Ok => {}
            _ => drifted.push(target.label().to_string()),
        }
    }
    drifted
}
