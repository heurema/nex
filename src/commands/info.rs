use crate::core::{dirs::{Dirs, validate_name}, registry::Registry, state::{InstalledState, Status}};

pub fn run(name: &str) -> anyhow::Result<()> {
    validate_name(name)?;

    let dirs = Dirs::new()?;
    let registry = Registry::load(&dirs.registry_path(), false)?;
    let state = InstalledState::load(&dirs.installed_path())?;

    let pkg = registry.get(name)
        .ok_or_else(|| anyhow::anyhow!("'{}' not found in registry", name))?;

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
    } else {
        println!("\nInstalled:   no");
        println!("Install:     nex install {name}");
    }

    Ok(())
}
