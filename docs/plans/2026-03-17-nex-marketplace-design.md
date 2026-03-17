# nex v0.8.0 — Marketplace Management

**Date:** 2026-03-17
**Status:** Draft
**Approach:** Codex D — layered SSoT (catalog=emporium, CC runtime=CC, Codex/Gemini=nex)
**Panel:** Codex + Gemini consensus on D, divergence on write strategy → Codex read-only wins

## Problem

nex sees 1 of 28 installed plugins. Created its own marketplace (nex-devtools) instead of integrating with emporium. signum exists in 3 places. Version updates require manual sync. No profile awareness.

## Architecture: Layered Source of Truth

```
┌─────────────────────────────────────────────────┐
│                  nex CLI                        │
│  ┌──────────┐  ┌──────────┐  ┌──────────────┐  │
│  │ Catalog   │  │ Profiles │  │ Platform     │  │
│  │ Manager   │  │ Manager  │  │ Adapters     │  │
│  └─────┬─────┘  └─────┬────┘  └──┬───┬───┬──┘  │
│        │              │          │   │   │      │
└────────┼──────────────┼──────────┼───┼───┼──────┘
         │              │          │   │   │
         ▼              ▼          ▼   ▼   ▼
    emporium/      ~/.nex/       CC  Codex Gemini
    marketplace    profiles/    (ro) (rw)  (rw)
    .json          *.toml
```

### Layer 1: Catalog (emporium)

**Owner:** nex
**SSoT:** `emporium/.claude-plugin/marketplace.json`
**Operations:** ref bump, version check, consistency validation, release lifecycle

emporium is the single catalog for all heurema plugins. nex release already manages it. Extend nex list/check/update to read it.

### Layer 2: Platform Runtime State

**Owner:** Each platform owns its own runtime state

| Platform | Runtime State | nex Access |
|----------|--------------|------------|
| Claude Code | `installed_plugins.json`, `settings.json`, `cache/` | **read-only** |
| Codex CLI | `~/.agents/skills/` symlinks | **read-write** |
| Gemini CLI | `~/.agents/skills/` symlinks (shared with Codex) | **read-write** |

nex NEVER writes to CC internal files. CC discovers plugins via filesystem (marketplaces, symlinks, cache).

### Layer 3: Profiles (desired state)

**Owner:** nex
**SSoT:** `~/.nex/profiles/<name>.toml`

```toml
# ~/.nex/profiles/work.toml
[plugins]
# heurema plugins to enable
enable = ["signum", "herald", "delve", "arbiter", "content-ops", "anvil", "forge", "genesis", "glyph", "reporter", "sentinel"]

[dev]
# dev-linked plugins (symlink to source, separate table)
herald = "~/personal/skill7/devtools/herald"
delve = "~/personal/skill7/devtools/delve"
arbiter = "~/personal/skill7/devtools/arbiter"

[platforms]
claude-code = true
codex = true
gemini = true
```

```toml
# ~/.nex/profiles/personal.toml
[plugins]
enable = ["signum", "delve"]

[platforms]
claude-code = true
codex = true
gemini = true
```

## Commands (changes)

### `nex list` (rewrite)

Current: reads only `~/.nex/installed.json` (1 plugin).
New: reads emporium `marketplace.json` + CC `installed_plugins.json` (read-only) + `~/.agents/skills/` symlinks.

```
PLUGIN           VERSION    EMPORIUM  CC     CODEX  GEMINI  DEV
────────────────────────────────────────────────────────────────
signum           4.8.0      v4.8.0    ✓      ✓      —       —
herald           2.1.0      v2.1.0    ✓      —      —       dev→~/...
delve            0.8.1      v0.8.1    ✓      —      —       dev→~/...
arbiter          0.3.0      v0.3.0    ✓      —      —       dev→~/...
anvil            0.1.0      v0.1.0    ✓      —      —       —
content-ops      0.2.0      v0.2.0    ✓      —      —       dev→~/...
...
```

Source priority: emporium marketplace.json → CC installed_plugins.json → ~/.agents/skills/ scan.

### `nex check` (rewrite)

Current: compares `~/.nex/installed.json` vs `registry-v2.json` (1 package).
New: compares emporium refs vs actual installed versions across all platforms.

```
PLUGIN           EMPORIUM    CC CACHE    CODEX     STATUS
──────────────────────────────────────────────────────────
herald           v2.1.0      v2.0.0      —         UPDATE ↑
signum           v4.8.0      v4.8.0      v4.8.0    OK
delve            v0.8.1      v0.8.1      —         OK (dev override)
```

