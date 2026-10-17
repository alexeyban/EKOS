#!/usr/bin/env bash
# Assembles the full package for one target (RFC 0185 §1): the ekos binary, a pinned
# standalone Python, the console's dependencies for that platform, the console, its built UI,
# the setup starters and INSTALL.md.
#
#   scripts/package-full.sh <target> <ekos binary> <out dir> <version> [<built ui dir>]
#
# Any target can be assembled on Linux: the Python comes prebuilt and the dependencies are
# wheels for the target's platform (uv --python-platform). Only the ekos binary must have been
# built for that target. Without a built UI directory, the UI is built from this checkout's
# committed tree.
#
# → <out dir>/ekos-<version>-<target>-full.tar.gz (.zip for Windows), and its directory.
#
# RFC 0199: an extension's binary packages the same way under its own name —
# EKOS_PACKAGE_PROGRAM=ekos-lakehouse (the binary, the archive and the starters use it) and
# EKOS_PACKAGE_INSTALL_MD=<its own INSTALL.md>. Unset, the package is EKOS's, as before.
set -euo pipefail

target="$1"; bin="$2"; out="$3"; version="$4"; ui="${5:-}"
repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cache="${EKOS_PACKAGE_CACHE:-${XDG_CACHE_HOME:-$HOME/.cache}/ekos-package}"
lock="$repo/deploy/package/python.lock"
command -v uv >/dev/null || { echo "needs uv (https://docs.astral.sh/uv/)" >&2; exit 2; }

release="$(awk '$1=="RELEASE"{print $2}' "$lock")"
pyver="$(awk '$1=="VERSION"{print $2}' "$lock")"
sha="$(awk -v t="$target" '$1==t{print $2}' "$lock")"
[ -n "$sha" ] || { echo "no Python for $target in $lock" >&2; exit 2; }

windows=0
case "$target" in *windows*) windows=1 ;; esac
program="${EKOS_PACKAGE_PROGRAM:-ekos}"
exe="$program"; [ "$windows" = 1 ] && exe="$program.exe"

name="${program}-${version}-${target}-full"
stage="$out/$name"
rm -rf "$stage"; mkdir -p "$stage/bin" "$stage/console"
cp "$bin" "$stage/bin/$exe"
chmod 755 "$stage/bin/$exe"

# ── Python, verified before it is unpacked ────────────────────────────────────
asset="cpython-${pyver}+${release}-${target}-install_only.tar.gz"
mkdir -p "$cache/python"
if [ ! -f "$cache/python/$asset" ]; then
  curl -fsSL --proto '=https' --tlsv1.2 -o "$cache/python/$asset.part" \
    "https://github.com/astral-sh/python-build-standalone/releases/download/${release}/${asset//+/%2B}"
  mv "$cache/python/$asset.part" "$cache/python/$asset"
fi
actual="$(sha256sum "$cache/python/$asset" | cut -d' ' -f1)"
if [ "$actual" != "$sha" ]; then
  echo "checksum mismatch for $asset: expected $sha, got $actual" >&2
  rm -f "$cache/python/$asset"
  exit 1
fi
tar -xzf "$cache/python/$asset" -C "$stage/console"   # → console/python/

# ── the console's dependencies, for the target platform ───────────────────────
src="$(mktemp -d)"
trap 'rm -rf "$src"' EXIT
if [ "${EKOS_PACKAGE_WORKTREE:-}" = 1 ]; then
  # Testing a change before it is committed. A package to hand out is built from HEAD.
  mkdir -p "$src/web/api" "$src/web/ui"
  (cd "$repo/web/api" && tar --exclude=.venv --exclude=__pycache__ --exclude=.pytest_cache \
    --exclude=.env -cf - .) | tar -x -C "$src/web/api"
  (cd "$repo/web/ui" && tar --exclude=node_modules --exclude=dist -cf - .) | tar -x -C "$src/web/ui"
