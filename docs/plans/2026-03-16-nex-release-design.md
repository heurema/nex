# nex release — Design Document v2

**Date:** 2026-03-16
**Status:** Draft (reviewed by Codex + Gemini panel, 2 rounds)
**Context:** Manual release of delve v0.8.1 required 8 steps across 3 repos. This command automates the full flow.

## Problem

Releasing a plugin today requires:

1. Bump version in `plugin.json` / `Cargo.toml`
2. Update CHANGELOG.md
3. Update README badge (if hardcoded version)
4. `git commit` + `git tag vX.Y.Z` + `git push --tags`
5. Find marketplace `marketplace.json`, update ref
6. `git commit` + `git push` in marketplace repo
7. Clear local cache
8. Reinstall plugin

Missing any step → stale cache, wrong tag, outdated marketplace. `nex doctor` doesn't catch any of this.

## Design Principles

1. **No hardcoded paths** — all paths via config, works for any user
2. **Dry-run by default** — `--execute` required to mutate
3. **Push exact refs** — never `--tags` (can leak unrelated tags)
4. **Configurable at 3 levels** — CLI flags > per-plugin > global > builtins
5. **Fail safe** — validate before mutating, warn on non-critical failures

---

## Configuration

### Precedence Table

| Priority | Source | Location |
|:---:|---|---|
| 1 (highest) | CLI flags | `nex release --tag-format ...` |
| 2 | Plugin config | `.nex/release.toml` (in plugin repo) |
| 3 | Global config | `~/.nex/config.toml` (user-wide) |
| 4 (lowest) | Builtin defaults | hardcoded in nex binary |

### Global Config: `~/.nex/config.toml`

```toml
schema_version = 1

[git]
# Default remote name. Auto-detected if empty.
remote = "origin"
# Default branch. Empty = auto-detect via `git symbolic-ref refs/remotes/{remote}/HEAD`.
branch = ""

[tag]
# Tag format. {version} is replaced with the semver string.
format = "v{version}"
# Use annotated tags (recommended for releases).
annotated = false
# Message for annotated tags. {name} and {version} available.
message = "Release {name} v{version}"

[commit]
# Commit message format for the release commit.
format = "release: v{version}"

[changelog]
# Enable changelog generation. Options: "auto" | "template" | "skip"
# auto = generate from git log, template = insert empty section, skip = don't touch
mode = "template"
# Changelog filename.
filename = "CHANGELOG.md"

# Named marketplace definitions.
# Each entry maps a name to a local repo path + manifest location.
[marketplaces]

  [marketplaces.example]
  # Local path to the marketplace repo checkout.
  path = "~/code/my-marketplace"
  # Path to manifest file, relative to repo root.
  manifest = ".claude-plugin/marketplace.json"
  # Commit message for marketplace updates. {name} and {version} available.
  commit_format = "bump {name} ref to v{version}"

# Post-release hooks. Executed from the plugin repo root after successful push.
# {name}, {version}, {tag}, {marketplace} available as placeholders.
[hooks]
post_release = []
```

### Per-Plugin Config: `.nex/release.toml`

```toml
schema_version = 1

# Name of the marketplace (must match a key in ~/.nex/config.toml [marketplaces]).
# If empty, PROPAGATE step is skipped.
marketplace = ""

# Entry name in marketplace manifest, if different from plugin name.
# marketplace_entry = "my-plugin"

# Files to version-bump. Each entry specifies path + format.
# Supported formats: "json" (key "version"), "toml" (key "version"), "regex".
[[version_files]]
path = ".claude-plugin/plugin.json"
format = "json"

# [[version_files]]
# path = "Cargo.toml"
# format = "toml"

# [[version_files]]
# path = "README.md"
# format = "regex"
# pattern = 'badge/version-([\d.]+)'
# replace = 'badge/version-{version}'

# Override global git/tag/commit settings for this plugin.
# [git]
# remote = "upstream"
# branch = "release"

# [tag]
# format = "{name}-v{version}"

# [commit]
# format = "chore(release): {name} v{version}"

# Per-plugin hooks.
[hooks]
# Run before version bump. Non-zero exit aborts release.
pre_release = []
# Run after successful push. Non-zero exit is a warning, not a failure.
post_release = []
```

### Hardcoded Assumptions → Config Keys

