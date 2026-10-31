#!/bin/sh
# ekos-entrypoint — the image's entrypoint (RFC 0232).
#
# A cloud's secret store reaches a container two ways: as environment variables (Azure Container
# Apps, Cloud Run, ECS inject them) or as files (the Secrets Store CSI driver on AKS, GKE and EKS;
# the boot script on a VM). With EKOS_SECRETS_DIR set, every file there whose name is a valid
# variable name becomes that variable, unless the environment already sets it; then the command
# runs. Values are never printed. Without EKOS_SECRETS_DIR this only runs the command.
set -eu

if [ -n "${EKOS_SECRETS_DIR:-}" ] && [ -d "$EKOS_SECRETS_DIR" ]; then
    for file in "$EKOS_SECRETS_DIR"/*; do
        [ -f "$file" ] || continue
        name=$(basename "$file")
        case "$name" in
            ''|[0-9]*|*[!A-Za-z0-9_]*) continue ;;
        esac
        # Already in the environment: the platform's value wins.
        if eval "[ -n \"\${$name+x}\" ]"; then
            continue
        fi
        value=$(cat "$file")
        export "$name=$value"
    done
fi

exec "$@"
