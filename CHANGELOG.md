## [0.14.1] - 2026-03-19

## [0.16.0] - 2026-03-20

## [0.15.0] - 2026-03-19

## [0.14.2] - 2026-03-19

## [0.14.0] - 2026-03-19

## [0.13.0] - 2026-03-19

# Changelog

## [0.12.0] - 2026-03-18

### Added
- Live-discovery fallback for `nex info`, `nex search`, and `nex doctor --plugin`
- Separate Codex and Gemini skill scanning, including plain skill directories

### Changed
- Codex now uses `~/.codex/skills/`; Gemini continues to use `~/.agents/skills/`
- `list`, `check`, `status`, `info`, `search`, and `doctor` now report merged `nex` state plus live-discovered platform state
- `install`, `uninstall`, and `profile apply` manage Codex and Gemini links independently

### Fixed
- `check` no longer reports false `DRIFT` for Claude Code-only plugins without agent adapters
- Live-discovered plugins such as `delve` now appear consistently across inspection commands
- `doctor` reports legacy Codex installs in `~/.agents/skills/` as migration warnings instead of missing links

## [0.11.0] - 2026-03-18

### Added
- `nex ship` command with auto-detected bump level from conventional commits

### Fixed
- Release and publish edge cases around registry merge, marketplace cleanliness, credential stripping, and symlink handling
- `PUBLISH` is skipped cleanly for non-plugin projects

## [0.10.1] - 2026-03-18

### Added
- Universal release pipeline support for Cargo.toml-based projects without `.claude-plugin/plugin.json`

## [0.7.0] - 2026-03-16

### Added
- `nex release` command — automated plugin release pipeline
- 9-stage pipeline: `PREFLIGHT -> HOOKS(pre) -> BUMP -> CHANGELOG -> COMMIT -> TAG -> PUSH -> PROPAGATE -> HOOKS(post)`
- Dry-run by default, `--execute` to mutate
- Push exact refs, never `--tags`
- 3-tier config: `~/.nex/config.toml` -> `.nex/release.toml` -> CLI flags
- Marketplace propagation via named config, without filesystem scans
- Version bump support for JSON, TOML, and regex version files
- Changelog template insertion
- Pre/post release hooks with placeholder expansion
- Shell metacharacter validation for hook safety

### Dependencies
- Added `toml = "0.8"`, `semver = "1"`, `pathdiff = "0.2"`
