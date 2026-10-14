#!/bin/sh
# EKOS installer (RFC 0153).
#
#   curl -fsSL https://raw.githubusercontent.com/alexeyban/EKOS/main/install.sh | sh
#
# Piping a script into a shell is a real supply-chain decision, not a neutral convenience. This
# script is deliberately short enough to read end to end first:
#
#   curl -fsSL https://raw.githubusercontent.com/alexeyban/EKOS/main/install.sh -o install.sh
#   less install.sh && sh install.sh
#
# It downloads one release asset, verifies it against the release's SHA256SUMS **before**
# unpacking, and installs a single binary into the EKOS home: $EKOS_HOME/bin (default
# ~/.ekos/bin). The home is also where projects keep their config and compiled data
# ($EKOS_HOME/data, RFC 0178). It never uses sudo and never writes outside the home, or outside
# $EKOS_INSTALL_DIR when that is set.
#
# Environment:
#   EKOS_VERSION       tag to install (default: the latest release)
#   EKOS_HOME          the EKOS home (default: $HOME/.ekos)
#   EKOS_INSTALL_DIR   install the binary here instead of $EKOS_HOME/bin
#   EKOS_BASE_URL      download the release files from here instead of GitHub (a mirror, or a
#                      local directory as file:///...). The checksum check is the same.
#   EKOS_CLI_ONLY=1    install the command only, even when the release has a full package
#   EKOS_NO_SETUP=1    install the full package without starting the setup wizard
#
# When the release has a full package for this machine (RFC 0185: the command, the web console
# and the Python that runs it), that is installed into the home and the setup wizard opens in
# the browser. Otherwise, and with EKOS_CLI_ONLY=1, the single binary is installed as before.

set -eu

REPO="alexeyban/EKOS"
EKOS_HOME_DIR="${EKOS_HOME:-$HOME/.ekos}"

# Where the binary goes (RFC 0179). An install made before the home existed is upgraded where it
# is: a second copy in ~/.ekos/bin would leave the old one first on PATH.
if [ -n "${EKOS_INSTALL_DIR:-}" ]; then
    INSTALL_DIR="$EKOS_INSTALL_DIR"
elif [ -z "${EKOS_HOME:-}" ] && [ -f "$HOME/.local/bin/ekos" ]; then
    INSTALL_DIR="$HOME/.local/bin"
    printf 'Found an earlier install in %s; upgrading it there.\n' "$INSTALL_DIR"
    printf 'To install into the EKOS home instead: EKOS_HOME=%s sh install.sh\n\n' "$EKOS_HOME_DIR"
else
    INSTALL_DIR="$EKOS_HOME_DIR/bin"
fi

# POSIX sh has no `local`, so each function names its arguments with a function-prefixed
# variable instead of reading "$1"/"$2" throughout.
die() {
    die_msg="$1"
    printf 'ekos install: %s\n' "$die_msg" >&2
    exit 1
}

need() {
    need_cmd="$1"
    command -v "$need_cmd" >/dev/null 2>&1 || die "this installer needs '$need_cmd' on PATH"
}

need uname
need mkdir
need tar

if command -v curl >/dev/null 2>&1; then
    fetch() { fetch_url="$1"; fetch_dest="$2"; curl -fsSL "$fetch_url" -o "$fetch_dest"; }
    fetch_stdout() { fetch_url="$1"; curl -fsSL "$fetch_url"; }
elif command -v wget >/dev/null 2>&1; then
    fetch() { fetch_url="$1"; fetch_dest="$2"; wget -qO "$fetch_dest" "$fetch_url"; }
    fetch_stdout() { fetch_url="$1"; wget -qO- "$fetch_url"; }
else
    die "this installer needs curl or wget on PATH"
fi

# ── Target ────────────────────────────────────────────────────────────────────
os="$(uname -s)"
arch="$(uname -m)"

case "$os" in
    Linux)  os_part="unknown-linux-gnu" ;;
    Darwin) os_part="apple-darwin" ;;
    *)
        die "unsupported operating system '$os'. Prebuilt binaries cover Linux and macOS; on
Windows download the .zip from https://github.com/$REPO/releases, and on anything else build
from source: https://github.com/$REPO#installation"
        ;;
esac

case "$arch" in
    x86_64 | amd64)  arch_part="x86_64" ;;
    aarch64 | arm64) arch_part="aarch64" ;;
    *)
        die "unsupported architecture '$arch'. Build from source instead:
