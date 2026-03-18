# nex

Cross-CLI plugin distribution for AI agents. Install once, use in Claude Code, Codex, and Gemini.

Single Rust binary that manages plugin installation, health checks, and releases across all three AI CLI platforms.

## Install

```bash
cargo install --git https://github.com/heurema/nex.git --locked
```

Or download from [releases](https://github.com/heurema/nex/releases).

## Quick start

```bash
nex install signum              # install a plugin
nex doctor                      # health check all plugins
nex ship --execute              # auto-detect bump level, release
```

## Commands

| Command | Description |
|---------|-------------|
| `nex install <name>` | Install a plugin for detected CLIs |
| `nex uninstall <name>` | Remove a plugin from all platforms |
| `nex list` | List installed plugins |
| `nex check` | Check for available updates |
| `nex update <name>` | Update a plugin to latest version |
| `nex search <query>` | Search plugins in registry |
| `nex info <name>` | Show detailed plugin information |
| `nex status` | Cross-platform plugin health view |
| `nex doctor` | Check plugin health and detect drift |
| `nex doctor --fix` | Auto-fix detected issues |
| `nex doctor --plugin <name>` | Check specific plugin only |
| `nex init <name>` | Scaffold a new plugin directory |
| `nex convert` | Convert Claude Code plugin to universal format |
| `nex publish <name>` | Compute SHA-256, update local registry |
| `nex dev link <path>` | Create dev symlink for local development |
| `nex dev unlink <name>` | Remove a dev symlink |
| `nex release [level]` | Release pipeline (dry-run by default) |
| `nex release --auto --execute` | Auto-detect bump level and release |
| `nex ship` | Alias for `release --auto` |
| `nex ship --execute` | Auto-detect and release in one command |
| `nex marketplace add <cat>` | Register a marketplace category |
| `nex profile list/apply` | Manage plugin profiles |

## Release pipeline

`nex release` and `nex ship` run a full release pipeline:

```
PREFLIGHT → BUMP → CHANGELOG → DOCS → COMMIT → TAG → PUSH → PROPAGATE → PUBLISH
```

- **BUMP** — update version in plugin.json or Cargo.toml
- **CHANGELOG** — insert version section (template or auto from git log)
- **DOCS** — sync README version refs, SKILL.md descriptions
- **COMMIT** — stage and commit all changes
- **TAG** — create git tag (lightweight or annotated)
- **PUSH** — push branch + tag (exact refs, never `--tags`)
- **PROPAGATE** — update marketplace manifest (auto-adds new plugins)
- **PUBLISH** — update local registry with SHA-256

### Auto-detect bump level

`nex ship` parses conventional commits since last tag:

- `feat:` → minor
- `fix:`, `chore:`, `docs:` → patch
- `BREAKING` / `breaking change` → major

### Configuration

Global: `~/.nex/config.toml`

```toml
[git]
remote = "origin"

[tag]
format = "v{version}"

[marketplaces.emporium]
path = "~/path/to/emporium"
manifest = ".claude-plugin/marketplace.json"
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

## Doctor

`nex doctor` runs health checks across all installed plugins:

- Skill directory existence
- Claude Code marketplace symlinks
- Codex/Gemini agent skill symlinks
- Registry consistency
- SHA-256 integrity (with `--deep`)
- Stale locks, orphan cache, duplicate plugins
- **Release drift** — detects untagged versions and unreleased commits
- **Marketplace ref** — detects stale emporium references

`--fix` auto-applies: remove stale files, create missing tags, push, propagate to marketplace.

## Plugin format

nex supports two plugin formats:

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

**Claude Code only:**
```
my-plugin/
  .claude-plugin/plugin.json
  commands/
  skills/
```

Convert with `nex convert`.

## License

MIT