| Original hardcoded value | Config key | Builtin default |
|---|---|---|
| `~/personal/skill7/emporium/` | `marketplaces.<name>.path` | (none, user must configure) |
| `~/personal/heurema/emporium/` | `marketplaces.<name>.path` | (none, user must configure) |
| Filesystem scan for marketplaces | `marketplace` in `.nex/release.toml` | (empty, skip propagation) |
| `origin` | `git.remote` | `"origin"` |
| `main` | `git.branch` | `""` (auto-detect) |
| `git push origin main --tags` | push exact refs only | `git push {remote} HEAD:{branch} refs/tags/{tag}` |
| `release: vX.Y.Z` commit msg | `commit.format` | `"release: v{version}"` |
| `bump {name} ref to vX.Y.Z` | `marketplaces.<name>.commit_format` | `"bump {name} ref to v{version}"` |
| `vX.Y.Z` tag format | `tag.format` | `"v{version}"` |
| Lightweight tags | `tag.annotated` | `false` |
| `plugin.json` + `Cargo.toml` | `version_files[]` in release.toml | `[{path: ".claude-plugin/plugin.json", format: "json"}]` |
| `CHANGELOG.md` | `changelog.filename` | `"CHANGELOG.md"` |
| `claude plugin install` | `hooks.post_release` | `[]` |
| `~/.claude/plugins/cache/...` | `hooks.post_release` | `[]` |

---

## Command

```
nex release [LEVEL] [OPTIONS]

Arguments:
  LEVEL                major | minor | patch (default: patch)

Options:
  --version <VER>      Explicit version (overrides LEVEL)
  --execute            Actually perform the release (default: dry-run)
  --marketplace <NAME> Override marketplace from config
  --tag-format <FMT>   Override tag format
  --no-propagate       Skip marketplace update
  --no-changelog       Skip changelog step
  --path <DIR>         Plugin directory (default: cwd)
  -v, --verbose        Show detailed output
```

## Pipeline

```
PREFLIGHT → HOOKS(pre) → BUMP → CHANGELOG → COMMIT → TAG → PUSH → PROPAGATE → HOOKS(post)
```

### 1. PREFLIGHT

```rust
struct ReleaseContext {
    plugin_name: String,
    current_version: semver::Version,
    next_version: semver::Version,
    plugin_root: PathBuf,
    version_files: Vec<VersionFile>,
    git_remote: String,          // resolved: flag > plugin > global > "origin"
    git_branch: String,          // resolved: flag > plugin > global > auto-detect
    tag_format: String,          // resolved from config
    marketplace: Option<MarketplaceRef>,
}
```

**Checks:**
- Working directory is clean (`git status --porcelain`)
- All `version_files` exist and parse correctly
- All version carriers agree (plugin.json == Cargo.toml == etc.)
- Next version > current version (no downgrade without `--force`)
- No existing tag matching `tag_format` with `next_version`
- Remote is accessible (`git ls-remote {remote}`)
- Branch auto-detection: `git symbolic-ref refs/remotes/{remote}/HEAD | sed 's|.*/||'`
- If `marketplace` configured: marketplace repo exists, is clean, manifest contains plugin entry

**Marketplace resolution:**
1. Read `marketplace` from `.nex/release.toml` (or `--marketplace` flag)
2. Look up name in `~/.nex/config.toml` `[marketplaces]`
3. If not configured → skip PROPAGATE with info message
4. Validate: path exists, is git repo, is clean, manifest contains plugin entry

### 2. HOOKS (pre_release)

Execute `hooks.pre_release` commands from `.nex/release.toml`.
Non-zero exit → abort release.

### 3. BUMP

For each entry in `version_files`:
- `json`: parse, update `"version"` field, write with preserved formatting
- `toml`: parse, update `version` field, write
- `regex`: apply pattern match and replacement

### 4. CHANGELOG

Based on `changelog.mode`:
- **`template`** (default): insert empty `## [X.Y.Z] - YYYY-MM-DD` section. User fills details.
- **`auto`**: generate from `git log {current_tag}..HEAD --oneline --no-merges`, group by conventional commit prefix
- **`skip`**: don't touch changelog

If `CHANGELOG.md` doesn't exist → skip regardless of mode.

### 5. COMMIT

```bash
git add {modified_files}
git commit -m "{commit.format with placeholders resolved}"
```

Only stage files actually modified by BUMP + CHANGELOG steps.

### 6. TAG

```bash
# Lightweight (default)
git tag {resolved_tag}

# Annotated (if tag.annotated = true)
git tag -a {resolved_tag} -m "{tag.message with placeholders}"
```

### 7. PUSH

**Push exact refs only** (never `--tags`):
```bash
git push {remote} HEAD:refs/heads/{branch} refs/tags/{resolved_tag}
```

If push fails → abort. Do not continue to PROPAGATE.

### 8. PROPAGATE

If marketplace configured:
1. `cd {marketplace.path}`
2. `git pull --ff-only` (ensure up to date)
3. Parse manifest, update `ref` field for this plugin entry
4. `git add {manifest_path}`
5. `git commit -m "{commit_format}"`
6. `git push {remote} HEAD:refs/heads/{branch}`

