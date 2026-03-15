use crate::core::{dirs::Dirs, registry::Registry, state::InstalledState};

pub fn run(query: Option<&str>, category: Option<&str>) -> anyhow::Result<()> {
    let dirs = Dirs::new()?;
    let registry = Registry::load(&dirs.registry_path(), false)?;
    let state = InstalledState::load(&dirs.installed_path())?;

    let q = query.unwrap_or("").to_lowercase();

    let mut results: Vec<(&String, &crate::core::registry::Package)> = registry.packages.iter()
        .filter(|(name, pkg)| {
            let cat_match = category.map_or(true, |c| pkg.category == c);
            let text_match = q.is_empty()
                || name.to_lowercase().contains(&q)
                || pkg.description.to_lowercase().contains(&q);
            cat_match && text_match
        })
        .collect();

    results.sort_by_key(|(n, _)| (*n).clone());

    if results.is_empty() {
        if q.is_empty() {
            println!("Registry is empty. Run `nex check --refresh` to update.");
        } else {
            println!("No plugins matching '{}'.", query.unwrap_or(""));
        }
        return Ok(());
    }

    println!("{:<16} {:<10} {:<6} {}",
        "PLUGIN", "VERSION", "PLAT", "DESCRIPTION");
    println!("{}", "-".repeat(70));

    for (name, pkg) in &results {
        let installed = if state.plugins.contains_key(*name) {
            " \x1b[32m✓\x1b[0m"
        } else {
            ""
        };
        let plat_count = pkg.platforms.len();
        let desc = if pkg.description.chars().count() > 40 {
            let end = pkg.description.char_indices()
                .nth(39)
                .map(|(i, _)| i)
                .unwrap_or(pkg.description.len());
            format!("{}…", &pkg.description[..end])
        } else {
            pkg.description.clone()
        };
        println!("{:<16} {:<10} {:<6} {}{installed}",
            name, pkg.version, plat_count, desc);
    }

    println!("\n{} plugin{} found.",
        results.len(), if results.len() == 1 { "" } else { "s" });

    Ok(())
}
