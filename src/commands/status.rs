use crate::core::{cc_adapter, dirs::Dirs, profiles};

pub fn run() -> anyhow::Result<()> {
    let dirs = Dirs::new()?;
    let dev_symlinks = cc_adapter::scan_dev_symlinks(&dirs.claude_plugins);
    let views = cc_adapter::load_plugin_views(&dirs)?;
    let codex_skills = cc_adapter::scan_codex_skills(&dirs.codex_skills);
    let gemini_skills = cc_adapter::scan_gemini_skills(&dirs.agents_skills);

    let active = profiles::get_active_profile(&dirs.active_profile_path());

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
        let heurema_enabled = enabled
            .iter()
            .filter(|k| k.contains("@emporium") || k.contains("@local"))
            .count();
        let official_enabled = enabled
            .iter()
            .filter(|k| k.contains("@claude-plugins-official"))
            .count();

        println!(
            "  CC plugins installed: {} ({} heurema, {} official)",
            enabled.len(),
            heurema_enabled,
            official_enabled
        );
        println!("  Codex skills:         {}", codex_skills.len());
        println!("  Gemini skills:        {}", gemini_skills.len());
        println!("  Dev overrides:        {}", dev_symlinks.len());

        let drift: Vec<_> = views
            .iter()
            .filter(|v| !v.drift.is_empty() && v.drift.iter().any(|d| d.contains("cache=")))
            .collect();
        if !drift.is_empty() {
            for d in &drift {
                println!("  Drift:                {}", d.drift.join(", "));
            }
        }
        println!();
    }

    Ok(())
}
