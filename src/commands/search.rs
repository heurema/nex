use crate::core::{cc_adapter, dirs::Dirs, registry::Registry, state::InstalledState};

struct SearchEntry {
    name: String,
    version: String,
    platform_count: usize,
    description: String,
    installed: bool,
}

pub fn run(query: Option<&str>, category: Option<&str>) -> anyhow::Result<()> {
    let dirs = Dirs::new()?;
    let registry = Registry::load(&dirs.registry_path(), false)?;
    let state = InstalledState::load(&dirs.installed_path())?;
    let live = cc_adapter::load_live_plugins(&dirs)?;

    let q = query.unwrap_or("").to_lowercase();

    let mut names: Vec<String> = registry.packages.keys().cloned().collect();
    for name in live.keys() {
        if !names.contains(name) {
            names.push(name.clone());
        }
    }
    names.sort();

    let results: Vec<SearchEntry> = names
        .into_iter()
        .filter_map(|name| {
            let registry_pkg = registry.get(&name);
            let live_pkg = live.get(&name);

            let category_value = registry_pkg
                .map(|pkg| pkg.category.clone())
                .or_else(|| live_pkg.map(|plugin| plugin.category.clone()))
                .unwrap_or_default();
            let description = registry_pkg
                .map(|pkg| pkg.description.clone())
                .or_else(|| live_pkg.map(|plugin| plugin.description.clone()))
                .unwrap_or_default();

            let cat_match = category.is_none_or(|c| category_value == c);
            let text_match = q.is_empty()
                || name.to_lowercase().contains(&q)
                || description.to_lowercase().contains(&q);

            if !cat_match || !text_match {
                return None;
            }

            let version = registry_pkg
                .map(|pkg| pkg.version.clone())
                .or_else(|| live_pkg.and_then(|plugin| plugin.version.clone()))
                .unwrap_or_else(|| "—".to_string());
            let platform_count = registry_pkg
                .map(|pkg| pkg.platforms.len())
                .or_else(|| live_pkg.map(|plugin| plugin.platforms.len()))
                .unwrap_or(0);
            let installed = state.plugins.contains_key(&name)
                || live_pkg.is_some_and(|plugin| plugin.is_installed());

            Some(SearchEntry {
                name,
                version,
                platform_count,
                description,
                installed,
            })
        })
        .collect();

    if results.is_empty() {
        if q.is_empty() {
            println!("No plugins found in registry or live discovery.");
        } else {
            println!("No plugins matching '{}'.", query.unwrap_or(""));
        }
        return Ok(());
    }

    println!(
        "{:<16} {:<10} {:<6} {}",
        "PLUGIN", "VERSION", "PLAT", "DESCRIPTION"
    );
    println!("{}", "-".repeat(70));

    for entry in &results {
        let installed = if entry.installed {
            " \x1b[32m✓\x1b[0m"
        } else {
            ""
        };
        let desc = if entry.description.chars().count() > 40 {
            let end = entry
                .description
                .char_indices()
                .nth(39)
                .map(|(i, _)| i)
                .unwrap_or(entry.description.len());
            format!("{}…", &entry.description[..end])
        } else {
            entry.description.clone()
        };
        println!(
            "{:<16} {:<10} {:<6} {}{installed}",
            entry.name, entry.version, entry.platform_count, desc
        );
    }

    println!(
        "\n{} plugin{} found.",
        results.len(),
        if results.len() == 1 { "" } else { "s" }
    );

    Ok(())
}
