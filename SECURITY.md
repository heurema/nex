# Security Policy

## Threat Model

skill7 is a local CLI tool that installs plugins from trusted registries
into the user's home directory. The trust boundary is:

- **Registry** — trusted (fetched over HTTPS, cached locally with TTL)
- **Plugin packages** — verified via SHA-256 integrity check before install
- **Local filesystem** — trusted (owned by the user, standard Unix permissions)

## Defenses (v0.2.0)

- Input validation: plugin/category names restricted to `[a-z0-9-]+`
- Path traversal prevention: `../`, `/`, absolute paths rejected
- Symlink rejection: packages containing symlinks are rejected during SHA-256 verification
- Adapter path canonicalization: install targets verified to stay within managed directories
- Atomic state writes: `NamedTempFile` + `persist` for `installed.json`, `known_marketplaces.json`, registry cache
- File lock: exclusive `flock` prevents concurrent operations

## Known Limitations

### Local attacker symlink pre-planting (won't fix)

A local attacker with write access to `~/.claude/plugins/`, `~/.agents/skills/`,
or `~/.skill7/` could pre-plant symlinks to redirect skill7's file operations
to arbitrary locations. This is outside the threat model:

- skill7 assumes the user's home directory structure is not attacker-controlled
- Defense requires OS-level protections (file permissions, MAC policies)
- `canonicalize()` checks are applied where practical but cannot prevent
  symlink replacement between check and use (TOCTOU)

This matches the security model of other local package managers (cargo, npm, pip)
which also trust the home directory.

## Reporting

Report security issues via GitHub Issues (private disclosure not yet configured).
