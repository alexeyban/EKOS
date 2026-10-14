#!/usr/bin/env bash
# Builds an offline install package for Linux x86_64 from the committed tree:
#
#   scripts/package-release.sh [--no-musl] [--out DIR]
#
# → DIR/ekos-<version>-release.tar.gz (default DIR: dist/), holding the glibc and the static
# musl binaries packed the way release.yml packs them, one SHA256SUMS, install.sh and
# INSTALL.md. Installed with:
#
#   EKOS_VERSION=v<version> EKOS_BASE_URL="file://$PWD" sh install.sh
#
# The source is `git archive HEAD`, so uncommitted changes are never in a package. <version> is
# the tag when HEAD is a tagged release, otherwise <crate version>-dev.<short commit>.
# The glibc build runs on this machine; the musl build runs in the Rust image of the pinned
# toolchain (it needs musl-gcc, which most hosts lack). Needs Docker for that; --no-musl skips
# it. Build directories are kept in ${EKOS_PACKAGE_CACHE:-~/.cache/ekos-package} between runs.
set -euo pipefail

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
out="$repo/dist"
musl=1
while [ $# -gt 0 ]; do
  case "$1" in
    --no-musl) musl=0 ;;
    --out) out="$2"; shift ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
  shift
done

[ "$(uname -sm)" = "Linux x86_64" ] || { echo "builds Linux x86_64 packages on Linux x86_64 only" >&2; exit 2; }

cache="${EKOS_PACKAGE_CACHE:-${XDG_CACHE_HOME:-$HOME/.cache}/ekos-package}"
commit="$(git -C "$repo" rev-parse --short HEAD)"
cargo_version="$(sed -n 's/^version = "\(.*\)"/\1/p' "$repo/ekos/Cargo.toml" | head -1)"
if tag="$(git -C "$repo" describe --exact-match --tags HEAD 2>/dev/null)"; then
  version="${tag#v}"
else
  version="${cargo_version}-dev.${commit}"
fi
toolchain="$(sed -n 's/^channel = "\(.*\)"/\1/p' "$repo/rust-toolchain.toml")"
[ -z "$(git -C "$repo" status --porcelain)" ] \
  || echo "note: the working tree has uncommitted changes; the package is built from $commit without them"

src="$cache/src"
rm -rf "$src"; mkdir -p "$src"
git -C "$repo" archive HEAD | tar -x -C "$src"

targets=(x86_64-unknown-linux-gnu)
echo "== building x86_64-unknown-linux-gnu ($version)"
(cd "$src/ekos" && CARGO_TARGET_DIR="$cache/target" \
  cargo build --release --locked -p ekos --target x86_64-unknown-linux-gnu)

if [ "$musl" = 1 ]; then
  targets+=(x86_64-unknown-linux-musl)
  echo "== building x86_64-unknown-linux-musl in rust:${toolchain%.0}-slim-bookworm"
  mkdir -p "$cache/target-musl" "$cache/registry-musl"
  # Root inside the container (apt, rustup); its own registry volume, so nothing root-owned
  # lands in this user's ~/.cargo; the output is handed back to this user at the end.
  # `rustup target add` runs in the source directory: rust-toolchain.toml selects the toolchain
  # there, and a target added anywhere else goes to the image's default one (see release.yml).
  docker run --rm -v "$src:/src:ro" -v "$cache/target-musl:/target" \
    -v "$cache/registry-musl:/usr/local/cargo/registry" \
    "rust:${toolchain%.0}-slim-bookworm" sh -c "
      set -e
      apt-get update -qq >/dev/null && apt-get install -y -qq musl-tools >/dev/null 2>&1
      cd /src/ekos && rustup target add x86_64-unknown-linux-musl >/dev/null 2>&1
      CC_x86_64_unknown_linux_musl=musl-gcc CARGO_TARGET_DIR=/target \
        cargo build --release --locked -p ekos --target x86_64-unknown-linux-musl
      chown -R $(id -u):$(id -g) /target /usr/local/cargo/registry"
fi

name="ekos-${version}-release"
stage="$out/$name"
rm -rf "$stage"; mkdir -p "$stage"
for t in "${targets[@]}"; do
  bin="$cache/target/$t/release/ekos"
  [ "$t" = x86_64-unknown-linux-musl ] && bin="$cache/target-musl/$t/release/ekos"
  d="ekos-${version}-$t"
  mkdir -p "$stage/$d"
  cp "$bin" "$src/README.md" "$src/LICENSE" "$stage/$d/"
  tar -C "$stage" -czf "$stage/$d.tar.gz" "$d"
  rm -r "${stage:?}/$d"
done
(cd "$stage" && sha256sum -- *.tar.gz > SHA256SUMS)
cp "$src/install.sh" "$stage/"
sed -e "s/@VERSION@/$version/g" -e "s/@COMMIT@/$commit/g" -e "s/@CARGO_VERSION@/$cargo_version/g" \
  "$src/deploy/package/INSTALL.md" > "$stage/INSTALL.md"
[ "$musl" = 1 ] || echo "note: built with --no-musl; INSTALL.md still lists the musl archive"

# The package must install: run it into a throwaway home.
check="$(mktemp -d)"
trap 'rm -rf "$check"' EXIT
(cd "$stage" && env -i PATH="$PATH" HOME="$check" EKOS_VERSION="v$version" \
  EKOS_BASE_URL="file://$stage" sh install.sh >/dev/null)
"$check/.ekos/bin/ekos" --version >/dev/null
echo "== installs and runs: $("$check/.ekos/bin/ekos" --version)"

tar -C "$out" -czf "$out/$name.tar.gz" "$name"
echo "== $out/$name.tar.gz"
cat "$stage/SHA256SUMS"
