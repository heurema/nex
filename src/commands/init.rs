use crate::core::dirs::validate_name;
use std::fs;
use std::path::Path;

pub fn run(name: &str) -> anyhow::Result<()> {
    validate_name(name)?;

    let target = Path::new(name);
    // Atomic: fs::create_dir fails if dir already exists (no TOCTOU race)
    fs::create_dir(target)
        .map_err(|e| anyhow::anyhow!("cannot create '{}': {e}", name))?;

    // Universal structure: root + platforms/claude-code/ with CC plugin layout
    let cc_dir = target.join("platforms/claude-code");
    fs::create_dir_all(target.join(".claude-plugin"))?;
    fs::create_dir_all(cc_dir.join(".claude-plugin"))?;
    fs::create_dir_all(cc_dir.join("skills"))?;
    fs::create_dir_all(cc_dir.join("commands"))?;
    fs::create_dir_all(cc_dir.join("agents"))?;
    fs::create_dir_all(cc_dir.join("hooks"))?;

    // Create root .claude-plugin/plugin.json (metadata)
    let plugin_json = serde_json::json!({
        "name": name,
        "version": "0.1.0",
        "description": format!("A nex plugin: {name}"),
        "category": "general",
        "platforms": ["claude-code"],
        "format_version": 1,
    });
    let plugin_json_str = serde_json::to_string_pretty(&plugin_json)?;
    fs::write(target.join(".claude-plugin/plugin.json"), &plugin_json_str)?;

    // Create CC platform plugin.json (CC reads from here when installed)
    let cc_plugin_json = serde_json::json!({
        "name": name,
        "description": format!("A nex plugin: {name}"),
        "version": "0.1.0",
    });
    fs::write(
        cc_dir.join(".claude-plugin/plugin.json"),
        serde_json::to_string_pretty(&cc_plugin_json)?,
    )?;

    // Create SKILL.md
    let skill_md = format!(
        "# {name}\n\nA new nex plugin.\n\n## Usage\n\nDescribe how to use this plugin.\n\n## Example\n\n```\n# example usage\n```\n"
    );
    fs::write(target.join("SKILL.md"), &skill_md)?;

    // Create platforms/claude-code/SKILL.md
    fs::write(cc_dir.join("SKILL.md"), &skill_md)?;

    println!("Initialized plugin '{name}' in ./{name}/");
    println!("  .claude-plugin/plugin.json            (root metadata)");
    println!("  SKILL.md");
    println!("  platforms/claude-code/");
    println!("    .claude-plugin/plugin.json          (CC manifest)");
    println!("    skills/  commands/  agents/  hooks/");
    Ok(())
}
