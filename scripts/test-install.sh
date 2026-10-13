#!/bin/sh
# Tests install.sh against a fake release in a local directory (RFC 0179 §1). No network.
#
#   scripts/test-install.sh
#
# The "binary" is a shell script that answers `--version`, packed and checksummed the way
# release.yml does it, and fetched through EKOS_BASE_URL=file://...

set -eu

repo="$(cd "$(dirname "$0")/.." && pwd)"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT INT TERM

case "$(uname -s)" in
    Linux)  os_part="unknown-linux-gnu" ;;
    Darwin) os_part="apple-darwin" ;;
    *) echo "unsupported OS for this test" >&2; exit 1 ;;
esac
case "$(uname -m)" in
    x86_64 | amd64)  arch_part="x86_64" ;;
    aarch64 | arm64) arch_part="aarch64" ;;
    *) echo "unsupported architecture for this test" >&2; exit 1 ;;
esac

version="v9.9.9"
stage="ekos-9.9.9-${arch_part}-${os_part}"
release="$work/release"
mkdir -p "$release/$stage"
printf '#!/bin/sh\necho "ekos 9.9.9"\n' > "$release/$stage/ekos"
chmod 755 "$release/$stage/ekos"
tar -C "$release" -czf "$release/$stage.tar.gz" "$stage"
rm -rf "${release:?}/$stage"
if command -v sha256sum >/dev/null 2>&1; then
    (cd "$release" && sha256sum "$stage.tar.gz" > SHA256SUMS)
else
    (cd "$release" && shasum -a 256 "$stage.tar.gz" > SHA256SUMS)
fi

failures=0
fail() {
    fail_msg="$1"
    printf 'FAIL: %s\n' "$fail_msg" >&2
    failures=$((failures + 1))
}

# Runs the installer with a clean HOME. Arguments are extra `NAME=value` settings.
run_install() {
    run_home="$1"
    shift
    env -i PATH="$PATH" HOME="$run_home" EKOS_VERSION="$version" \
        EKOS_BASE_URL="file://$release" "$@" sh "$repo/install.sh" > "$work/out.log" 2>&1
}

# 1. Nothing set: the binary goes to ~/.ekos/bin and the data directory exists.
home1="$work/h1"; mkdir -p "$home1"
run_install "$home1" || fail "default install exited non-zero: $(cat "$work/out.log")"
[ -x "$home1/.ekos/bin/ekos" ] || fail "default install did not put the binary in ~/.ekos/bin"
[ -d "$home1/.ekos/data" ] || fail "default install did not create ~/.ekos/data"
[ ! -e "$home1/.local/bin/ekos" ] || fail "default install wrote to ~/.local/bin"
grep -q 'export PATH=' "$work/out.log" || fail "no PATH hint for a directory that is not on PATH"

# 2. EKOS_HOME set: its bin and data.
home2="$work/h2"; mkdir -p "$home2"
run_install "$home2" EKOS_HOME="$work/custom-home" || fail "EKOS_HOME install exited non-zero"
[ -x "$work/custom-home/bin/ekos" ] || fail "EKOS_HOME install did not use \$EKOS_HOME/bin"
[ -d "$work/custom-home/data" ] || fail "EKOS_HOME install did not create \$EKOS_HOME/data"
[ ! -e "$home2/.ekos" ] || fail "EKOS_HOME install also wrote to ~/.ekos"

# 3. EKOS_INSTALL_DIR set: it wins, as before.
home3="$work/h3"; mkdir -p "$home3"
run_install "$home3" EKOS_INSTALL_DIR="$work/custom-bin" || fail "EKOS_INSTALL_DIR install exited non-zero"
[ -x "$work/custom-bin/ekos" ] || fail "EKOS_INSTALL_DIR was not used"
[ ! -e "$home3/.ekos/bin/ekos" ] || fail "EKOS_INSTALL_DIR install also wrote to ~/.ekos/bin"

# 4. An earlier install in ~/.local/bin is upgraded there.
home4="$work/h4"; mkdir -p "$home4/.local/bin"
printf '#!/bin/sh\necho "ekos 0.0.1"\n' > "$home4/.local/bin/ekos"
chmod 755 "$home4/.local/bin/ekos"
run_install "$home4" || fail "upgrade install exited non-zero"
[ "$("$home4/.local/bin/ekos" --version)" = "ekos 9.9.9" ] || fail "the earlier install was not upgraded in place"
[ ! -e "$home4/.ekos/bin/ekos" ] || fail "upgrade install left a second copy in ~/.ekos/bin"

# 5. With EKOS_HOME set, an earlier install does not redirect it.
run_install "$home4" EKOS_HOME="$home4/.ekos" || fail "explicit-home install exited non-zero"
[ -x "$home4/.ekos/bin/ekos" ] || fail "explicit EKOS_HOME was overridden by an earlier install"

# 6. A wrong checksum refuses, and installs nothing.
bad="$work/bad"; mkdir -p "$bad"
cp "$release/$stage.tar.gz" "$bad/"
printf '%064d  %s\n' 0 "$stage.tar.gz" > "$bad/SHA256SUMS"
home6="$work/h6"; mkdir -p "$home6"
if env -i PATH="$PATH" HOME="$home6" EKOS_VERSION="$version" EKOS_BASE_URL="file://$bad" \
    sh "$repo/install.sh" > "$work/out.log" 2>&1; then
    fail "a wrong checksum was accepted"
fi
grep -q 'checksum mismatch' "$work/out.log" || fail "a wrong checksum did not say so"
[ ! -e "$home6/.ekos/bin/ekos" ] || fail "a wrong checksum still installed the binary"

# 7. An archive missing from SHA256SUMS refuses.
: > "$bad/SHA256SUMS"
if env -i PATH="$PATH" HOME="$home6" EKOS_VERSION="$version" EKOS_BASE_URL="file://$bad" \
    sh "$repo/install.sh" > "$work/out.log" 2>&1; then
    fail "an unlisted archive was accepted"
fi

if [ "$failures" -ne 0 ]; then
    printf '%s check(s) failed\n' "$failures" >&2
    exit 1
fi
printf 'install.sh: all checks passed\n'
