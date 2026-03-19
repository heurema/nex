use crate::core::{cc_adapter, dirs::Dirs, lock::FileLock, profiles, reconcile, registry::Registry, state, state::InstalledState};

pub fn run(name: Option<&str>, all: bool) -> anyhow::Result<()> {
    let dirs = Dirs::new()?;
    dirs.ensure_dirs()?;
    let _lock = FileLock::acquire(&dirs.lock_path())?;
    let registry = Registry::load(&dirs.registry_path(), true)?;
    let state = InstalledState::load(&dirs.installed_path())?;

    let to_update: Vec<String> = if all {
        state.plugins.iter()
            .filter(|(name, plugin)| {
                registry.get(name).is_some_and(|pkg| pkg.version != plugin.version)
            })
            .map(|(name, _)| name.clone())
            .collect()
    } else if let Some(name) = name {
        vec![name.to_string()]
    } else {
        anyhow::bail!("Specify a plugin name or use --all");
    };

    if to_update.is_empty() {
        println!("Everything is up to date.");

        // Show CC cache drift info for emporium plugins
        let catalog = cc_adapter::load_emporium_catalog(&dirs.emporium_marketplace_path()).unwrap_or_default();
        let cc_cache = cc_adapter::scan_cc_cache(&dirs.cc_cache_dir());
        for (name, cat) in &catalog {
            if cat.version.is_empty() { continue; }
            if let Some(cached) = cc_cache.get(name) {
                if *cached != cat.version {
                    println!("{name}: emporium=v{} but CC cache=v{cached}. Restart `claude` to pull update.", cat.version);
                }
            }
        }

        return Ok(());
    }

    // ac-012: hold lock throughout update sequence; do not drop before install
    // _lock is held until end of function (implicit drop)

    let mut success = 0;
    let mut failed = 0;

    for plugin_name in &to_update {
        let installed = state.get(plugin_name);
        let pkg = registry.get(plugin_name);

        match (installed, pkg) {
            (Some(inst), Some(pkg)) if inst.version == pkg.version => {
                println!("{plugin_name}: already at v{}", pkg.version);
            }
            (Some(inst), Some(pkg)) => {
                println!("{plugin_name}: v{} → v{}", inst.version, pkg.version);

                // Reconcile: compute desired platforms from profile/state, not historical install
                let active_profile = profiles::get_active_profile(&dirs.active_profile_path())
                    .and_then(|name| {
                        let path = dirs.nex_profiles_dir().join(format!("{name}.toml"));
                        profiles::Profile::load(&path).ok()
                    });
                let desired = reconcile::resolve_targets(
                    &pkg.platforms,
                    false, false, false, // no CLI flags during update
                    active_profile.as_ref(),
                    Some(&inst.desired_platforms),
                );
                let has_cc = desired.iter().any(|t| *t == crate::core::platform::Platform::ClaudeCode);
                let has_codex = desired.iter().any(|t| *t == crate::core::platform::Platform::Codex);
                let has_gemini = desired.iter().any(|t| *t == crate::core::platform::Platform::Gemini);

                // Preserve origin from old record
                let old_origin = inst.origin;

                // ac-004: read scope from installed state instead of hardcoding "user"
                let scope = inst.platforms.get("claude-code")
                    .and_then(|p| p.scope.as_deref())
                    .unwrap_or("user")
                    .to_string();

                // deadlock-fix: call install_inner() directly (we already hold the lock)
                match crate::commands::install::install_inner(plugin_name, has_cc, has_codex, has_gemini, &scope, &dirs) {
                    Ok(()) => {
                        // Restore origin (install_inner sets Managed, which is correct for update too)
                        let mut st = state::InstalledState::load(&dirs.installed_path()).unwrap_or_default();
                        if let Some(plugin) = st.plugins.get_mut(plugin_name) {
                            plugin.origin = old_origin;
                        }
                        let _ = st.save(&dirs.installed_path());
                        success += 1;
                    }
                    Err(e) => {
                        eprintln!("{plugin_name}: update failed: {e}");
                        failed += 1;
                    }
                }
            }
            (None, _) => {
                eprintln!("{plugin_name}: not installed, use `nex install {plugin_name}`");
                failed += 1;
            }
            (_, None) => {
                eprintln!("{plugin_name}: not found in registry");
                failed += 1;
            }
        }
    }

    if to_update.len() > 1 {
        println!("\nUpdated {success}/{} plugins ({failed} failed)", to_update.len());
    }

    if failed > 0 && success == 0 {
        anyhow::bail!("update failed");
    }

    Ok(())
}
