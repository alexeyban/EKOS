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
# unpacking, and installs a single binary into $EKOS_INSTALL_DIR (default ~/.local/bin).
# It never uses sudo and never writes outside that directory.
#
# Environment:
#   EKOS_VERSION       tag to install (default: the latest release)
#   EKOS_INSTALL_DIR   install destination (default: $HOME/.local/bin)

set -eu

REPO="alexeyban/EKOS"
INSTALL_DIR="${EKOS_INSTALL_DIR:-$HOME/.local/bin}"

die() {
    printf 'ekos install: %s\n' "$1" >&2
    exit 1
}

need() {
    command -v "$1" >/dev/null 2>&1 || die "this installer needs '$1' on PATH"
}

need uname
need mkdir
need tar

if command -v curl >/dev/null 2>&1; then
    fetch() { curl -fsSL "$1" -o "$2"; }
    fetch_stdout() { curl -fsSL "$1"; }
elif command -v wget >/dev/null 2>&1; then
    fetch() { wget -qO "$2" "$1"; }
    fetch_stdout() { wget -qO- "$1"; }
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
asset="ekos-${bare}-${target}.tar.gz"
base="https://github.com/$REPO/releases/download/$version"

# ── Download, verify, unpack ──────────────────────────────────────────────────
tmp="$(mktemp -d)"
# `trap` on EXIT rather than a cleanup call at the end: a failed download must not leave the
# archive lying in /tmp either.
trap 'rm -rf "$tmp"' EXIT INT TERM

base="https://github.com/$REPO/releases/download/$version"

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
cp "$binary" "$INSTALL_DIR/ekos"
chmod 755 "$INSTALL_DIR/ekos"

printf '\nInstalled ekos %s to %s/ekos\n' "$version" "$INSTALL_DIR"

case ":${PATH}:" in
    *":$INSTALL_DIR:"*) ;;
    *)
        printf '\n%s is not on your PATH. Add it:\n' "$INSTALL_DIR"
        printf '    export PATH="%s:$PATH"\n' "$INSTALL_DIR"
        ;;
esac

printf '\nNext:\n'
printf '    ekos init --detect     # write an ekos.toml that matches this repository\n'
printf '    ekos doctor            # check the environment\n'
