# nex

<div align="center">

**Cross-CLI plugin distribution for AI agents**

![Rust](https://img.shields.io/badge/Rust-CLI-5b21b6?style=flat-square)
![Version](https://img.shields.io/badge/version-0.12.0-5b21b6?style=flat-square)
![License](https://img.shields.io/badge/license-MIT-5b21b6?style=flat-square)

```bash
cargo install --git https://github.com/heurema/nex.git --locked
```

</div>

## What it does

nex manages AI agent plugins across Claude Code, Codex, and Gemini from a single CLI. Install once — symlinks, marketplace entries, and agent skill configs are handled automatically.

```
registry ─→ nex install ─→ ~/.skills/{name}/
                          ├─→ Claude Code (marketplace symlink)
                          ├─→ Codex (~/.codex/skills/ symlink)
                          └─→ Gemini (~/.agents/skills/ symlink)
```

For plugin authors, nex handles the full release lifecycle: version bump, changelog, tag, push, marketplace propagation, and registry publish — in one command.

## Install

```bash
cargo install --git https://github.com/heurema/nex.git --locked
```

<details>
<summary>Build from source</summary>

```bash
git clone https://github.com/heurema/nex.git
cd nex
cargo build --release
cp target/release/nex ~/.local/bin/
```

</details>

## Quick start

```bash
nex install signum        # install a plugin
nex doctor                # health check all plugins
nex ship --execute        # auto-detect bump, release
```

## Commands

| Command | What it does |
|---------|-------------|
| `nex install <name>` | Install a plugin for detected CLIs |
| `nex uninstall <name>` | Remove from all platforms |
| `nex list` | List installed plugins |
| `nex search <query>` | Search registry |
| `nex info <name>` | Show detailed plugin info |
| `nex check` | Check for available updates |
| `nex update <name>` | Update to latest version |
| `nex status` | Cross-platform health view |
| `nex doctor` | Health check + drift detection |
| `nex doctor --fix` | Auto-fix issues (stale files, missing tags) |
| `nex init <name>` | Scaffold a new plugin |
| `nex convert` | Convert CC plugin to universal format |
| `nex publish <name>` | Compute SHA-256, update registry |
| `nex dev link <path>` | Symlink for local development |
| `nex dev unlink <name>` | Remove dev symlink |
| `nex release [level]` | Release pipeline (dry-run default) |
| `nex ship` | Auto-detect bump + release |
| `nex ship --execute` | Ship it |
| `nex marketplace add` | Register marketplace category |
| `nex profile apply <name>` | Switch plugin profile |

## Architecture

### Release pipeline

`nex ship` and `nex release` run a 9-stage pipeline:

```
PREFLIGHT → BUMP → CHANGELOG → DOCS → COMMIT → TAG → PUSH → PROPAGATE → PUBLISH
```

- **BUMP** — version in plugin.json or Cargo.toml
- **CHANGELOG** — insert section (template or auto from git log)
- **DOCS** — sync README version refs, SKILL.md descriptions
- **COMMIT + TAG** — atomic release commit + lightweight/annotated tag
- **PUSH** — exact refs (never `--tags`)
- **PROPAGATE** — update marketplace manifest (auto-adds new plugins)
- **PUBLISH** — update local registry with SHA-256

`nex ship` auto-detects bump level from conventional commits:
`feat:` → minor, `fix:` → patch, `BREAKING` → major.

### Discovery model

nex combines two views of plugin state:

- **nex-managed state** from `~/.nex/installed.json`
- **Live discovery** from Claude Code cache/marketplaces, `~/.codex/skills/`, and `~/.agents/skills/`

This keeps `list`, `check`, `status`, `info`, `search`, and `doctor --plugin` aligned even when a plugin was installed outside `nex`.

### Doctor

14 health checks across all installed plugins:

- Skill directories, symlinks, registry consistency
- SHA-256 integrity (`--deep`)
- Stale locks, orphan cache, duplicates
- **Release drift** — untagged versions, unreleased commits
- **Marketplace ref** — stale emporium references
- **Legacy Codex path** — warns when Codex is still linked via `~/.agents/skills/`

`--fix` auto-applies: remove stale files, create tags, push, propagate.
`--plugin <name>` filters to a specific plugin.

### Plugin format

**Universal** (recommended):
```
my-plugin/
  SKILL.md
  .claude-plugin/plugin.json
  platforms/
    claude-code/
    codex/
    gemini/
```

**Claude Code only** (convertible via `nex convert`):
```
my-plugin/
  .claude-plugin/plugin.json
  commands/
  skills/
```

## Configuration

Global config: `~/.nex/config.toml`

```toml
[git]
remote = "origin"

[tag]
format = "v{version}"

[marketplaces.emporium]
path = "~/path/to/emporium"
manifest = ".claude-plugin/marketplace.json"
commit_format = "bump {name} ref to v{version}"
```

Per-project: `.nex/release.toml`

```toml
marketplace = "emporium"

[[version_files]]
path = ".claude-plugin/plugin.json"
format = "json"
```

For non-plugin Rust projects:

```toml
name = "my-cli"

[[version_files]]
path = "Cargo.toml"
format = "toml"
```

## Requirements

- Rust 1.85+ (edition 2024)
- macOS or Linux
- git
- Claude Code, Codex CLI, or Gemini CLI (at least one)

## Privacy

nex runs entirely on your machine. Registry fetches go to GitHub raw content (public). No telemetry, no accounts, no cloud sync.

## Feedback

Found a bug? File an issue at [heurema/nex](https://github.com/heurema/nex/issues) or use [Reporter](https://github.com/heurema/reporter) from Claude Code:

```
/report bug
```

## See also

- [emporium](https://github.com/heurema/emporium) — heurema plugin marketplace
- [signum](https://github.com/heurema/signum) — contract-first AI dev pipeline
- [herald](https://github.com/heurema/herald) — local-first news digest
- [arbiter](https://github.com/heurema/arbiter) — multi-AI orchestrator

## License

[MIT](LICENSE)