https://github.com/$REPO#installation"
        ;;
esac

target="${arch_part}-${os_part}"

# A glibc binary only runs on glibc >= the one it was built against, and no amount of care at
# build time makes that check unnecessary on the user's machine. On x86_64 Linux there is a
# fully static musl build to fall back to when the gnu one will not start. Everywhere else there
# is no second artifact, and the script says so rather than failing obscurely.
fallback=""
if [ "$os" = "Linux" ] && [ "$arch_part" = "x86_64" ]; then
    fallback="x86_64-unknown-linux-musl"
fi

# ── Version ───────────────────────────────────────────────────────────────────
version="${EKOS_VERSION:-}"
if [ -z "$version" ]; then
    printf 'Resolving the latest release...\n'
    version="$(fetch_stdout "https://api.github.com/repos/$REPO/releases/latest" \
        | sed -n 's/.*"tag_name": *"\([^"]*\)".*/\1/p' | head -n 1)"
    [ -n "$version" ] || die "could not resolve the latest release. Set EKOS_VERSION=vX.Y.Z, or
check https://github.com/$REPO/releases"
fi

bare="${version#v}"
base="${EKOS_BASE_URL:-https://github.com/$REPO/releases/download/$version}"

# ── Download, verify, unpack ──────────────────────────────────────────────────
tmp="$(mktemp -d)"
# `trap` on EXIT rather than a cleanup call at the end: a failed download must not leave the
# archive lying in /tmp either.
trap 'rm -rf "$tmp"' EXIT INT TERM

# Fetches one target's archive, verifies it against SHA256SUMS, unpacks it, and sets $binary.
fetch_target() {
    _t="$1"
    _asset="ekos-${bare}-${_t}.tar.gz"

    printf 'Downloading %s (%s)...\n' "$_asset" "$version"
    fetch "$base/$_asset" "$tmp/$_asset" \
        || die "could not download $base/$_asset — check that $version has a build for $_t at
https://github.com/$REPO/releases/tag/$version"

    # Not optional: an unverified archive is unpacked code from the network.
    if [ ! -f "$tmp/SHA256SUMS" ]; then
        fetch "$base/SHA256SUMS" "$tmp/SHA256SUMS" \
            || die "could not download the checksum file for $version — refusing to install unverified"
    fi

    # The asset name goes into a regex, and it is full of `.` characters that would otherwise
    # match any byte — so a line for a *different* file could satisfy the lookup. Escape it first.
    _asset_re="$(printf '%s' "$_asset" | sed 's/[].[^$*\\/]/\\&/g')"
    _expected="$(sed -n "s/^\([0-9a-f]\{64\}\)[ *][ *]*${_asset_re}\$/\1/p" "$tmp/SHA256SUMS" | head -n 1)"
    [ -n "$_expected" ] || die "$_asset is not listed in SHA256SUMS — refusing to install unverified"

    if command -v sha256sum >/dev/null 2>&1; then
        _actual="$(sha256sum "$tmp/$_asset" | cut -d' ' -f1)"
    elif command -v shasum >/dev/null 2>&1; then
        _actual="$(shasum -a 256 "$tmp/$_asset" | cut -d' ' -f1)"
    else
        die "this installer needs sha256sum or shasum to verify the download"
    fi

    [ "$_actual" = "$_expected" ] || die "checksum mismatch for $_asset
  expected $_expected
  actual   $_actual
Refusing to install. Report this at https://github.com/$REPO/issues"

    printf 'Checksum verified.\n'

    tar -C "$tmp" -xzf "$tmp/$_asset"
    binary="$tmp/ekos-${bare}-${_t}/ekos"
    [ -f "$binary" ] || die "the archive did not contain the expected binary at ekos/"
    chmod 755 "$binary"
}

# Put a new binary in place by renaming a copy over it: a running program (an agent's MCP server,
# a pipeline run) cannot be overwritten ("Text file busy"), but it can be replaced by a rename,
# and keeps running from the old file.
place_binary() {
    cp "$1" "$2.new.$$"
    chmod 755 "$2.new.$$"
    mv -f "$2.new.$$" "$2"
}

