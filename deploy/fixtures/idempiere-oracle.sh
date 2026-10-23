#!/usr/bin/env bash
# RFC 0211 Part 0 — import the iDempiere 12 seed into the Oracle XE testbed, then create the
# read-only user EKOS connects as.
#
#   docker compose -f docker-compose.oracle.yml up -d
#   deploy/fixtures/idempiere-oracle.sh
#
# The server build is downloaded once into $EKOS_TESTBED_CACHE (default ~/.cache/ekos-testbed) and
# checked against a pinned SHA-256 before anything is unpacked. The import itself is iDempiere's own
# utils/oracle/ImportIdempiere.sh, run with ORACLE_DOCKER_CONTAINER so sqlplus and impdp execute
# inside the container. Re-running drops and re-imports the ADEMPIERE schema.
set -euo pipefail

CACHE="${EKOS_TESTBED_CACHE:-$HOME/.cache/ekos-testbed}"
CONTAINER="${ORACLE_CONTAINER:-ekos-oracle-xe-1}"
SYSTEM_PASSWORD="${ORACLE_SYSTEM_PASSWORD:-ekos-local-only}"
SCHEMA_USER="adempiere"
SCHEMA_PASSWORD="${IDEMPIERE_DB_PASSWORD:-adempiere}"
RO_USER="ekos_ro"
RO_PASSWORD="${EKOS_RO_PASSWORD:-ekos-ro-local-only}"
SERVICE="XEPDB1"

ZIP="idempiereServer12.gtk.linux.x86_64.zip"
URL="https://sourceforge.net/projects/idempiere/files/v12/server/$ZIP/download"
# The MD5 SourceForge publishes beside the file is dac51c269dfa18e19f34ee42d077d263.
SHA256="e58d527ff432a23352e88a92e3b71e0045bbe8dc3b1dee38b4c6079229aee68f"

mkdir -p "$CACHE"
if [[ ! -f "$CACHE/$ZIP" ]]; then
  echo "downloading $ZIP"
  curl -fL --retry 3 -o "$CACHE/$ZIP.part" "$URL"
  mv "$CACHE/$ZIP.part" "$CACHE/$ZIP"
fi
echo "$SHA256  $CACHE/$ZIP" | sha256sum -c -

SERVER="$CACHE/idempiere-12"
if [[ ! -d "$SERVER/idempiere-server" ]]; then
  rm -rf "$SERVER" && mkdir -p "$SERVER"
  unzip -q "$CACHE/$ZIP" -d "$SERVER.tmp"
  mv "$SERVER.tmp"/*/idempiere-server "$SERVER/" && rm -rf "$SERVER.tmp"
fi
HOME_DIR="$SERVER/idempiere-server"
if [[ ! -f "$HOME_DIR/data/seed/Adempiere.dmp" ]]; then
  (cd "$HOME_DIR/data/seed" && unzip -q -o Adempiere.jar)
fi

if [[ "$(docker inspect -f '{{.State.Health.Status}}' "$CONTAINER")" != healthy ]]; then
  echo "container $CONTAINER is not healthy yet" >&2
  exit 1
fi

# The four steps of the release's own utils/oracle/ImportIdempiere.sh, run inside the container
# with that release's SQL files. (Its ORACLE_DOCKER_CONTAINER support arrived in release 13, after
# this seed, and the v12 script expects sqlplus and impdp on the host.)
IN="/opt/oracle/idempiere"
DB="localhost:1521/$SERVICE"
docker exec "$CONTAINER" mkdir -p "$IN/seed" "$IN/script"
docker cp "$HOME_DIR/data/seed/Adempiere.dmp" "$CONTAINER:$IN/seed/"
for f in CreateUser.sql CreateDataPumpDir.sql AfterImport.sql; do
  docker cp "$HOME_DIR/utils/oracle/$f" "$CONTAINER:$IN/script/"
done
docker exec -u 0 "$CONTAINER" chown -R oracle:dba "$IN"

echo "re-creating user $SCHEMA_USER"
docker exec "$CONTAINER" sqlplus -S -L "system/$SYSTEM_PASSWORD@$DB" \
  @"$IN/script/CreateUser.sql" "$SCHEMA_USER" "$SCHEMA_PASSWORD"
docker exec "$CONTAINER" sqlplus -S -L "system/$SYSTEM_PASSWORD@$DB" \
  @"$IN/script/CreateDataPumpDir.sql" "$IN/seed"

echo "importing Adempiere.dmp"
# impdp exits non-zero when it reports warnings (an object that already existed, a grant to a role
# the seed's source database had); the import log is kept and AfterImport below is the check.
docker exec "$CONTAINER" impdp "$SCHEMA_USER/$SCHEMA_PASSWORD@$DB" \
  DIRECTORY=ADEMPIERE_DATA_PUMP_DIR DUMPFILE=Adempiere.dmp LOGFILE=import.log \
  REMAP_SCHEMA=reference:"$SCHEMA_USER" || echo "impdp exited $? (see $IN/seed/import.log)"

echo "running AfterImport.sql"
docker exec "$CONTAINER" sqlplus -S -L "$SCHEMA_USER/$SCHEMA_PASSWORD@$DB" @"$IN/script/AfterImport.sql"

echo "creating $RO_USER"
docker exec -i "$CONTAINER" sqlplus -S -L "system/$SYSTEM_PASSWORD@localhost:1521/$SERVICE" <<SQL
WHENEVER SQLERROR EXIT FAILURE
SET SERVEROUTPUT ON FEEDBACK OFF
BEGIN
  EXECUTE IMMEDIATE 'DROP USER $RO_USER CASCADE';
EXCEPTION WHEN OTHERS THEN
  IF SQLCODE != -1918 THEN RAISE; END IF;
END;
/
CREATE USER $RO_USER IDENTIFIED BY "$RO_PASSWORD" ACCOUNT UNLOCK;
GRANT CREATE SESSION TO $RO_USER;
GRANT SELECT_CATALOG_ROLE TO $RO_USER;
DECLARE
  n PLS_INTEGER := 0;
BEGIN
  FOR o IN (SELECT object_name FROM all_objects
             WHERE owner = UPPER('$SCHEMA_USER') AND object_type IN ('TABLE', 'VIEW')
               AND object_name NOT LIKE 'BIN\$%') LOOP
    EXECUTE IMMEDIATE 'GRANT SELECT ON ' || UPPER('$SCHEMA_USER') || '."' || o.object_name
      || '" TO $RO_USER';
    n := n + 1;
  END LOOP;
  DBMS_OUTPUT.PUT_LINE('granted SELECT on ' || n || ' tables and views');
END;
/
EXIT
SQL
echo "done: connect as $RO_USER@localhost:51521/$SERVICE"
