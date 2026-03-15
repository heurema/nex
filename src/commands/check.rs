use crate::core::{dirs::Dirs, registry::Registry, state::InstalledState};

pub fn run(refresh: bool) -> anyhow::Result<()> {
    let dirs = Dirs::new()?;
    let registry = Registry::load(&dirs.registry_path(), refresh)?;
    let state = InstalledState::load(&dirs.installed_path())?;

    println!("{:<16} {:<12} {:<12} {}", "PLUGIN", "INSTALLED", "AVAILABLE", "STATUS");
    println!("{}", "-".repeat(56));

    let mut has_updates = false;

    // Show installed plugins
    for (name, plugin) in &state.plugins {
        let available = registry.get(name).map(|p| p.version.as_str()).unwrap_or("?");
        let status = if available == "?" {
            "UNKNOWN"
        } else if available == plugin.version {
            "OK"
        } else {
            has_updates = true;
            "\x1b[33mUPDATE ↑\x1b[0m"
        };
        println!("{:<16} {:<12} {:<12} {}", name, plugin.version, available, status);
    }

    // Show available but not installed
    for (name, pkg) in &registry.packages {
        if !state.plugins.contains_key(name) {
            println!("{:<16} {:<12} {:<12} {}", name, "—", pkg.version, "\x1b[36mAVAILABLE\x1b[0m");
        }
    }

    if has_updates {
        println!("\nRun `nex update <name>` or `nex update --all` to update.");
    }

    Ok(())
}
