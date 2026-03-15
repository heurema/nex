use crate::core::{dirs::Dirs, lock::FileLock, registry::Registry, state::InstalledState};

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
                let has_cc = inst.platforms.contains_key("claude-code");
                let has_codex = inst.platforms.contains_key("codex");
                let has_gemini = inst.platforms.contains_key("gemini");

                // ac-004: read scope from installed state instead of hardcoding "user"
                let scope = inst.platforms.get("claude-code")
                    .and_then(|p| p.scope.as_deref())
                    .unwrap_or("user")
                    .to_string();

                // deadlock-fix: call install_inner() directly (we already hold the lock)
                match crate::commands::install::install_inner(plugin_name, has_cc, has_codex, has_gemini, &scope, &dirs) {
                    Ok(()) => success += 1,
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
