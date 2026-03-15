use crate::core::{dirs::Dirs, state::InstalledState};

pub fn run() -> anyhow::Result<()> {
    let dirs = Dirs::new()?;
    let state = InstalledState::load(&dirs.installed_path())?;

    if state.plugins.is_empty() {
        println!("No plugins installed. Run `skill7 install <name>` to get started.");
        return Ok(());
    }

    println!("{:<16} {:<10} {}", "PLUGIN", "VERSION", "PLATFORMS");
    println!("{}", "-".repeat(60));

    let mut entries: Vec<_> = state.plugins.iter().collect();
    entries.sort_by_key(|(name, _)| (*name).clone());

    for (name, plugin) in entries {
        let platforms: Vec<String> = plugin.platforms.iter().map(|(plat, status)| {
            if status.status == crate::core::state::Status::Ok {
                format!("\x1b[32m{plat}\x1b[0m")
            } else {
                format!("\x1b[31m{plat}✗\x1b[0m")
            }
        }).collect();

        println!("{:<16} {:<10} {}", name, plugin.version, platforms.join(", "));
    }

    Ok(())
}
