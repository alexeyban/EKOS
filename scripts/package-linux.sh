#!/usr/bin/env bash
# Builds the Linux installers — a .deb and an .rpm — from one full package (scripts/package-full.sh).
#
#   scripts/package-linux.sh <full package .tar.gz> <out dir> <version> [deb|rpm|both]
#
# Both install the same tree:
#   /opt/ekos/{bin,console,LICENSE,INSTALL.md,installer.toml,share/ekos.png}
#   /usr/bin/ekos                        → /opt/ekos/bin/ekos
#   /usr/share/applications/ekos-setup.desktop    "EKOS Setup"   (the wizard in the EKOS.DEV window when
#                                        the package has the app, and it starts; else `ekos setup`
#                                        in a terminal, the wizard in the browser: RFC 0261 §8)
#   /usr/share/applications/ekos-console.desktop  "EKOS Console" (ekos console open --start)
#   /usr/share/applications/ekos-desktop.desktop  "EKOS.DEV" (/opt/ekos/bin/ekos-desktop), when the
#                                        full package has the desktop app (RFC 0261 §2.8)
# The desktop app needs WebKitGTK 4.1 and an AppIndicator library. They are recommended, not
# required, so a server gets the command without a desktop stack (apt --no-install-recommends).
# Nothing runs as root after installing except a message: the setup wizard, the EKOS home and
# the console belong to the person who runs `ekos setup`. `installer.toml` tells `ekos setup` and
# `ekos uninstall` that the package manager owns /opt/ekos: setup copies nothing and adds no PATH
# lines, uninstall removes the person's home only.
#
# The .deb is built with dpkg-deb, the .rpm with rpmbuild (on Debian/Ubuntu: `apt-get install
# rpm`). Build each architecture on its own machine: the package's architecture is the target's.
set -euo pipefail

full="$1"; out="$2"; version="$3"; which="${4:-both}"
repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
mkdir -p "$out"
out="$(cd "$out" && pwd)"

name="$(basename "$full" .tar.gz)"            # ekos-<version>-<target>-full
target="${name#ekos-"${version}"-}"; target="${target%-full}"
case "$target" in
  x86_64-unknown-linux-gnu)  deb_arch=amd64; rpm_arch=x86_64 ;;
  aarch64-unknown-linux-gnu) deb_arch=arm64; rpm_arch=aarch64 ;;
  *) echo "no Linux installer for target $target" >&2; exit 2 ;;
esac
# Package versions are numbers: v1.5.5-rc1 → 1.5.5~rc1 (sorts before 1.5.5); a manual run → the
# workspace version.
if [[ "$version" == "dev" ]]; then
  pkgver="$(sed -n '/^\[workspace.package\]/,/^\[/s/^version *= *"\([^"]*\)".*/\1/p' "$repo/ekos/Cargo.toml" | head -1)"
else
  pkgver="$version"
fi
deb_ver="${pkgver/-/\~}"
rpm_ver="${pkgver%%-*}"
rpm_rel="1"; [[ "$pkgver" == *-* ]] && rpm_rel="0.${pkgver#*-}"

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
tar -xzf "$full" -C "$work"
src="$work/$name"
[[ -x "$src/bin/ekos" ]] || { echo "$full has no bin/ekos" >&2; exit 1; }
app=0
[[ -x "$src/bin/ekos-desktop" ]] && app=1
deb_recommends="xdg-utils"
[[ "$app" = 1 ]] && deb_recommends="xdg-utils, libwebkit2gtk-4.1-0, libayatana-appindicator3-1"

# ── the tree both packages install ───────────────────────────────────────────
stage() {  # $1 = root, $2 = deb|rpm
  local root="$1" kind="$2" remove
  case "$kind" in
    deb) remove="sudo apt remove ekos" ;;
    *)   remove="sudo dnf remove ekos" ;;
  esac
  mkdir -p "$root/opt/ekos/share" "$root/usr/bin" "$root/usr/share/applications"
  cp -a "$src/bin" "$src/console" "$src/LICENSE" "$src/INSTALL.md" "$root/opt/ekos/"
  cp "$repo/packaging/msix/Assets/Square310x310Logo.png" "$root/opt/ekos/share/ekos.png"
  printf 'kind = "%s"\nremove = "%s"\n' "$kind" "$remove" > "$root/opt/ekos/installer.toml"
  ln -s /opt/ekos/bin/ekos "$root/usr/bin/ekos"
  # RFC 0261 §8: in the app's window when it can start (a server without WebKitGTK cannot: then
  # the wizard opens in the browser, as before).
  setup_exec="/usr/bin/ekos setup"
  setup_terminal=true
  if [[ "$app" = 1 ]]; then
    setup_exec='sh -c "/opt/ekos/bin/ekos-desktop --setup || exec /usr/bin/ekos setup"'
    setup_terminal=false
  fi
  cat > "$root/usr/share/applications/ekos-setup.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=EKOS Setup
Comment=Set up EKOS: its home, the web console, a first project and your AI agents
Exec=${setup_exec}
Icon=/opt/ekos/share/ekos.png
Terminal=${setup_terminal}
Categories=Development;
EOF
  cat > "$root/usr/share/applications/ekos-console.desktop" <<'EOF'
[Desktop Entry]
Type=Application
Name=EKOS Console
Comment=Open the EKOS web console, starting it when it is not running
Exec=/usr/bin/ekos console open --start
Icon=/opt/ekos/share/ekos.png
Terminal=false
Categories=Development;
EOF
  if [[ "$app" = 1 ]]; then
    cat > "$root/usr/share/applications/ekos-desktop.desktop" <<'EOF'
[Desktop Entry]
Type=Application
Name=EKOS.DEV
Comment=The EKOS console in its own window, with a tray icon and run notifications
Exec=/opt/ekos/bin/ekos-desktop
Icon=/opt/ekos/share/ekos.png
Terminal=false
Categories=Development;
StartupWMClass=ekos-desktop
EOF
  fi
  # Readable by everyone, writable by root only, whatever the build machine's umask was.
  chmod -R u+rwX,go+rX,go-w "$root"
}

