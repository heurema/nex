use crate::core::dirs::validate_name;
use std::fs;
use std::path::Path;

pub fn run(name: &str) -> anyhow::Result<()> {
    validate_name(name)?;

    let target = Path::new(name);
    // Atomic: fs::create_dir fails if dir already exists (no TOCTOU race)
    fs::create_dir(target)
        .map_err(|e| anyhow::anyhow!("cannot create '{}': {e}", name))?;

    // Subdirectories inside the claimed root
    fs::create_dir_all(target.join(".claude-plugin"))?;
    fs::create_dir_all(target.join("skills"))?;
    fs::create_dir_all(target.join("commands"))?;
    fs::create_dir_all(target.join("agents"))?;
    fs::create_dir_all(target.join("hooks"))?;
    fs::create_dir_all(target.join("platforms/claude-code"))?;

    // Create .claude-plugin/plugin.json
    let plugin_json = serde_json::json!({
        "name": name,
        "version": "0.1.0",
        "description": format!("A skill7 plugin: {name}"),
        "category": "general",
        "platforms": ["claude-code"],
    });
    fs::write(
        target.join(".claude-plugin/plugin.json"),
        serde_json::to_string_pretty(&plugin_json)?,
    )?;

    // Create SKILL.md
    let skill_md = format!(
        "# {name}\n\nA new skill7 plugin.\n\n## Usage\n\nDescribe how to use this plugin.\n\n## Example\n\n```\n# example usage\n```\n"
    );
    fs::write(target.join("SKILL.md"), &skill_md)?;

    // Create platforms/claude-code/SKILL.md
    fs::write(target.join("platforms/claude-code/SKILL.md"), &skill_md)?;

    println!("Initialized plugin '{name}' in ./{name}/");
    println!("  .claude-plugin/plugin.json  (version: 0.1.0)");
    println!("  SKILL.md");
    println!("  skills/");
    println!("  commands/");
    println!("  agents/");
    println!("  hooks/");
    println!("  platforms/claude-code/");
    Ok(())
}
