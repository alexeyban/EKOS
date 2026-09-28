#!/usr/bin/env bash
# RustSec advisory check over every Cargo workspace in the repo, plus a lockfile-freshness check.
#
# The same thing `.github/workflows/audit.yml` runs, for local use. Most commits here carry
# `[skip ci]`, which is how RUSTSEC-2026-0285 (rustls) sat unnoticed until devlog_222, so run this
# before merging locally.
#
#   scripts/audit.sh            # fails on any vulnerability or stale lockfile
#
# Unmaintained-crate warnings are printed but do not fail the run: they are tracked in TODO.md and
# replacing a crate is a planned change, not something to force under a red gate.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
workspaces=(ekos tests/integration benchmark)

if ! command -v cargo-audit >/dev/null 2>&1; then
  echo "cargo-audit not installed: cargo install cargo-audit --locked --version 0.22.2" >&2
  exit 2
fi

status=0
for ws in "${workspaces[@]}"; do
  echo "── ${ws}"
  # A lockfile that no longer matches its manifests is what `--locked` refuses. benchmark/ and
  # tests/integration/ drifted this way while CI was skipped (devlog_222).
  if ! (cd "$root/$ws" && cargo metadata --locked --format-version 1 >/dev/null); then
    echo "   stale Cargo.lock — run \`cargo update --workspace\` in ${ws}" >&2
    status=1
  fi
  # Exits non-zero on a vulnerability; unmaintained/unsound warnings are reported, not fatal.
  if ! (cd "$root/$ws" && cargo audit --quiet); then
    status=1
  fi
done
exit "$status"