Detects:
- Emporium ref ahead of CC cache → UPDATE available
- Dev symlink overriding cached version → info note
- CC profile has plugin disabled → DISABLED note
- Missing symlink for Codex/Gemini → DRIFT

### `nex update [name|--all]` (rewrite)

For CC: trigger cache refresh by updating emporium ref (CC auto-pulls on next launch).
For Codex/Gemini: update `~/.agents/skills/` symlinks.

Does NOT write to CC `installed_plugins.json` or `cache/`. CC handles its own cache lifecycle.

### `nex status` (new command)

Cross-platform health view. Reads CC profiles (read-only).

```
PROFILE: work (active)

  CC plugins enabled:  27 (11 heurema, 16 official)
  Codex skills:        1 (signum)
  Gemini skills:       1 (signum)
  Dev overrides:       5 (herald, delve, arbiter, content-ops, numerai)
  Drift:               herald CC cache=v2.0.0 but emporium=v2.1.0

PROFILE: personal

  CC plugins enabled:  14 (0 heurema, 14 official)
  Codex skills:        1
  Gemini skills:       1
```

### `nex profile list|show|apply` (new command)

```bash
nex profile list                    # show all profiles
nex profile show work               # show desired state
nex profile apply work              # apply: create/remove symlinks for Codex/Gemini
                                    # for CC: show drift report (read-only)
```

`nex profile apply` for CC shows what needs to change but doesn't modify CC settings. User enables/disables in CC manually or via `claude config`.

### `nex doctor` (extend)

Add checks:
- **nex-devtools exists** → WARN: "nex-devtools is deprecated, remove it"
- **Duplicate plugin** → ERROR: same plugin in local + emporium + nex-devtools
- **Emporium ref drift** → WARN: emporium ref != CC cache version
- **Profile drift** → WARN: desired state != actual state
- **Stale dev symlinks** → WARN: symlink target doesn't exist
- **Orphan cache** → INFO: CC cache dir exists but plugin not in emporium

## Migration Plan

### Step 1: Delete nex-devtools (cleanup)

```bash
rm -rf ~/.claude/plugins/marketplaces/nex-devtools
# Remove from known_marketplaces.json
jq 'del(.["nex-devtools"])' ~/.claude/plugins/known_marketplaces.json > tmp && mv tmp ~/.claude/plugins/known_marketplaces.json
# Remove stale cache
rm -rf ~/.claude/plugins/cache/nex-devtools
```

### Step 2: Ensure all heurema plugins are in emporium

Already done — emporium has all 12. Verify refs are current.

### Step 3: Implement CC adapter (read-only)

New module: `src/core/cc_adapter.rs`

Reads:
- `~/.claude/plugins/installed_plugins.json` → installed plugins with versions
- `~/.claude/plugins/known_marketplaces.json` → marketplace list
- `~/.claude/plugins/marketplaces/emporium/.claude-plugin/marketplace.json` → emporium catalog
- `~/.claude/plugins/cache/` → cached versions (dir listing)
- `~/.claude/settings.json` + `~/.claude-profiles/*/config/settings.json` → enabled plugins per profile

Never writes to any of these.

### Step 4: Implement profile manager

New module: `src/core/profiles.rs`

Reads/writes `~/.nex/profiles/*.toml`.
`nex profile apply` creates/removes symlinks for Codex/Gemini, shows CC drift.

### Step 5: Rewrite list/check/update

Use CC adapter + emporium catalog as data sources instead of `~/.nex/installed.json`.

### Step 6: Deprecate ~/.nex/installed.json

Keep for backward compat (nex ≤0.7), read on fallback only. New state = emporium catalog + CC adapter + profiles.

## Data Model

```
Catalog Plugin {
  name: String,
  version: String,           // from emporium marketplace.json ref
  repo: String,
  description: String,
  category: String,
}

Installed Plugin {
  name: String,
  catalog_version: String,   // emporium ref
  cc_version: Option<String>, // from CC installed_plugins.json
  cc_cache_version: Option<String>, // from CC cache dir
  codex_linked: bool,         // symlink exists in ~/.agents/skills/
  gemini_linked: bool,        // same symlink (shared)
  dev_override: Option<PathBuf>, // if dev symlink exists
  drift: Vec<DriftItem>,
}

enum DriftItem {
  CacheBehindCatalog { catalog: String, cache: String },
  DevOverride { path: PathBuf },
  MissingCodexLink,
  MissingGeminiLink,
  DisabledInProfile { profile: String },
  DuplicateInstall { locations: Vec<String> },
}
```

## Out of Scope

