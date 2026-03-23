# nex — AI Agent Instructions

nex is a Rust CLI for cross-platform AI agent plugin management.
It installs, updates, and distributes plugins across Claude Code,
Codex CLI, and Gemini CLI from a single command.

## Architecture

- `src/cmd/` — CLI subcommands (install, uninstall, check, ship, search)
- `src/registry/` — plugin registry resolution (emporium, git, local)
- `src/platform/` — platform adapters (claude, codex, gemini)
- `src/lib.rs` — core library entry point
- `tests/` — integration tests (run with `cargo test`)

## Conventions

- Rust 2021 edition, stable toolchain
- Error handling: `anyhow` for CLI, `thiserror` for library errors
- No `unwrap()` in library code — propagate errors
- CLI output: structured JSON with `--json` flag, human-readable by default
- All file operations use atomic writes (write to temp, then rename)
- Registry format: `registry-v2.json` — do not modify schema without migration

## Contributing

- Run `cargo clippy` and `cargo test` before submitting
- New platforms: implement the `Platform` trait in `src/platform/`
- New registry sources: implement `RegistrySource` in `src/registry/`
- Keep dependencies minimal — justify new crates in PR description