If emporium push fails → warn but don't fail (plugin tag is already pushed).
If emporium is dirty → warn and skip (don't auto-commit unrelated changes).

### 9. HOOKS (post_release)

Execute `hooks.post_release` from `.nex/release.toml` then `~/.nex/config.toml`.
Non-zero exit → warning only.

Example post_release for cache invalidation:
```toml
post_release = [
  "rm -rf ~/.claude/plugins/cache/emporium/{name}/",
  "claude plugin install {name}@emporium"
]
```

---

## Dry-run Output

```
$ nex release patch

nex release v0.8.1 (dry-run)

  Plugin:    delve
  Version:   0.8.0 → 0.8.1
  Config:    .nex/release.toml (found)
  Marketplace: heurema (~/code/emporium)

  Steps:
    ✓ PREFLIGHT   clean tree, 3 commits since v0.8.0
    → BUMP        .claude-plugin/plugin.json
    → CHANGELOG   insert [0.8.1] template
    → COMMIT      "release: v0.8.1"
    → TAG         v0.8.1
    → PUSH        origin/main (exact refs)
    → PROPAGATE   heurema marketplace ref → v0.8.1
    → HOOKS       post_release (2 commands)

  Pass --execute to run.
```

---

## Error Handling

| Error | Stage | Action |
|-------|-------|--------|
| Dirty working tree | PREFLIGHT | Abort |
| Tag already exists | PREFLIGHT | Abort |
| No plugin.json | PREFLIGHT | Abort: "not a plugin directory" |
| Version carriers disagree | PREFLIGHT | Abort: list mismatches |
| Marketplace not in config | PREFLIGHT | Info: skip PROPAGATE |
| Marketplace dirty | PROPAGATE | Warn: skip PROPAGATE |
| Push fails | PUSH | Abort |
| Marketplace push fails | PROPAGATE | Warn, continue to hooks |
| Pre-release hook fails | HOOKS | Abort |
| Post-release hook fails | HOOKS | Warn |
| Downgrade attempt | PREFLIGHT | Abort (unless --force) |
| No CHANGELOG.md | CHANGELOG | Skip |
| Resume after tag-exists | PREFLIGHT | suggest `--no-propagate` to skip |

---

## Doctor Checks (v0.2, deferred)

Three new checks planned for a follow-up release:

1. **`version_tag_drift`** — installed.json vs plugin.json vs git tag mismatch (no network)
2. **`cache_staleness`** — cache version < installed version (no network)
3. **`emporium_ref_drift`** — marketplace ref behind latest tag (--deep, requires network)

---

## Implementation Plan

| Phase | Scope | LOC |
|-------|-------|-----|
| 0 | Config system (`config.toml` + `release.toml` parsing) | ~150 |
| 1 | `release.rs` — PREFLIGHT + BUMP + COMMIT + TAG + PUSH | ~250 |
| 2 | PROPAGATE (marketplace lookup + update) | ~120 |
| 3 | CHANGELOG template insertion | ~60 |
| 4 | Hooks execution | ~50 |
| **Total** | | **~630** |

**Cut from v0.1 (deferred to v0.2):**
- Changelog `auto` mode (conventional commit parsing)
- Doctor checks (3 new)
- `--force` for downgrade
- Resume mechanism for partially failed releases

### File Structure

```
src/commands/
  release.rs       # new: release pipeline
src/core/
  config.rs        # new: config.toml + release.toml parsing
  marketplace.rs   # new: marketplace lookup + update
  changelog.rs     # new: changelog template insertion
  mod.rs           # update: register new modules
```

### Dependencies

No new crates needed:
- `toml` — add for config parsing (small, well-maintained)
- `git2` — already used
- `serde` / `serde_json` — already used
- `semver` — already used or manual parsing

---

## Review History

**Round 1 (Codex + Gemini panel):**
- Emporium discovery hardcoded → replaced with explicit config
- Pipeline not transactional → added preflight validation + warn-on-fail for non-critical steps
- `main` hardcoded → auto-detect with fallback
- `--tags` too broad → push exact refs
- v0.1 scope: cut changelog auto-grouping, README badge regex, INVALIDATE, doctor checks

**Round 2 (Codex + Gemini config review):**
- 3-tier config system: `~/.nex/config.toml` > `.nex/release.toml` > CLI flags
- Marketplace definitions by name, not filesystem scan
- Hooks for pre/post release (replaces hardcoded `claude plugin install`)
- `version_files[]` array replaces hardcoded plugin.json + Cargo.toml
- All placeholders: `{name}`, `{version}`, `{tag}`, `{marketplace}`

## References

- [cargo-release](https://github.com/crate-ci/cargo-release) — dry-run default, version bump + tag + push
- [release-plz](https://release-plz.dev/) — changelog generation, semver from conventional commits
- [cargo-tag](https://crates.io/crates/cargo-tag) — minimal version bump + tag
