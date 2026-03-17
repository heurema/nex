use crate::core::{cc_adapter, dirs::Dirs};

pub fn run(_refresh: bool) -> anyhow::Result<()> {
    let dirs = Dirs::new()?;

    let catalog = cc_adapter::load_emporium_catalog(&dirs.emporium_marketplace_path())?;
    let cc_cache = cc_adapter::scan_cc_cache(&dirs.cc_cache_dir());
    let cc_installed = cc_adapter::load_cc_installed(&dirs.cc_installed_plugins_path());
    let dev_symlinks = cc_adapter::scan_dev_symlinks(&dirs.claude_plugins);
    let agent_skills = cc_adapter::scan_agent_skills(&dirs.agents_skills);

    let views = cc_adapter::build_plugin_views(
        &catalog, &cc_cache, &cc_installed, &dev_symlinks, &agent_skills,
    );

    println!(
        "{:<16} {:<12} {:<12} {:<10} {}",
        "PLUGIN", "EMPORIUM", "CC CACHE", "CODEX", "STATUS"
    );
    println!("{}", "\u{2500}".repeat(62));

    let mut update_count = 0;
    let mut drift_count = 0;

    for v in &views {
        let emp = v
            .catalog_version
            .as_ref()
            .map(|v| format!("v{v}"))
            .unwrap_or_else(|| "\u{2014}".to_string());
        let cache = v
            .cc_cache_version
            .as_ref()
            .map(|v| format!("v{v}"))
            .unwrap_or_else(|| "\u{2014}".to_string());
        let codex = if v.codex_linked { "linked" } else { "\u{2014}" };

        let status = if v.drift.is_empty() {
            "\x1b[32mOK\x1b[0m".to_string()
        } else if v.drift.iter().any(|d| d.contains("cache=")) {
            update_count += 1;
            "\x1b[33mUPDATE \u{2191}\x1b[0m".to_string()
        } else if v.dev_override.is_some() {
            "OK (dev override)".to_string()
        } else {
            drift_count += 1;
            "\x1b[33mDRIFT\x1b[0m".to_string()
        };

        println!("{:<16} {:<12} {:<12} {:<10} {}", v.name, emp, cache, codex, status);
    }

    if update_count > 0 {
        println!("\n{update_count} update(s) available. Restart `claude` to pull updated cache.");
    }
    if drift_count > 0 {
        println!("{drift_count} drift(s) detected. Run `nex doctor` for details.");
    }
    if update_count == 0 && drift_count == 0 {
        println!("\nAll plugins in sync.");
    }

    Ok(())
}