else
  git -C "$repo" archive HEAD web/api web/ui | tar -x -C "$src"
fi
(cd "$src/web/api" && uv export --frozen --no-dev --no-emit-project --quiet -o "$src/requirements.txt")
uv pip install --quiet --target "$stage/console/site-packages" \
  --python-platform "$target" --python-version "${pyver%.*}" \
  --only-binary :all: --require-hashes -r "$src/requirements.txt"

# ── the console and its UI ────────────────────────────────────────────────────
cp -R "$src/web/api/app" "$stage/console/app"
find "$stage/console/app" -name __pycache__ -prune -exec rm -rf {} +
if [ -z "$ui" ]; then
  (cd "$src/web/ui" && npm ci --ignore-scripts --silent && npm run build --silent >/dev/null)
  ui="$src/web/ui/dist"
fi
cp -R "$ui" "$stage/console/ui"

# ── starters ─────────────────────────────────────────────────────────────────
if [ "$windows" = 1 ]; then
  printf '@echo off\r\ncd /d "%%~dp0"\r\nbin\\ekos.exe setup %%*\r\n' > "$stage/setup.cmd"
else
  cat > "$stage/setup.sh" <<'EOF'
#!/bin/sh
# Starts the EKOS setup wizard in your browser.
cd "$(dirname "$0")" && exec ./bin/ekos setup "$@"
EOF
  chmod 755 "$stage/setup.sh"
  case "$target" in
    *apple-darwin)
      cat > "$stage/setup.command" <<'EOF'
#!/bin/sh
# Double-click to start the EKOS setup wizard. Opening this file (right-click → Open the first
# time) is your consent to run what is in this folder, so the quarantine flag macOS put on the
# download is cleared for it.
cd "$(dirname "$0")" || exit 1
xattr -dr com.apple.quarantine . 2>/dev/null
exec ./bin/ekos setup
EOF
      chmod 755 "$stage/setup.command" ;;
  esac
fi
# The starters run this package's own binary.
if [ "$program" != ekos ]; then
  for f in "$stage/setup.cmd" "$stage/setup.sh" "$stage/setup.command"; do
    [ -f "$f" ] && sed -i.bak -e "s#bin/ekos #bin/$program #g" -e "s#bin\\\\ekos.exe#bin\\\\$program.exe#g" \
      -e "s#bin/ekos\$#bin/$program#" -e "s#EKOS setup wizard#$program setup wizard#" "$f" && rm -f "$f.bak"
  done
fi
commit="$(git -C "$repo" rev-parse --short HEAD)"
install_md="${EKOS_PACKAGE_INSTALL_MD:-$repo/deploy/package/INSTALL-full.md}"
sed -e "s/@VERSION@/$version/g" -e "s/@COMMIT@/$commit/g" -e "s/@TARGET@/$target/g" \
  "$install_md" > "$stage/INSTALL.md"
cp "$repo/LICENSE" "$stage/"

# ── a check on this machine when it can run the target ───────────────────────
host="$(uname -m)-$(uname -s)"
if [ "$target" = "x86_64-unknown-linux-gnu" ] && [ "$host" = "x86_64-Linux" ]; then
  PYTHONPATH="$stage/console:$stage/console/site-packages" PYTHONNOUSERSITE=1 \
    "$stage/console/python/bin/python3" -c "import app.main, keyring, uvicorn; print('console imports')"
  "$stage/bin/$exe" --version >/dev/null
fi

# ── archive ──────────────────────────────────────────────────────────────────
rm -f "$out/$name.tar.gz" "$out/$name.zip"
if [ "$windows" = 1 ]; then
  (cd "$out" && python3 -m zipfile -c "$name.zip" "$name")
else
  tar -C "$out" -czf "$out/$name.tar.gz" "$name"
fi
echo "$out/$name.$([ "$windows" = 1 ] && echo zip || echo tar.gz)"
