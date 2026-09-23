#!/bin/sh
# Publish the EKOS workspace to crates.io, in dependency order (RFC 0153 §6).
#
#   scripts/publish-crates.sh --dry-run     # show the plan, publish nothing
#   scripts/publish-crates.sh               # publish for real, one crate at a time
#
# `cargo install ekos` needs every internal crate on the registry, because path dependencies are
# stripped at publish time and only the version remains. The order below is a topological sort of
# the real dependency graph: each crate follows all of its dependencies.
#
# Publishing is IRREVERSIBLE. A published version can be yanked, which stops new dependents
# resolving it, but it can never be deleted or replaced. Read the plan before running for real.
#
# The script is resumable: it skips any crate whose version is already on crates.io, so a run that
# fails halfway can simply be run again after the cause is fixed.
#
# Requires a crates.io token: `cargo login`, or CARGO_REGISTRY_TOKEN in the environment.

set -eu

cd "$(dirname "$0")/.."
MANIFEST="ekos/Cargo.toml"

DRY_RUN=0
[ "${1:-}" = "--dry-run" ] && DRY_RUN=1

VERSION="$(sed -n 's/^version = "\(.*\)"$/\1/p' "$MANIFEST" | head -n 1)"
[ -n "$VERSION" ] || { echo "could not read the workspace version from $MANIFEST" >&2; exit 1; }

# Slightly over the ~10 minute refill, so a retry does not immediately re-trip the limit.
RATE_LIMIT_WAIT="${EKOS_PUBLISH_RETRY_SECONDS:-660}"
MAX_RATE_LIMIT_RETRIES="${EKOS_PUBLISH_MAX_RETRIES:-8}"
LOG="$(mktemp)"
RC="$(mktemp)"
trap 'rm -f "$LOG" "$RC"' EXIT INT TERM

# Dependency order. Regenerate with the metadata walk documented in devlog_202 if the graph
# changes; `cargo package` fails loudly if a crate is published before one of its dependencies,
# so a stale order here cannot silently produce a broken registry state.
CRATES="ekos-common
ekos-kir
ekos-artifact
ekos-segment-backend
ekos-ledger
ekos-sql-dialect-sdk
ekos-plugin-sql-dialect-clickhouse
ekos-compiler-core
ekos-plugin-sql-dialect-databricks
ekos-plugin-sql-dialect-mssql
ekos-plugin-sql-dialect-mysql
ekos-plugin-sql-dialect-postgres
ekos-plugin-sql-dialect-snowflake
ekos-identity
ekos-semantic
ekos-recovery
ekos-runtime
ekos-clickhouse-query
ekos-cluster
ekos-dbt-gen
ekos-distributed
ekos-docs-gen
ekos-ekl
ekos-evals
ekos-marketing
ekos-observation-sdk
ekos-plugin-clickhouse
ekos-plugin-confluence
ekos-plugin-crypto
ekos-plugin-elixir
ekos-plugin-file
ekos-plugin-git
ekos-plugin-github
ekos-plugin-governance
ekos-plugin-javascript
ekos-plugin-localdocs
ekos-plugin-pentaho
ekos-plugin-perl
ekos-plugin-python
ekos-plugin-rust
ekos-plugin-treasury
ekos-session
ekos-simulation
ekos"

total=$(printf '%s\n' "$CRATES" | wc -l | tr -d ' ')

printf 'EKOS crates.io publish — version %s, %s crates\n\n' "$VERSION" "$total"
if [ "$DRY_RUN" -eq 1 ]; then
    printf 'DRY RUN — nothing will be published.\n\n'
else
    printf 'This publishes %s crates to crates.io. Publishing cannot be undone.\n\n' "$total"
    printf 'crates.io rate-limits new crates to a burst of ~5, then ~1 every 10 minutes, so this\n'
    printf 'run will take several hours. It waits out each rate limit and retries, so it can be\n'
    printf 'left alone; interrupting it is safe, because re-running skips whatever succeeded.\n\n'
    printf 'Press Enter to continue, or Ctrl-C to stop.\n'
    read -r _
fi

# Already on crates.io at this exact version? Then skip it — that is what makes this resumable.
# Verified against the real API: /api/v1/crates/serde/1.0.200 -> 200, .../serde/99.99.99 -> 404.
already_published() {
    _url="https://crates.io/api/v1/crates/$1/$VERSION"
    _code=$(curl -s -o /dev/null -w '%{http_code}' "$_url" \
        -H 'User-Agent: ekos-publish (https://github.com/alexeyban/EKOS)' --max-time 20 || echo 000)
    [ "$_code" = "200" ]
}

i=0
for crate in $CRATES; do
    i=$((i + 1))
    printf '[%2d/%s] %s ' "$i" "$total" "$crate"

    if already_published "$crate"; then
        printf '— already on crates.io at %s, skipping\n' "$VERSION"
        continue
    fi

    if [ "$DRY_RUN" -eq 1 ]; then
        printf '— would publish\n'
        continue
    fi

    printf '— publishing...\n'

    # crates.io rate-limits BRAND NEW crates hard: a burst of about 5, then roughly one every ten
    # minutes. Publishing 44 new crates therefore takes hours, and a run that aborted on the first
    # 429 would need babysitting for all of them. So a rate-limit response is not a failure here —
    # it is waited out and retried. Every other failure still stops the run immediately.
    #
    # `cargo publish` itself blocks until the new version is visible in the index, so the next
    # crate can resolve it. No manual sleep is needed for propagation, only for the rate limit.
    attempt=1
    while :; do
        # The exit status of a pipeline is the LAST command's, so `cargo publish | tee` would
        # report tee's success and hide a real failure. POSIX sh has no PIPESTATUS, so cargo's
        # status is stashed in a file inside the pipeline, which keeps output streaming live
        # through a run that takes hours.
        { cargo publish --manifest-path "$MANIFEST" -p "$crate" --locked; echo $? > "$RC"; } 2>&1 \
            | tee "$LOG"
        if [ "$(cat "$RC")" = "0" ]; then
            break
        fi
        if grep -qiE '429|too many requests|too many new crates|rate limit' "$LOG"; then
            if [ "$attempt" -ge "$MAX_RATE_LIMIT_RETRIES" ]; then
                printf '\nStill rate-limited on %s after %d attempts. Run the script again later;\n' \
                    "$crate" "$attempt" >&2
                printf 'everything already published is skipped.\n' >&2
                exit 1
            fi
            printf '        rate-limited by crates.io — waiting %ss before retry %d/%s\n' \
                "$RATE_LIMIT_WAIT" "$((attempt + 1))" "$MAX_RATE_LIMIT_RETRIES"
            sleep "$RATE_LIMIT_WAIT"
            attempt=$((attempt + 1))
            continue
        fi
        printf '\nFAILED on %s (%d of %s) — not a rate limit.\n' "$crate" "$i" "$total" >&2
        printf 'Fix the cause and run this script again; everything already published is skipped.\n' >&2
        exit 1
    done
done

if [ "$DRY_RUN" -eq 1 ]; then
    printf '\nDry run complete — nothing was published.\n'
else
    printf '\nDone. %s is now installable with `cargo install ekos`.\n' "$VERSION"
fi
