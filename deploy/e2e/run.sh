#!/usr/bin/env bash
# End-to-end check of a deployment: the production image, a real OIDC provider (Dex), two
# tenants, roles, a limit inside a project, API tokens, the MCP gateway, backup and restore.
#
#   docker build -t ekos:e2e .          # from the repository root; several minutes
#   deploy/e2e/run.sh
#
# Needs Docker with Compose v2.24+, and `uv` (the scenario scripts use the console's own
# Python environment for `httpx`). It makes no model call: the fixture's config names none.
# Two stages fetch a Python package on first use (`mcp`, `playwright`). The browser stage uses
# a Chromium installed for Playwright and prints SKIP when there is none.
#
# It runs its own Compose project (`ekos-e2e`), with its own volume and its own settings, on
# loopback ports 8009 and 5556. It does not read `deploy/.env`. The project and its volume are
# removed at the end; EKOS_E2E_KEEP=1 leaves them running.
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO="$(cd "$HERE/../.." && pwd)"
IMAGE="${EKOS_E2E_IMAGE:-ekos:e2e}"
export EKOS_E2E_PORT="${EKOS_E2E_PORT:-8009}"
WORK="$(mktemp -d "${TMPDIR:-/tmp}/ekos-e2e.XXXXXX")"

docker image inspect "$IMAGE" >/dev/null 2>&1 || {
  echo "image $IMAGE not found. Build it first: docker build -t $IMAGE $REPO" >&2
  exit 2
}

# ── fixture sources: two tenants, each word below appears in exactly one file ─
mkdir -p "$WORK"/sources/acme/{api/db,api/src,handbook/hr} "$WORK"/sources/globex/billing/db
cat > "$WORK/sources/acme/api/db/schema.sql" <<'EOF'
CREATE TABLE customers (id INT PRIMARY KEY, email TEXT NOT NULL);
CREATE TABLE orders (id INT PRIMARY KEY, customer_id INT REFERENCES customers(id), total NUMERIC);
EOF
cat > "$WORK/sources/acme/api/README.md" <<'EOF'
# Payments API

The ACMEAPIMARKER service stores customers and orders.
EOF
cat > "$WORK/sources/acme/handbook/onboarding.md" <<'EOF'
# Onboarding

Welcome. The HANDBOOKOPENMARKER guide explains the first week.
EOF
cat > "$WORK/sources/acme/handbook/hr/pay.md" <<'EOF'
# Compensation bands

ACMESECRETPAYMARKER: band seven starts at a confidential figure.
EOF
cat > "$WORK/sources/globex/billing/db/schema.sql" <<'EOF'
CREATE TABLE invoices (id INT PRIMARY KEY, amount NUMERIC, GLOBEXMARKER_note TEXT);
EOF
cat > "$WORK/sources/globex/billing/README.md" <<'EOF'
# Billing

GLOBEXMARKER invoices are issued monthly.
EOF
for d in acme/api acme/handbook globex/billing; do
  git -C "$WORK/sources/$d" init -q
  git -C "$WORK/sources/$d" add -A
  git -C "$WORK/sources/$d" -c user.email=e2e@example.com -c user.name=e2e commit -qm initial
done

# ── Dex: five people, one password. The hash is Dex's documented example for "password" ─
HASH='$2a$10$2b2cU8CPhOTaGrs1HRQuAueS7JTT5ZHsHSzYiFPm1leZck7Mc8T4W'
{
  cat <<EOF
issuer: http://dex:5556/dex
storage: {type: memory}
web: {http: 0.0.0.0:5556}
oauth2: {skipApprovalScreen: true}
staticClients:
  - id: ekos
    secret: ekos-e2e-secret
    name: EKOS
    redirectURIs: ["http://127.0.0.1:${EKOS_E2E_PORT}/api/auth/callback"]
enablePasswordDB: true
# One fixed person, kilgore@kilgore.trout, in the group "authors".
connectors:
  - {type: mockCallback, id: mock, name: Mock}
staticPasswords:
EOF
  n=0
  for who in owner alice bob carol dave; do
    n=$((n + 1))
    echo "  - {email: ${who}@example.com, hash: \"${HASH}\", username: ${who}, userID: 00000000-0000-0000-0000-00000000000${n}}"
  done
} > "$WORK/dex.yaml"

