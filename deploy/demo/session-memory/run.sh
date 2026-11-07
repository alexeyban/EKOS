#!/usr/bin/env bash
# Two-session demo of EKOS session memory (RFC 0151), including the stale-memory moment.
# Synthetic workspace — NOT the Plausible Analytics example the plan names (that needs a real,
# built ledger for it). Reproducible from a clean checkout: needs only `cargo` and `python3`.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
EKOS=(cargo run -q --manifest-path "$ROOT/ekos/Cargo.toml" -p ekos --)
WS="$(mktemp -d)"; trap 'rm -rf "$WS"' EXIT
cd "$WS"
mkdir -p src && printf 'CREATE TABLE orders (id INT PRIMARY KEY, total_cents BIGINT);\n' > src/schema.sql
cat > ekos.toml <<'TOML'
[workspace]
name = "session-demo"
[observe]
paths = ["."]
[recover.sql]
default-dialect = "postgres"
[session-memory]
enabled = true
TOML
echo "== build the ledger"; "${EKOS[@]}" init >/dev/null 2>&1 || true
"${EKOS[@]}" build && "${EKOS[@]}" recover && "${EKOS[@]}" resolve && "${EKOS[@]}" compile && "${EKOS[@]}" commit
echo; echo "== SESSION A: record notes"
"${EKOS[@]}" session note "orders.total_cents is stored in cents; divide by 100 for display" --kind decision --rationale "finance reports dollars" --anchor orders --session A
"${EKOS[@]}" session note "partitioning orders by day made thousands of tiny files" --kind dead_end --anchor orders --session A
"${EKOS[@]}" session status
"${EKOS[@]}" session commit
echo; echo "== SESSION B: brief + recall (fresh)"
"${EKOS[@]}" session brief --scope orders
"${EKOS[@]}" session recall "how is the orders total stored"
echo; echo "== code moves: orders gains a column, ledger rebuilt"
printf 'CREATE TABLE orders (id INT PRIMARY KEY, total_cents BIGINT, currency TEXT);\n' > src/schema.sql
"${EKOS[@]}" build && "${EKOS[@]}" recover && "${EKOS[@]}" resolve && "${EKOS[@]}" compile && "${EKOS[@]}" commit
echo; echo "== SESSION C: the same notes now flag themselves"
"${EKOS[@]}" session recall "how is the orders total stored"
echo; echo "== negative control"
"${EKOS[@]}" session recall "kubernetes ingress certificate rotation"
