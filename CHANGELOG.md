## [0.10.1] - 2026-03-18

# Changelog

## [0.7.0] - 2026-03-16

### Added
- `nex release` command — automated plugin release pipeline
  - 9-stage pipeline: PREFLIGHT → HOOKS(pre) → BUMP → CHANGELOG → COMMIT → TAG → PUSH → PROPAGATE → HOOKS(post)
  - Dry-run by default, `--execute` to mutate
  - Push exact refs (never `--tags`)
  - 3-tier config: `~/.nex/config.toml` > `.nex/release.toml` > CLI flags
  - Marketplace propagation via named config (no filesystem scan)
  - Version bump supports JSON, TOML, regex formats
  - Changelog template insertion
  - Pre/post release hooks with placeholder expansion
  - Shell metacharacter validation for hook safety
- `src/core/config.rs` — global + per-plugin config parsing
- `src/core/marketplace.rs` — marketplace ref update + git operations
- `src/core/changelog.rs` — changelog template section insertion

### Dependencies
- Added `toml = "0.8"`, `semver = "1"`, `pathdiff = "0.2"`

