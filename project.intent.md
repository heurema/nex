# nex — Cross-CLI Skill Portability for AI Agents

## Intent

nex ensures your AI agent skills, plugins, and workflows are portable across AI CLI tools. If you need to switch from Claude Code to Codex or Gemini CLI — or run all three — your skills follow you. One command to install, one command to sync, zero vendor lock-in.

The key promise: if tomorrow you decide Codex is your primary CLI instead of Claude Code, `nex profile apply work` gives you all your skills there instantly. No migration, no manual symlinks, no lost workflows.

## Core Problem

AI coding agents (Claude Code, Codex CLI, Gemini CLI) each have their own plugin/skill systems with incompatible layouts, config formats, and discovery mechanisms. Without a cross-CLI layer:

- Skills written for one CLI don't work in another
- Switching primary CLI means rebuilding your entire toolkit
- Multi-CLI workflows (e.g., arbiter panels) require manual symlink management
- Version drift across CLIs goes undetected

## What nex Does

1. **Install once, use everywhere** — `nex install signum` creates the right symlinks for Claude Code, Codex, and Gemini simultaneously
2. **Marketplace management** — reads emporium catalog as source of truth, detects drift against CC cache, shows cross-platform health
3. **Profile-based desired state** — `~/.nex/profiles/work.toml` declares which skills are active per CLI, `nex profile apply` syncs symlinks
4. **Release automation** — `nex release patch --execute` bumps version, tags, pushes, and propagates to marketplace in one pipeline
5. **Health monitoring** — `nex doctor` catches duplicates, stale symlinks, orphan caches, version drift across all platforms
6. **Docs actualization** (planned v0.9) — `nex release` auto-generates changelog entries from git log, updates README badges, syncs SKILL.md descriptions

## Design Principles

- **Read-only for CC** — nex never writes to Claude Code internal state. CC discovers plugins via filesystem
- **Layered SSoT** — emporium = catalog truth, CC = runtime truth, nex = Codex/Gemini truth + profiles
- **Graceful degradation** — missing CLI, missing files, corrupted JSON → warning, not crash
- **Universal skill format** — SKILL.md + platforms/ layout works across all three CLIs

## Non-Goals

- IDE plugin management (VS Code, Cursor, Zed extensions)
- Dependency resolution between plugins
- Building/compiling plugins from source
- Replacing CC's built-in marketplace for official Anthropic plugins
