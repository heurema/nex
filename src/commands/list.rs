use crate::core::{cc_adapter, dirs::Dirs};

pub fn run() -> anyhow::Result<()> {
    let dirs = Dirs::new()?;

    let catalog = cc_adapter::load_emporium_catalog(&dirs.emporium_marketplace_path())?;
    if catalog.is_empty() {
        println!("No emporium plugins found. Check ~/.claude/plugins/marketplaces/emporium/");
        return Ok(());
    }

    let cc_cache = cc_adapter::scan_cc_cache(&dirs.cc_cache_dir());
    let cc_installed = cc_adapter::load_cc_installed(&dirs.cc_installed_plugins_path());
    let dev_symlinks = cc_adapter::scan_dev_symlinks(&dirs.claude_plugins);
    let agent_skills = cc_adapter::scan_agent_skills(&dirs.agents_skills);

    let views = cc_adapter::build_plugin_views(
        &catalog, &cc_cache, &cc_installed, &dev_symlinks, &agent_skills,
    );

    println!(
        "{:<16} {:<10} {:<10} {:<6} {:<6} {}",
        "PLUGIN", "VERSION", "EMPORIUM", "CC", "CODEX", "DEV"
    );
    println!("{}", "\u{2500}".repeat(68));

    for v in &views {
        let ver = v.catalog_version.as_deref().unwrap_or("\u{2014}");
        let emp = v
            .catalog_version
            .as_ref()
            .map(|v| format!("v{v}"))
            .unwrap_or_else(|| "\u{2014}".to_string());
        let cc = if v.cc_installed {
            "\x1b[32m\u{2713}\x1b[0m"
        } else {
            "\u{2014}"
        };
        let codex = if v.codex_linked {
            "\x1b[32m\u{2713}\x1b[0m"
        } else {
            "\u{2014}"
        };
        let dev = match &v.dev_override {
            Some(p) => {
                let s = p.to_string_lossy();
                if let Some(idx) = s.find("personal") {
                    format!("dev\u{2192}~/{}", &s[idx..])
                } else {
                    format!("dev\u{2192}{s}")
                }
            }
            None => "\u{2014}".to_string(),
        };

        println!("{:<16} {:<10} {:<10} {:<6} {:<6} {}", v.name, ver, emp, cc, codex, dev);
    }

    let drift_count = views.iter().filter(|v| !v.drift.is_empty()).count();
    if drift_count > 0 {
        println!("\n{drift_count} plugin(s) with drift. Run `nex check` for details.");
    }

    Ok(())
}
