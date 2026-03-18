use crate::core::{
    cc_adapter,
    dirs::{Dirs, validate_name},
    registry::Registry,
    state::{InstalledState, Status},
};

pub fn run(name: &str) -> anyhow::Result<()> {
    validate_name(name)?;

    let dirs = Dirs::new()?;
    let registry = Registry::load(&dirs.registry_path(), false)?;
    let state = InstalledState::load(&dirs.installed_path())?;
    let live = cc_adapter::load_live_plugins(&dirs)?;

    if let Some(pkg) = registry.get(name) {
        println!("{name} v{}", pkg.version);
        println!("{}", "-".repeat(40));
        println!("Description: {}", pkg.description);
        println!("Category:    {}", pkg.category);
        println!("Platforms:   {}", pkg.platforms.join(", "));
        println!("Repository:  {}", pkg.repo);

        if let (Some(score), Some(max)) = (pkg.rubric_score, pkg.rubric_max) {
            println!("Rubric:      {score}/{max}");
        }

        if let Some(installed) = state.get(name) {
            let status = if installed.version == pkg.version {
                "up to date"
            } else {
                "update available"
            };
            println!("\nInstalled:   v{} ({status})", installed.version);
            let mut plats: Vec<(&String, _)> = installed.platforms.iter().collect();
            plats.sort_by_key(|(p, _)| (*p).clone());
            for (plat, pstatus) in &plats {
                let icon = if pstatus.status == Status::Ok {
                    "\x1b[32m✓\x1b[0m"
                } else {
                    "\x1b[31m✗\x1b[0m"
                };
                println!("  {icon} {plat}");
            }
        } else if let Some(plugin) = live.get(name) {
            if plugin.is_installed() {
                println!("\nInstalled:   discovered (not managed by nex)");
                for platform in &plugin.platforms {
                    println!("  \x1b[32m✓\x1b[0m {platform}");
                }
            } else {
                println!("\nInstalled:   no");
                println!("Install:     nex install {name}");
            }
        } else {
            println!("\nInstalled:   no");
            println!("Install:     nex install {name}");
        }

        return Ok(());
    }

    let plugin = live
        .get(name)
        .ok_or_else(|| anyhow::anyhow!("'{}' not found in registry or live discovery", name))?;

    println!(
        "{} v{}",
        plugin.name,
        plugin.version.as_deref().unwrap_or("unknown")
    );
    println!("{}", "-".repeat(40));
    println!(
        "Description: {}",
        if plugin.description.is_empty() {
            "—"
        } else {
            &plugin.description
        }
    );
    println!(
        "Category:    {}",
        if plugin.category.is_empty() {
            "—"
        } else {
            &plugin.category
        }
    );
    println!(
        "Platforms:   {}",
        if plugin.platforms.is_empty() {
            "unknown".to_string()
        } else {
            plugin.platforms.join(", ")
        }
    );
    println!(
        "Repository:  {}",
        if plugin.repo.is_empty() {
            "—"
        } else {
            &plugin.repo
        }
    );

    if plugin.is_installed() {
        println!("\nInstalled:   discovered (not managed by nex)");
        for platform in &plugin.platforms {
            println!("  \x1b[32m✓\x1b[0m {platform}");
        }
    } else {
        println!("\nInstalled:   no");
        println!("Install:     nex install {name}");
    }

    Ok(())
}
