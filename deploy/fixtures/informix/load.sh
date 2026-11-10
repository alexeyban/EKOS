#!/usr/bin/env bash
# RFC 0252 §3.5 — load the Informix fixture into docker-compose.informix.yml's server: the
# read-only OS user `ekos_ro` (Informix signs users in through the operating system), then
# shop.sql as the DBA `informix`. Safe to run again: the user is kept, the database recreated.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
compose=(docker compose -f "$here/../../../docker-compose.informix.yml")
password="${EKOS_INFORMIX_PW:-Ekos-ro-local-1}"

"${compose[@]}" exec -T -u root informix \
  sh -c 'id ekos_ro >/dev/null 2>&1 || useradd -M -s /sbin/nologin ekos_ro'
printf 'ekos_ro:%s\n' "$password" | "${compose[@]}" exec -T -u root informix chpasswd
"${compose[@]}" exec -T -u informix informix \
  bash -c '. /usr/local/bin/informix_inf.env && DELIMIDENT=y DBDATE=Y4MD- dbaccess - -' \
  < "$here/shop.sql"
echo "fixture loaded: database shop, owners sales and hr, read-only user ekos_ro"
