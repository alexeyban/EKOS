#!/usr/bin/env bash
# End-to-end check of the full package on Linux x86_64 (RFC 0185): assemble it, install it
# with `ekos setup --answers` into a scratch HOME, use the console it starts (local sign-in,
# a setting changed and applied by a restart, a pipeline run), drive the browser wizard with
# Playwright into a second home, and uninstall.
#
#   deploy/e2e/package.sh [<ekos binary>]     (default: ekos/target/release/ekos)
#
# The chat stage uses a local Ollama model (default qwen2.5:1.5b) — never a paid one — and is
# skipped when Ollama does not answer.
# HOME points at a scratch directory throughout, so no shell profile, agent config or service
# of the person running it is touched. Nothing is started at login and no browser is opened;
# the keychain is off. No model is called.
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO="$(cd "$HERE/../.." && pwd)"
BIN="${1:-$REPO/ekos/target/release/ekos}"
WORK="$(mktemp -d "${TMPDIR:-/tmp}/ekos-package-e2e.XXXXXX")"
REAL_HOME="$HOME"   # uv's and Playwright's caches; nothing of EKOS is written there
export HOME="$WORK/user" EKOS_CONSOLE_KEYCHAIN=off
mkdir -p "$HOME"
unset EKOS_HOME EKOS_CONSOLE_PYTHON EKOS_CONSOLE_APP_DIR
pass=0; fail=0
check() { if eval "$2"; then echo "PASS  $1"; pass=$((pass+1)); else echo "FAIL  $1"; fail=$((fail+1)); fi; }
cleanup() {
  for h in "$HOME/.ekos" "$WORK/home2"; do
    [ -x "$h/bin/ekos" ] && "$h/bin/ekos" --home "$h" console stop >/dev/null 2>&1 || true
  done
  [ -n "${WIZARD_PID:-}" ] && kill "$WIZARD_PID" 2>/dev/null || true
  [ "${EKOS_E2E_KEEP:-}" = 1 ] && echo "kept $WORK" || rm -rf "$WORK"
}
trap cleanup EXIT
free_port() { python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1",0)); print(s.getsockname()[1])'; }

echo "== 1. assemble"
ui="$REPO/web/ui/dist"; [ -d "$ui" ] || ui=""
archive="$(HOME="$REAL_HOME" EKOS_PACKAGE_WORKTREE="${EKOS_PACKAGE_WORKTREE:-1}" \
  "$REPO/scripts/package-full.sh" x86_64-unknown-linux-gnu "$BIN" "$WORK/out" 0.0.0-e2e $ui | tail -1)"
check "the package is assembled" '[ -f "$archive" ]'
mkdir -p "$WORK/unpacked" && tar -xzf "$archive" -C "$WORK/unpacked"
PKG="$WORK/unpacked/ekos-0.0.0-e2e-x86_64-unknown-linux-gnu-full"
check "it has the binary, a Python, the console, its UI, a starter" \
  '[ -x "$PKG/bin/ekos" ] && [ -x "$PKG/console/python/bin/python3" ] && [ -f "$PKG/console/app/main.py" ] && [ -f "$PKG/console/ui/index.html" ] && [ -x "$PKG/setup.sh" ]'

echo "== 2. setup from answers"
mkdir -p "$WORK/src/shop/db"
printf 'CREATE TABLE customers (id INT PRIMARY KEY, email TEXT);\n' > "$WORK/src/shop/db/schema.sql"
printf '# Shop\n\nPACKAGEMARKER customers buy things.\n' > "$WORK/src/shop/README.md"
PORT="$(free_port)"
cat > "$WORK/answers.toml" <<EOF
add_to_path = true
[console]
mode = "local"
port = $PORT
[project]
name = "shop"
sources = ["$WORK/src/shop"]
detect = true
[agents]
claude_desktop = true
vscode = true
[chat]
provider = "${EKOS_E2E_CHAT_PROVIDER:-ollama}"
model = "${EKOS_E2E_CHAT_MODEL:-qwen2.5:1.5b}"
[start]
at_login = false
now = true
open = false
EOF
"$PKG/bin/ekos" setup --answers "$WORK/answers.toml" --dry-run > "$WORK/plan.txt"
check "a dry run lists the steps and changes nothing" 'grep -q "create project shop" "$WORK/plan.txt" && [ ! -e "$HOME/.ekos/bin" ]'
"$PKG/bin/ekos" setup --answers "$WORK/answers.toml" | tee "$WORK/setup.txt"
E="$HOME/.ekos/bin/ekos"
check "setup reports every step ok" '! grep -q "^FAIL" "$WORK/setup.txt"'
check "the program is in the home" '[ -x "$E" ] && [ -x "$HOME/.ekos/console/python/bin/python3" ]'
check "PATH line in ~/.profile" 'grep -q "added by ekos setup" "$HOME/.profile"'
check "Claude Desktop and VS Code know the project" \
  'grep -q ekos-shop "$HOME/.config/Claude/claude_desktop_config.json" && grep -q ekos-shop "$HOME/.config/Code/User/mcp.json"'
check "the project exists" '[ -f "$HOME/.ekos/data/default/shop/ekos.toml" ]'
check "the source tree was not written to" '[ "$(LC_ALL=C ls -A "$WORK/src/shop" | tr "\n" " ")" = "README.md db " ]'
check "the console runs" '"$E" console status | grep -q running'

echo "== 3. the console"
"$E" console open --no-browser > "$WORK/open.txt"
link="$(grep -o 'http://[^ ]*code=[0-9a-f]*' "$WORK/open.txt")"
rc=0
(cd "$REPO/web/api" && HOME="$REAL_HOME" E2E_LINK="$link" E2E_BASE="http://127.0.0.1:$PORT" \
  uv run --quiet python "$HERE/package_console.py") || rc=$?
check "the console stage passed" '[ $rc -eq 0 ]'

"$E" console open --no-browser > "$WORK/open2.txt"
link2="$(grep -o 'http://[^ ]*code=[0-9a-f]*' "$WORK/open2.txt")"
rc=0
(cd "$REPO/web/api" && HOME="$REAL_HOME" E2E_LINK="$link2" E2E_BASE="http://127.0.0.1:$PORT" \
  PYTHONPATH="$HERE" uv run --quiet --with playwright python "$HERE/package_browser.py") || rc=$?
check "the browser stage passed" '[ $rc -eq 0 ]'

echo "== 4. the browser wizard"
WPORT="$(free_port)"
"$PKG/bin/ekos" --home "$WORK/home2" setup --no-browser --port "$WPORT" > "$WORK/wizard.txt" 2>&1 &
WIZARD_PID=$!
for _ in $(seq 1 50); do grep -q 'http://' "$WORK/wizard.txt" && break; sleep 0.2; done
wlink="$(grep -o 'http://[^ ]*' "$WORK/wizard.txt" | head -1)"
P2="$(free_port)"
rc=0
(cd "$REPO/web/api" && HOME="$REAL_HOME" E2E_WIZARD="$wlink" E2E_HOME2="$WORK/home2" \
  E2E_PORT2="$P2" E2E_SOURCE="$WORK/src/shop" \
  uv run --quiet --with playwright python "$HERE/package_wizard.py") || rc=$?
check "the wizard stage passed" '[ $rc -eq 0 ]'
check "the wizard installed and started a second console" \
  '"$WORK/home2/bin/ekos" --home "$WORK/home2" console status | grep -q running'
wait "$WIZARD_PID" 2>/dev/null || true; WIZARD_PID=""

echo "== 5. uninstall, from the console's Settings page (RFC 0188)"
"$E" console open --no-browser > "$WORK/open3.txt"
link3="$(grep -o 'http://[^ ]*code=[0-9a-f]*' "$WORK/open3.txt")"
rc=0
(cd "$REPO/web/api" && HOME="$REAL_HOME" E2E_LINK="$link3" E2E_BASE="http://127.0.0.1:$PORT" \
  PYTHONPATH="$HERE" uv run --quiet --with playwright python "$HERE/package_uninstall.py") || rc=$?
check "the uninstall stage passed" '[ $rc -eq 0 ]'
check "the program is gone, the projects stay" '[ ! -e "$HOME/.ekos/bin" ] && [ ! -e "$HOME/.ekos/console" ] && [ -f "$HOME/.ekos/data/default/shop/ekos.toml" ]'
check "the PATH line and agent entries are gone" \
  '! grep -q "added by ekos setup" "$HOME/.profile" && ! grep -q ekos-shop "$HOME/.config/Claude/claude_desktop_config.json"'
check "the console stopped" '! curl -fs "http://127.0.0.1:$PORT/api/health" >/dev/null'
"$WORK/home2/bin/ekos" --home "$WORK/home2" uninstall --purge --yes >/dev/null
check "--purge removes the second home" '[ ! -e "$WORK/home2" ]'

echo; echo "$pass passed, $fail failed"
[ "$fail" -eq 0 ]
