use std::fs;
use std::path::Path;

pub fn run() -> anyhow::Result<()> {
    let cwd = std::env::current_dir()?;

    // Must have .claude-plugin/plugin.json (CC format)
    let plugin_json_path = cwd.join(".claude-plugin/plugin.json");
    if !plugin_json_path.exists() {
        anyhow::bail!("No .claude-plugin/plugin.json found — not a Claude Code plugin");
    }

    // Must NOT already have platforms/ (already converted)
    if cwd.join("platforms").is_dir() {
        anyhow::bail!("platforms/ already exists — plugin appears to be in universal format already");
    }

    // Read plugin.json for metadata
    let content = fs::read_to_string(&plugin_json_path)?;
    let meta: serde_json::Value = serde_json::from_str(&content)
        .map_err(|e| anyhow::anyhow!("failed to parse plugin.json: {e}"))?;
    let name = meta.get("name").and_then(|v| v.as_str()).unwrap_or("plugin");
    let description = meta.get("description").and_then(|v| v.as_str()).unwrap_or("");

    println!("Converting '{name}' to universal format...");

    // 1. Create platforms/claude-code/
    let cc_dir = cwd.join("platforms/claude-code");
    fs::create_dir_all(&cc_dir)?;

    // 2. Copy CC-specific directories into platforms/claude-code/, then remove originals.
    //    Keep root .claude-plugin/ with plugin.json for publish metadata extraction.
    let cc_dirs = [".claude-plugin", "skills", "commands", "agents", "hooks"];
    for dir_name in &cc_dirs {
        let src = cwd.join(dir_name);
        let dst = cc_dir.join(dir_name);
        if src.is_dir() {
            copy_dir_recursive(&src, &dst)?;
            if *dir_name == ".claude-plugin" {
                // Keep root .claude-plugin/ so publish can extract metadata (category, etc.)
                println!("  copied {dir_name}/ → platforms/claude-code/{dir_name}/ (root kept)");
            } else {
                fs::remove_dir_all(&src)?;
                println!("  moved {dir_name}/ → platforms/claude-code/{dir_name}/");
            }
        }
    }

    // 3. Generate root SKILL.md if not present
    if !cwd.join("SKILL.md").exists() {
        let skill_content = format!(
            "# {name}\n\n{description}\n\n## Usage\n\nInstall via nex:\n\n```bash\nnex install {name}\n```\n"
        );
        fs::write(cwd.join("SKILL.md"), &skill_content)?;
        println!("  created SKILL.md (from plugin.json metadata)");
    } else {
        println!("  SKILL.md already exists — kept as-is");
    }

    // 4. Copy SKILL.md into platforms/codex/ and platforms/gemini/ as stubs
    let codex_dir = cwd.join("platforms/codex");
    fs::create_dir_all(&codex_dir)?;
    fs::copy(cwd.join("SKILL.md"), codex_dir.join("SKILL.md"))?;
    println!("  created platforms/codex/SKILL.md");

    let gemini_dir = cwd.join("platforms/gemini");
    fs::create_dir_all(&gemini_dir)?;
    fs::copy(cwd.join("SKILL.md"), gemini_dir.join("SKILL.md"))?;
    println!("  created platforms/gemini/SKILL.md");

    println!("\nConversion complete. Review the generated files:");
    println!("  SKILL.md                        — root skill description");
    println!("  platforms/claude-code/           — Claude Code plugin (moved)");
    println!("  platforms/codex/SKILL.md         — Codex skill (stub, customize)");
    println!("  platforms/gemini/SKILL.md        — Gemini skill (stub, customize)");
    println!("\nRun `nex publish {name}` to generate a registry entry.");
    Ok(())
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> anyhow::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}