cat > "$WORK/override.yml" <<EOF
services:
  ekos:
    image: ${IMAGE}
    env_file: !reset []
    environment:
      EKOS_CONSOLE_SESSION_SECRET: "$(openssl rand -hex 32)"
      EKOS_CONSOLE_OIDC_ISSUER: http://dex:5556/dex
      EKOS_CONSOLE_OIDC_CLIENT_ID: ekos
      EKOS_CONSOLE_OIDC_CLIENT_SECRET: ekos-e2e-secret
      EKOS_CONSOLE_OIDC_REDIRECT_URI: http://127.0.0.1:${EKOS_E2E_PORT}/api/auth/callback
      EKOS_CONSOLE_OIDC_SCOPES: "openid email profile groups"
      EKOS_CONSOLE_OWNERS: owner@example.com
      EKOS_CONSOLE_PRODUCTION: "true"
      ACME_MODEL_KEY: acme-key-value
      GLOBEX_MODEL_KEY: globex-key-value
  dex:
    image: ghcr.io/dexidp/dex:v2.41.1
    command: ["dex", "serve", "/etc/dex/config.yaml"]
    volumes: ["${WORK}/dex.yaml:/etc/dex/config.yaml:ro"]
    ports: ["127.0.0.1:5556:5556"]
EOF

export EKOS_SOURCES="$WORK/sources" EKOS_PORT="$EKOS_E2E_PORT"
dc() { docker compose -p ekos-e2e -f "$REPO/deploy/docker-compose.yml" -f "$WORK/override.yml" "$@"; }
py() { (cd "$REPO/web/api" && PYTHONPATH="$HERE" uv run --quiet python "$HERE/$1"); }
wait_up() {
  for _ in $(seq 1 60); do
    curl -sf "http://127.0.0.1:${EKOS_E2E_PORT}/api/health" >/dev/null && return 0
    sleep 1
  done
  echo "the console did not come up" >&2
  dc logs ekos | tail -30 >&2
  return 1
}
cleanup() {
  if [ "${EKOS_E2E_KEEP:-}" = 1 ]; then
    echo "left running (project ekos-e2e, files in $WORK)"
  else
    dc down -v >/dev/null 2>&1 || true
    rm -r "$WORK"
  fi
}
trap cleanup EXIT

step() { printf '\n== %s\n' "$1"; }

dc down -v >/dev/null 2>&1 || true
dc up -d >/dev/null 2>&1
wait_up

step "1. sign-in, tenants, roles, a project through the API"
py s1_setup.py

step "2. a project of two sources from the command line, then a restart"
dc exec -T ekos ekos --tenant acme project add shop \
  --source api=/sources/acme/api --source handbook=/sources/acme/handbook | head -3
dc restart ekos >/dev/null 2>&1
wait_up
py s2_pipeline.py

step "3. a limited viewer, tokens, the gateway, the audit log"
py s3_scope_gateway.py
py s4_ids.py

step "4. a group from the provider's claim; a real MCP client; the pages in a browser"
py s6_groups.py
(cd "$REPO/web/api" && PYTHONPATH="$HERE" uv run --quiet --with mcp python "$HERE/s7_mcp_client.py")
(cd "$REPO/web/api" && PYTHONPATH="$HERE" uv run --quiet --with playwright python "$HERE/s8_browser.py")

step "5. what each project's server was given"
dc exec -T ekos sh -c '
  bad=0
  for p in /proc/[0-9]*; do
    c=$(tr "\0" " " < $p/cmdline 2>/dev/null) || continue
    case "$c" in *"mcp serve"*) ;; *) continue ;; esac
    t=$(echo "$c" | sed -n "s/.*--tenant \([a-z]*\).*/\1/p")
    [ -n "$t" ] || continue   # this shell: its own command line holds the words searched for
    e=$(tr "\0" "\n" < $p/environ | grep -E "MODEL_KEY|EKOS_CONSOLE" | sed "s/=.*//" | tr "\n" " ")
    echo "  $t: $e"
    case "$t" in
      acme)   [ "$e" = "ACME_MODEL_KEY " ] || bad=1 ;;
      globex) [ "$e" = "GLOBEX_MODEL_KEY " ] || bad=1 ;;
    esac
  done
  exit $bad' && echo "PASS  each server holds its own tenant's variable and none of the console's"

step "6. backup, a new volume, restore"
dc exec -T ekos ekos home backup | head -2
name=$(dc exec -T ekos sh -c 'ls /var/lib/ekos/backups | head -1' | tr -d '\r')
dc cp "ekos:/var/lib/ekos/backups/$name" "$WORK/backup.tar.zst" >/dev/null 2>&1
dc down -v >/dev/null 2>&1
dc up -d >/dev/null 2>&1
wait_up
dc cp "$WORK/backup.tar.zst" ekos:/tmp/b.tar.zst >/dev/null 2>&1
if dc exec -T ekos ekos home restore /tmp/b.tar.zst >/dev/null 2>&1; then
  echo "FAIL  restore without --force replaced the console's database"
  exit 1
fi
echo "PASS  restore without --force refuses to replace the new console's database"
dc exec -T ekos ekos home restore /tmp/b.tar.zst --force | head -4
dc restart ekos >/dev/null 2>&1
wait_up
py s5_restore.py

printf '\nAll stages passed.\n'