# ── The full package (RFC 0185) ───────────────────────────────────────────────
full="ekos-${bare}-${target}-full"
if [ "${EKOS_CLI_ONLY:-}" != 1 ] && [ -z "${EKOS_INSTALL_DIR:-}" ] \
    && fetch "$base/SHA256SUMS" "$tmp/SHA256SUMS" 2>/dev/null \
    && grep -q "[ *]${full}\.tar\.gz\$" "$tmp/SHA256SUMS"; then
    printf 'Downloading %s.tar.gz (%s)...\n' "$full" "$version"
    fetch "$base/$full.tar.gz" "$tmp/$full.tar.gz" || die "could not download $base/$full.tar.gz"
    full_re="$(printf '%s' "$full.tar.gz" | sed 's/[].[^$*\\/]/\\&/g')"
    full_expected="$(sed -n "s/^\([0-9a-f]\{64\}\)[ *][ *]*${full_re}\$/\1/p" "$tmp/SHA256SUMS" | head -n 1)"
    if command -v sha256sum >/dev/null 2>&1; then
        full_actual="$(sha256sum "$tmp/$full.tar.gz" | cut -d' ' -f1)"
    else
        full_actual="$(shasum -a 256 "$tmp/$full.tar.gz" | cut -d' ' -f1)"
    fi
    [ "$full_actual" = "$full_expected" ] || die "checksum mismatch for $full.tar.gz — refusing to install"
    printf 'Checksum verified.\n'
    tar -C "$tmp" -xzf "$tmp/$full.tar.gz"
    if "$tmp/$full/bin/ekos" --version >/dev/null 2>&1; then
        mkdir -p "$EKOS_HOME_DIR/data"
        # Replace the program, never the data: only bin/ and console/ are touched.
        rm -rf "$EKOS_HOME_DIR/console"
        mkdir -p "$EKOS_HOME_DIR/bin"
        cp -R "$tmp/$full/console" "$EKOS_HOME_DIR/console"
        place_binary "$tmp/$full/bin/ekos" "$EKOS_HOME_DIR/bin/ekos"
        printf '\nInstalled EKOS %s with its web console to %s\n' "$version" "$EKOS_HOME_DIR"
        if [ "${EKOS_NO_SETUP:-}" = 1 ]; then
            printf 'Next: %s/bin/ekos setup\n' "$EKOS_HOME_DIR"
        else
            printf 'Opening the setup page in your browser...\n'
            "$EKOS_HOME_DIR/bin/ekos" --home "$EKOS_HOME_DIR" setup
        fi
        exit 0
    fi
    printf '\nThe full package does not run on this system; installing the command only.\n\n'
fi

fetch_target "$target"

# Verify it actually runs here before installing it. Checking the binary empirically beats
# parsing `ldd --version`: it covers every reason a build might not start on this machine, not
# just the glibc one we happen to know about.
if ! "$binary" --version >/dev/null 2>&1; then
    if [ -n "$fallback" ]; then
        printf '\nThe %s build does not run on this system (most likely an older glibc).\n' "$target"
        printf 'Falling back to the fully static %s build.\n\n' "$fallback"
        fetch_target "$fallback"
        "$binary" --version >/dev/null 2>&1 \
            || die "neither the $target nor the $fallback build runs on this system. Please open
an issue with your OS and \`uname -m\`: https://github.com/$REPO/issues"
    else
        die "the $target build does not run on this system, and there is no fallback build for
this platform. Please open an issue with your OS and \`uname -m\`, or build from source:
https://github.com/$REPO#installation"
    fi
fi

# ── Install ──────────────────────────────────────────────────────────────────
mkdir -p "$INSTALL_DIR"
# `cp` then `chmod`, not `install`: BusyBox and some minimal images lack `install(1)`.
place_binary "$binary" "$INSTALL_DIR/ekos"

# The home's data directory, so `ekos project add` has somewhere to put a project and the
# layout is visible before the first one exists.
mkdir -p "$EKOS_HOME_DIR/data"

printf '\nInstalled ekos %s to %s/ekos\n' "$version" "$INSTALL_DIR"
printf 'EKOS home: %s\n' "$EKOS_HOME_DIR"

case ":${PATH}:" in
    *":$INSTALL_DIR:"*) ;;
    *)
        printf '\n%s is not on your PATH. Add it:\n' "$INSTALL_DIR"
        printf '    export PATH="%s:$PATH"\n' "$INSTALL_DIR"
        ;;
esac

printf '\nNext, either keep a project in the EKOS home (nothing is written into the sources):\n'
printf '    ekos project add <name> --source <dir> --detect\n'
printf '    ekos --project <name> build\n'
printf 'or work inside one repository:\n'
printf '    ekos init --detect     # write an ekos.toml that matches this repository\n'
printf '    ekos doctor            # check the environment\n'
