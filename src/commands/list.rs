use crate::core::{cc_adapter, dirs::Dirs};

pub fn run() -> anyhow::Result<()> {
    let dirs = Dirs::new()?;
    let views = cc_adapter::load_plugin_views(&dirs)?;
    if views.is_empty() {
        println!("No plugins found in emporium, Claude Code, Codex, or Gemini paths.");
        return Ok(());
    }

    println!(
        "{:<16} {:<10} {:<10} {:<6} {:<6} {:<6} {}",
        "PLUGIN", "VERSION", "EMPORIUM", "CC", "CODEX", "GEM", "DEV"
    );
    println!("{}", "\u{2500}".repeat(75));

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
        let gemini = if v.gemini_linked {
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

        println!(
            "{:<16} {:<10} {:<10} {:<6} {:<6} {:<6} {}",
            v.name, ver, emp, cc, codex, gemini, dev
        );
    }

    let drift_count = views.iter().filter(|v| !v.drift.is_empty()).count();
    if drift_count > 0 {
        println!("\n{drift_count} plugin(s) with drift. Run `nex check` for details.");
    }

    Ok(())
}