message='EKOS is installed in /opt/ekos; the command is ekos.
Set it up as yourself, not as root: run `ekos setup` (or open "EKOS Setup" from your
applications menu). It opens a page in your browser for the EKOS home, the web console,
a first project and your AI agents. Your EKOS home (~/.ekos) is yours: removing this
package leaves it in place.'

# ── .deb ─────────────────────────────────────────────────────────────────────
build_deb() {
  local root="$work/deb" file="$out/ekos-${version}-${target}.deb"
  stage "$root" deb
  mkdir -p "$root/DEBIAN"
  cat > "$root/DEBIAN/control" <<EOF
Package: ekos
Version: ${deb_ver}
Architecture: ${deb_arch}
Maintainer: EKOS <https://github.com/alexeyban/EKOS>
Installed-Size: $(du -sk "$root/opt" | cut -f1)
Depends: libc6 (>= 2.34)
Recommends: ${deb_recommends}
Section: devel
Priority: optional
Homepage: https://github.com/alexeyban/EKOS
Description: EKOS, a compiler for enterprise knowledge
 The ekos command, its web console with the Python it runs on, and the
 EKOS Setup wizard. Run "ekos setup" as yourself after installing.
EOF
  {
    echo '#!/bin/sh'
    echo "cat <<'EOM'"
    printf '%s\n' "$message"
    echo 'EOM'
  } > "$root/DEBIAN/postinst"
  chmod 755 "$root/DEBIAN/postinst"
  dpkg-deb --root-owner-group -Zxz --build "$root" "$file" >/dev/null
  echo "$file"
}

# ── .rpm ─────────────────────────────────────────────────────────────────────
build_rpm() {
  local root="$work/rpm-root" top="$work/rpmbuild" file
  stage "$root" rpm
  mkdir -p "$top"/{BUILD,RPMS,SOURCES,SPECS,SRPMS}
  # No automatic requirements: the bundled Python's libraries would each become one. No
  # stripping, build ids or debug package: the binaries ship as built.
  cat > "$top/SPECS/ekos.spec" <<EOF
Name:           ekos
Version:        ${rpm_ver}
Release:        ${rpm_rel}
Summary:        EKOS, a compiler for enterprise knowledge
License:        MIT
URL:            https://github.com/alexeyban/EKOS
AutoReqProv:    no
Requires:       glibc >= 2.34
%global debug_package %{nil}
%global __os_install_post %{nil}
%global _build_id_links none
%global _binary_payload w6.xzdio

%description
The ekos command, its web console with the Python it runs on, and the
EKOS Setup wizard. Run "ekos setup" as yourself after installing.

%install
cp -a ${root}/. %{buildroot}/

%post
cat <<'EOM'
${message}
EOM

%files
%defattr(-,root,root,-)
/opt/ekos
/usr/bin/ekos
/usr/share/applications/ekos-setup.desktop
/usr/share/applications/ekos-console.desktop
EOF
  if [[ "$app" = 1 ]]; then
    # Weak dependencies by library, so they resolve whatever each distribution names its packages.
    printf '%s\n' 'Recommends:     libwebkit2gtk-4.1.so.0()(64bit)' \
      'Recommends:     libayatana-appindicator3.so.1()(64bit)' > "$work/recommends"
    sed -i "/^Requires: /r $work/recommends" "$top/SPECS/ekos.spec"
    echo /usr/share/applications/ekos-desktop.desktop >> "$top/SPECS/ekos.spec"
  fi
  rpmbuild -bb --quiet --target "$rpm_arch" --define "_topdir $top" "$top/SPECS/ekos.spec" >/dev/null
  file="$out/ekos-${version}-${target}.rpm"
  mv "$top/RPMS/$rpm_arch/"ekos-*.rpm "$file"
  echo "$file"
}

case "$which" in
  deb) build_deb ;;
  rpm) build_rpm ;;
  both) build_deb; build_rpm ;;
  *) echo "deb, rpm or both, not $which" >&2; exit 2 ;;
esac
