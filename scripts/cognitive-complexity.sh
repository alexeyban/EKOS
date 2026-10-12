#!/usr/bin/env bash
# Lists Rust functions at or above a cognitive-complexity score (default 31, one over Sonar's
# limit of 30 for rule S3776). See scripts/cogc/src/main.rs for what the estimate is and how
# close it is to SonarCloud's own number.
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
cargo build --quiet --release --manifest-path "$root/scripts/cogc/Cargo.toml"
cd "$root"
find ekos benchmark tests -name '*.rs' -not -path '*/target/*' -not -path '*/tests/fixtures/*' -print0 \
  | xargs -0 "$root/scripts/cogc/target/release/cogc" "${1:-31}" | sort -rn
