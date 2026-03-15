use crate::core::dirs::{Dirs, validate_name};
use std::fs;
use std::io::Write;
use std::path::Path;

pub fn add(category: Option<&str>, all: bool) -> anyhow::Result<()> {
    let dirs = Dirs::new()?;
    let categories = if all {
        vec!["devtools", "trading", "creative"]
    } else if let Some(cat) = category {
        // ac-002: validate category name
        validate_name(cat)?;
        vec![cat]
    } else {
        anyhow::bail!("Specify a category (devtools, trading, creative) or use --all");
    };

    for cat in categories {
        // ac-002: validate builtin categories too (safe, but consistent)
        validate_name(cat)?;
        let mp_dir = dirs.marketplace_dir(cat)?;
        let manifest_dir = mp_dir.join(".claude-plugin");
        let plugins_dir = mp_dir.join("plugins");

        fs::create_dir_all(&manifest_dir)?;
        fs::create_dir_all(&plugins_dir)?;

        // Fix 4: verify marketplace dir resolves within dirs.claude_plugins (prevent symlink redirect)
        {
            let expected_base = dirs.claude_plugins.canonicalize()
                .map_err(|e| anyhow::anyhow!("cannot canonicalize claude_plugins dir: {e}"))?;
            let mp_dir_canonical = mp_dir.canonicalize()
                .map_err(|e| anyhow::anyhow!("cannot canonicalize marketplace dir: {e}"))?;
            if !mp_dir_canonical.starts_with(&expected_base) {
                anyhow::bail!("marketplace directory resolves outside managed tree — aborting for security");
            }
        }

        // Write minimal marketplace.json if not exists
        let manifest_path = manifest_dir.join("marketplace.json");
        if !manifest_path.exists() {
            let json = serde_json::json!({
                "name": format!("skill7-{cat}"),
                "owner": { "name": "heurema" },
                "metadata": {
                    "description": format!("heurema {cat} plugins"),
                    "pluginRoot": "./plugins"
                },
                "plugins": []
            });
            fs::write(&manifest_path, serde_json::to_string_pretty(&json)?)?;
        }

        // ac-009: register marketplace in known_marketplaces.json
        let marketplace_name = format!("skill7-{cat}");
        register_marketplace(&marketplace_name, &mp_dir, &dirs)?;

        println!("Created marketplace: skill7-{cat}");
        println!("  {}", mp_dir.display());
    }

    println!("\nMarketplaces ready. Plugins installed via `skill7 install` will appear here.");
    Ok(())
}

// ac-009: register_marketplace writes to known_marketplaces.json
fn register_marketplace(marketplace_name: &str, marketplace_dir: &Path, dirs: &Dirs) -> anyhow::Result<()> {
    let known_path = dirs.claude_plugins.join("known_marketplaces.json");
    let mut known: serde_json::Value = if known_path.exists() {
        let content = fs::read_to_string(&known_path)?;
        serde_json::from_str(&content).unwrap_or_else(|_| serde_json::json!({}))
    } else {
        serde_json::json!({})
    };

    if known.get(marketplace_name).is_some() {
        return Ok(());
    }

    known[marketplace_name] = serde_json::json!({
        "source": {
            "source": "directory",
            "path": marketplace_dir.to_string_lossy()
        },
        "installLocation": marketplace_dir.to_string_lossy(),
    });

    let parent = if let Some(p) = known_path.parent() {
        fs::create_dir_all(p)?;
        p.to_path_buf()
    } else {
        std::path::PathBuf::from(".")
    };
    // Security: use NamedTempFile (not a predictable path) to prevent symlink-follow attacks
    let mut tmp = tempfile::NamedTempFile::new_in(&parent)?;
    tmp.write_all(serde_json::to_string_pretty(&known)?.as_bytes())?;
    tmp.flush()?;
    tmp.persist(&known_path)
        .map_err(|e| anyhow::anyhow!("failed to persist known_marketplaces.json: {}", e.error))?;
    eprintln!("Registered marketplace: {marketplace_name}");
    Ok(())
}

pub fn list() -> anyhow::Result<()> {
    let dirs = Dirs::new()?;
    let marketplaces_dir = dirs.claude_plugins.join("marketplaces");

    if !marketplaces_dir.exists() {
        println!("No skill7 marketplaces found. Run `skill7 marketplace add devtools`.");
        return Ok(());
    }

    println!("{:<24} {:<8} {}", "MARKETPLACE", "PLUGINS", "PATH");
    println!("{}", "-".repeat(70));

    for entry in fs::read_dir(&marketplaces_dir)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.starts_with("skill7-") {
            continue;
        }

        let plugins_dir = entry.path().join("plugins");
        let count = if plugins_dir.exists() {
            fs::read_dir(&plugins_dir)?.count()
        } else {
            0
        };

        println!("{:<24} {:<8} {}", name, count, entry.path().display());
    }

    Ok(())
}
