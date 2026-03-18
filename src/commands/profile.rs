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
        let marker = if active.as_deref() == Some(name.as_str()) {
            " *"
        } else {
            ""
        };
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
    println!(
        "\nPlatforms: CC={} Codex={} Gemini={}",
        profile.platforms.claude_code, profile.platforms.codex, profile.platforms.gemini
    );
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

    if profile.platforms.codex {
        sync_agent_profile_links(
            &dirs.codex_skills,
            "Codex",
            "platforms/codex",
            "platforms/gemini",
            &profile,
            &dirs,
        )?;
    }
    if profile.platforms.gemini {
        sync_agent_profile_links(
            &dirs.agents_skills,
            "Gemini",
            "platforms/gemini",
            "platforms/codex",
            &profile,
            &dirs,
        )?;
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
                (Some(e), Some(c)) => {
                    println!("  [DRIFT] {plugin_name} emporium=v{e} cache=v{c}")
                }
                (Some(e), None) => {
                    println!("  [MISSING] {plugin_name} v{e} \u{2014} not in CC cache")
                }
                _ => println!("  [?] {plugin_name} \u{2014} not in emporium"),
            }
        }
    }

    // Set as active
    profiles::set_active_profile(&dirs.active_profile_path(), name)?;
    println!("\nProfile '{name}' applied and set as active.");

    Ok(())
}

fn sync_agent_profile_links(
    link_dir: &std::path::Path,
    platform_name: &str,
    preferred_adapter: &str,
    fallback_adapter: &str,
    profile: &profiles::Profile,
    dirs: &Dirs,
) -> anyhow::Result<()> {
    std::fs::create_dir_all(link_dir)?;

    let current_skills = if platform_name == "Codex" {
        cc_adapter::scan_codex_skills(link_dir)
    } else {
        cc_adapter::scan_gemini_skills(link_dir)
    };

    for plugin_name in &profile.plugins.enable {
        if current_skills.contains_key(plugin_name) {
            println!("  [OK] {plugin_name} \u{2014} {platform_name} entry exists");
            continue;
        }

        let source = if let Some(dev_path) = profile.dev.get(plugin_name) {
            let expanded = shellexpand::tilde(dev_path).to_string();
            std::path::PathBuf::from(expanded)
        } else {
            dirs.skills_store.join(plugin_name)
        };

        let preferred_dir = source.join(preferred_adapter);
        let fallback_dir = source.join(fallback_adapter);
        let root_skill = source.join("SKILL.md");
        let link_target = if preferred_dir.exists() {
            preferred_dir
        } else if fallback_dir.exists() {
            eprintln!(
                "warning: {plugin_name}: using fallback adapter for {platform_name}. Dedicated platform adapter recommended."
            );
            fallback_dir
        } else if root_skill.is_file() {
            eprintln!(
                "warning: {plugin_name}: using root SKILL.md fallback for {platform_name}. Dedicated platform adapter recommended."
            );
            source.clone()
        } else {
            println!(
                "  [SKIP] {plugin_name} \u{2014} no {platform_name} adapter in {}",
                source.display()
            );
            continue;
        };
        let link_path = link_dir.join(plugin_name);

        std::os::unix::fs::symlink(&link_target, &link_path)?;
        println!(
            "  [NEW] {plugin_name} \u{2014} {platform_name}: {} -> {}",
            link_path.display(),
            link_target.display()
        );
    }

    for (existing, _) in &current_skills {
        if !profile.plugins.enable.contains(existing) {
            let link = link_dir.join(existing);
            if link.is_symlink() {
                std::fs::remove_file(&link)?;
                println!("  [DEL] {existing} \u{2014} removed from {platform_name} profile");
            }
        }
    }

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