- Writing to CC internal state (installed_plugins.json, settings.json, cache/)
- Managing official CC plugins (claude-plugins-official)
- Managing non-heurema marketplaces (hiya-plugins)
- Auto-enabling plugins in CC profiles (user does this manually)
- Dependency resolution between plugins
- Building/compiling plugins (nex expects git tags)

## Acceptance Criteria

1. `nex list` shows all 12 emporium plugins with versions across all platforms
2. `nex check` detects version drift between emporium ref and CC cache
3. `nex update herald` bumps emporium ref + refreshes Codex/Gemini symlinks
4. `nex status` shows cross-platform health per profile (read-only CC)
5. `nex profile apply work` creates missing Codex/Gemini symlinks
6. `nex doctor` warns about nex-devtools, duplicates, drift
7. nex-devtools marketplace deleted
8. Zero writes to CC internal files

## Spec Review Fixes (14 issues from code-architect)

### Fixed in this revision:

1. **CC versions are git SHAs, not semver** — cc_version comparison only meaningful for emporium plugins. CC adapter compares only `name@emporium` entries. Official plugins ignored.
2. **Multi-scope installs** — `installed_plugins.json` values are arrays. CC adapter returns `Vec<InstallRecord>`, not single record.
3. **CC profiles path** — profiles live at `~/.claude-profiles/{name}/config/settings.json`, not globbable. CC adapter reads known profile dirs: `~/.claude/settings.json` (main) + `~/.claude-profiles/personal/config/settings.json` + `~/.claude-profiles/work/config/settings.json`. Enabled plugins = `enabledPlugins` key in each.
4. **Cache is 3 levels deep** — `cache/<marketplace>/<plugin>/<version>/`. CC adapter uses triple `read_dir`.
5. **known_marketplaces.json write** — existing `nex install` writes to this. For v0.8.0: `nex install` still writes marketplace registration (this is filesystem setup, not CC runtime state). Clarified: "CC internal files" = `installed_plugins.json`, `settings.json`, `blocklist.json`. Marketplace dirs + `known_marketplaces.json` are shared infrastructure.
6. **nex update for CC** — clarified: `nex update` does NOT bump emporium ref (that's `nex release`). `nex update` for CC = no-op if emporium ref already current. Shows instruction: "Run `claude` to pull updated cache." For Codex/Gemini = re-clone + update symlinks.
7. **Profile activation** — added `~/.nex/active_profile` file (single line: profile name). `nex profile activate work` writes it. Default: first profile found.
8. **TOML nesting** — changed to flat structure: `[plugins]` has `enable`, separate `[dev]` table for dev overrides. No nested `[plugins.dev]`.
9. **Migration: clean installed_plugins.json** — added step: `claude plugin uninstall signum@nex-devtools` before removing nex-devtools dir. Or manual jq delete.
10. **Doctor data source** — doctor iterates emporium catalog as primary source, CC adapter as secondary. No dependency on deprecated `installed.json`.
11. **Emporium schema** — documented: `plugins[]` array with `{name, description, category, source: {source, url, ref}, homepage}`. `ref` = git tag = semver for version comparison.
12. **Effort estimate** — revised to 1400 LOC (see updated table).
13. **Concurrent read safety** — CC adapter uses `fs::read_to_string` + `serde_json::from_str`. If JSON is truncated mid-write, serde fails → nex logs warning, continues with stale/empty data. No crash.
14. **"enabled" vs "installed"** — nex status shows "installed" count from `installed_plugins.json`, "enabled" from profile `settings.json`. Both labeled correctly.

## Risk

- **CC format change:** Mitigated by isolated CC adapter module. If `installed_plugins.json` schema changes, only `cc_adapter.rs` needs update.
- **Race condition:** nex reads CC state that CC is concurrently updating. Mitigated by read-only access — stale reads are safe (worst case: `nex check` shows outdated status).
- **Profile complexity:** Keep profiles simple (plugin list + dev overrides). Don't try to replicate CC settings.

## Effort Estimate

| Component | LOC (est.) | Complexity |
|-----------|-----------|------------|
| CC adapter (read-only) | ~350 | Medium (JSON parsing, 3-level cache scan, multi-scope) |
| Profile manager | ~300 | Medium (TOML read/write, activation, symlink ops, drift) |
| Rewrite list/check/update | ~400 | Medium (merge 4 data sources, drift detection) |
| New status command | ~150 | Low (display logic, profile iteration) |
| Doctor extensions | ~120 | Low (6 new checks against new data sources) |
| Migration (delete nex-devtools) | ~30 | Trivial |
| Tests | ~200 | Medium (mock CC state, profile scenarios) |
| **Total** | **~1550** | **Medium** |
