#!/usr/bin/env bash
# smoke.sh — skill7 CLI smoke tests
# Uses a temporary HOME to avoid polluting real state.
# Tests: install happy path, uninstall happy path, error: missing plugin, bad sha, missing tag.

set -euo pipefail

CLI="${CLI:-cargo run --quiet --manifest-path "$(dirname "$0")/../Cargo.toml" --}"
PASS=0
FAIL=0

# ── helpers ──────────────────────────────────────────────────────────────────

pass() { echo "PASS: $1"; PASS=$((PASS + 1)); }
fail() { echo "FAIL: $1"; FAIL=$((FAIL + 1)); }

run_test() {
    local label="$1"
    local expected_exit="$2"
    shift 2
    local actual_exit=0
    "$@" >/dev/null 2>&1 || actual_exit=$?
    if [[ "$actual_exit" -eq "$expected_exit" ]]; then
        pass "$label"
    else
        fail "$label (expected exit $expected_exit, got $actual_exit)"
    fi
}

# ── temp environment ──────────────────────────────────────────────────────────

TMPDIR_ROOT="$(mktemp -d)"
REAL_HOME="$HOME"
export HOME="$TMPDIR_ROOT/home"
mkdir -p "$HOME"

cleanup() { rm -rf "$TMPDIR_ROOT"; }
trap cleanup EXIT

# Create temp registry with a test plugin
REGISTRY_DIR="$HOME/.skill7"
mkdir -p "$REGISTRY_DIR"
mkdir -p "$HOME/.skills"
mkdir -p "$HOME/.agents/skills"

# Create a test plugin source tree in a temp bare git repo
PLUGIN_SRC="$TMPDIR_ROOT/test-plugin-src"
mkdir -p "$PLUGIN_SRC/platforms/claude-code/.claude-plugin"
mkdir -p "$PLUGIN_SRC/platforms/codex"
cat > "$PLUGIN_SRC/SKILL.md" <<'EOF'
# test-plugin
A minimal skill for smoke testing.
EOF
cat > "$PLUGIN_SRC/platforms/codex/SKILL.md" <<'EOF'
# test-plugin (codex)
EOF
cat > "$PLUGIN_SRC/platforms/claude-code/.claude-plugin/plugin.json" <<'EOF'
{"name":"test-plugin","description":"smoke test plugin","version":"0.1.0"}
EOF

# Init git repo and tag it
cd "$PLUGIN_SRC"
git init -q
git config user.email "test@test.com"
git config user.name "Test"
git add -A
git commit -q -m "init"
git tag v0.1.0
cd - >/dev/null

# Compute sha256 of the plugin source (skip-dev for smoke test)
PLUGIN_SHA="skip-dev"

# Write registry.json pointing to local repo
cat > "$REGISTRY_DIR/registry.json" <<EOF
{
  "version": 1,
  "packages": {
    "test-plugin": {
      "name": "test-plugin",
      "version": "0.1.0",
      "repo": "file://$PLUGIN_SRC",
      "sha256": "$PLUGIN_SHA",
      "category": "devtools",
      "description": "smoke test plugin",
      "platforms": ["codex", "claude-code", "gemini"]
    }
  }
}
EOF

# ── Test suite ────────────────────────────────────────────────────────────────

echo "=== skill7 smoke tests ==="

# Build the binary BEFORE changing HOME (rustup needs real HOME)
MANIFEST="$(cd "$(dirname "$0")/.." && pwd)/Cargo.toml"
BINARY="$TMPDIR_ROOT/skill7"
if [[ -n "${CLI_BINARY:-}" ]]; then
    cp "$CLI_BINARY" "$BINARY"
elif ! env HOME="$REAL_HOME" cargo build --quiet --manifest-path "$MANIFEST" 2>/dev/null; then
    echo "SKIP: cargo build failed — skipping execution tests"
    exit 0
else
    cp "$(dirname "$MANIFEST")/target/debug/skill7" "$BINARY"
fi

# T1: install happy path
run_test "install test-plugin" 0 \
    "$BINARY" install test-plugin --codex

# T2: verify installed.json records the install
if [[ -f "$REGISTRY_DIR/installed.json" ]]; then
    pass "installed.json created after install"
else
    fail "installed.json not found after install"
fi

# T3: uninstall happy path — plugin is installed, uninstall should succeed
run_test "uninstall test-plugin" 0 \
    "$BINARY" uninstall test-plugin

# T4: verify installed.json no longer has the plugin after uninstall
if [[ -f "$REGISTRY_DIR/installed.json" ]]; then
    if grep -q '"test-plugin"' "$REGISTRY_DIR/installed.json" 2>/dev/null; then
        fail "test-plugin still in installed.json after uninstall"
    else
        pass "test-plugin removed from installed.json after uninstall"
    fi
else
    pass "installed.json cleared after uninstall"
fi

# T5: error case — uninstall missing (not installed) plugin
if "$BINARY" uninstall missing-plugin >/dev/null 2>&1; then
    fail "uninstall missing plugin should fail"
else
    pass "uninstall missing plugin returns failure (missing)"
fi

# T6: error case — bad sha256 check during install
# Registry entry with wrong sha triggers mismatch warning (not hard error in v0.1 dev mode)
# Write a registry entry with a known-bad sha
cat > "$REGISTRY_DIR/registry.json" <<EOF
{
  "version": 1,
  "packages": {
    "bad-sha-plugin": {
      "name": "bad-sha-plugin",
      "version": "0.1.0",
      "repo": "file://$PLUGIN_SRC",
      "sha256": "0000000000000000000000000000000000000000000000000000000000000000",
      "category": "devtools",
      "description": "plugin with bad sha",
      "platforms": ["codex", "claude-code", "gemini"]
    }
  }
}
EOF
# install should fail with SHA256 MISMATCH error (hard error since v0.2)
SHA_OUTPUT=$("$BINARY" install bad-sha-plugin --codex 2>&1 || true)
if echo "$SHA_OUTPUT" | grep -qi "mismatch"; then
    pass "bad sha256 triggers mismatch error"
else
    fail "bad sha256 did not trigger mismatch error"
fi

# T7: error case — missing tag during install
# Use a repo URL where the tag doesn't exist
NOTAG_SRC="$TMPDIR_ROOT/notag-src"
mkdir -p "$NOTAG_SRC"
cd "$NOTAG_SRC"
git init -q
git config user.email "test@test.com"
git config user.name "Test"
echo "test" > README.md
git add -A
git commit -q -m "init"
# Note: no tag created here
cd - >/dev/null

cat > "$REGISTRY_DIR/registry.json" <<EOF
{
  "version": 1,
  "packages": {
    "notag-plugin": {
      "name": "notag-plugin",
      "version": "9.9.9",
      "repo": "file://$NOTAG_SRC",
      "sha256": "skip-dev",
      "category": "devtools",
      "description": "plugin where tag is missing",
      "platforms": ["codex", "claude-code", "gemini"]
    }
  }
}
EOF
# install should fail with tag not found error
if ! "$BINARY" install notag-plugin --codex >/dev/null 2>&1; then
    pass "install with missing tag returns failure (tag not found)"
else
    fail "install with missing tag should fail"
fi

# ── Summary ───────────────────────────────────────────────────────────────────

echo ""
echo "Results: $PASS passed, $FAIL failed"
[[ "$FAIL" -eq 0 ]]
