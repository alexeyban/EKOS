#!/usr/bin/env bash
# Builds the macOS installer (.pkg) from one full package (scripts/package-full.sh). Runs on macOS:
# pkgbuild, productbuild, sips and iconutil come with it.
#
#   scripts/package-macos.sh <full package .tar.gz> <out dir> <version>
#
# Installs (for every user of the Mac, so it needs an administrator's password):
#   /usr/local/ekos/{bin,console,LICENSE,INSTALL.md,installer.toml,setup.command,uninstall.sh}
#   /usr/local/bin/ekos              → /usr/local/ekos/bin/ekos
#   /Applications/EKOS Setup.app     opens Terminal with `ekos setup` (the setup wizard)
#   /Applications/EKOS Console.app   `ekos console open --start`
# The package's postinstall runs as root, so it never runs the wizard itself: it opens EKOS Setup
# as the person logged in at the screen, and not at all for a command-line install.
# `installer.toml` tells `ekos setup` and `ekos uninstall` that the program is the package's:
# setup copies nothing and adds no PATH lines, uninstall removes the person's home only.
#
# Unsigned. Opening the downloaded .pkg the first time: right-click → Open (or System Settings →
# Privacy & Security → Open Anyway). See INSTALL.md.
set -euo pipefail

full="$1"; out="$2"; version="$3"
repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
mkdir -p "$out"
out="$(cd "$out" && pwd)"
name="$(basename "$full" .tar.gz)"
target="${name#ekos-"${version}"-}"; target="${target%-full}"
case "$target" in
  aarch64-apple-darwin) host_arch=arm64 ;;
  x86_64-apple-darwin)  host_arch=x86_64 ;;
  *) echo "no macOS installer for target $target" >&2; exit 2 ;;
esac
if [[ "$version" == "dev" ]]; then
  pkgver="$(sed -n '/^\[workspace.package\]/,/^\[/s/^version *= *"\([^"]*\)".*/\1/p' "$repo/ekos/Cargo.toml" | head -1)"
else
  pkgver="${version%%-*}"
fi
id="com.ekos.cli"

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
tar -xzf "$full" -C "$work"
src="$work/$name"
[[ -x "$src/bin/ekos" ]] || { echo "$full has no bin/ekos" >&2; exit 1; }

root="$work/root"
prog="$root/usr/local/ekos"
mkdir -p "$prog" "$root/usr/local/bin" "$root/Applications"
cp -a "$src/bin" "$src/console" "$src/LICENSE" "$src/INSTALL.md" "$prog/"
printf 'kind = "pkg"\nremove = "sudo /usr/local/ekos/uninstall.sh"\n' > "$prog/installer.toml"
ln -s /usr/local/ekos/bin/ekos "$root/usr/local/bin/ekos"

cat > "$prog/setup.command" <<'EOF'
#!/bin/sh
# The EKOS setup wizard, in your browser; this window shows its address until you finish.
exec /usr/local/ekos/bin/ekos setup
EOF
cat > "$prog/uninstall.sh" <<'EOF'
#!/bin/bash
# Removes the EKOS program this Mac's installer put in place. Your EKOS home (~/.ekos, your
# projects) stays: to undo what `ekos setup` did for you, run `ekos uninstall` first.
set -eu
if [[ "$(id -u)" != 0 ]]; then exec sudo "$0" "$@"; fi
if [[ "$(readlink /usr/local/bin/ekos 2>/dev/null)" = /usr/local/ekos/bin/ekos ]]; then
  rm -f /usr/local/bin/ekos
fi
rm -rf "/Applications/EKOS Setup.app" "/Applications/EKOS Console.app" /usr/local/ekos
pkgutil --forget com.ekos.cli >/dev/null 2>&1 || true
echo "EKOS is removed. Your EKOS home is where it was."
EOF
chmod 755 "$prog/setup.command" "$prog/uninstall.sh"

# ── the two apps ─────────────────────────────────────────────────────────────
icns=""
iconset="$work/ekos.iconset"
mkdir -p "$iconset"
png="$repo/packaging/msix/Assets/Square310x310Logo.png"
make_icns() {
  local s
  for s in 16 32 128 256; do
    sips -z "$s" "$s" "$png" --out "$iconset/icon_${s}x${s}.png" >/dev/null || return 1
    sips -z $((s * 2)) $((s * 2)) "$png" --out "$iconset/icon_${s}x${s}@2x.png" >/dev/null || return 1
  done
  iconutil -c icns "$iconset" -o "$work/ekos.icns"
}
if make_icns; then
  icns="$work/ekos.icns"
else
  echo "no icon: sips/iconutil failed; the apps get the default icon" >&2
fi

app() {  # name, bundle id suffix, command run by the app
  local title="$1" suffix="$2" command="$3"
  local bundle="$root/Applications/$title.app"
  mkdir -p "$bundle/Contents/MacOS" "$bundle/Contents/Resources"
  printf '#!/bin/sh\n%s\n' "$command" > "$bundle/Contents/MacOS/run"
  chmod 755 "$bundle/Contents/MacOS/run"
  [[ -n "$icns" ]] && cp "$icns" "$bundle/Contents/Resources/ekos.icns"
  cat > "$bundle/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>${title}</string>
  <key>CFBundleDisplayName</key><string>${title}</string>
  <key>CFBundleIdentifier</key><string>com.ekos.${suffix}</string>
  <key>CFBundleExecutable</key><string>run</string>
  <key>CFBundleIconFile</key><string>ekos</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>${pkgver}</string>
  <key>CFBundleVersion</key><string>${pkgver}</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>LSUIElement</key><true/>
</dict>
</plist>
EOF
}
app "EKOS Setup" setup 'exec open -a Terminal /usr/local/ekos/setup.command'
app "EKOS Console" console 'exec /usr/local/ekos/bin/ekos console open --start'

# ── postinstall: EKOS Setup for the person at the screen ─────────────────────
scripts="$work/scripts"
mkdir -p "$scripts"
cat > "$scripts/postinstall" <<'EOF'
#!/bin/bash
# Runs as root. The wizard must run as the person who will use EKOS, so it is opened in their
# session; a command-line install (installer -pkg) opens nothing.
[[ -n "${COMMAND_LINE_INSTALL:-}" ]] && exit 0
user="$(stat -f%Su /dev/console 2>/dev/null || true)"
case "$user" in ""|root|loginwindow) exit 0 ;; *) ;; esac
uid="$(id -u "$user" 2>/dev/null)" || exit 0
launchctl asuser "$uid" sudo -u "$user" open "/Applications/EKOS Setup.app" >/dev/null 2>&1 || true
exit 0
EOF
chmod 755 "$scripts/postinstall"

# ── the component: apps installed where they are, never moved to another copy ─
plist="$work/component.plist"
pkgbuild --analyze --root "$root" "$plist" >/dev/null
count="$(plutil -convert json -o - "$plist" | python3 -c 'import json,sys; print(len(json.load(sys.stdin)))')"
for ((i = 0; i < count; i++)); do
  plutil -replace "$i.BundleIsRelocatable" -bool NO "$plist"
done
mkdir -p "$work/pkgs"
pkgbuild --root "$root" --component-plist "$plist" --identifier "$id" --version "$pkgver" \
  --install-location / --scripts "$scripts" "$work/pkgs/ekos-component.pkg" >/dev/null

# ── the product: welcome, licence, conclusion ────────────────────────────────
res="$work/resources"
mkdir -p "$res"
cp "$repo/LICENSE" "$res/LICENSE.txt"
cat > "$res/welcome.html" <<EOF
<html><body style="font-family:-apple-system,sans-serif">
<h2>EKOS ${pkgver}</h2>
<p>Installs the <code>ekos</code> command, its web console and the Python it runs on in
<code>/usr/local/ekos</code>, puts <code>ekos</code> in <code>/usr/local/bin</code>, and adds
<b>EKOS Setup</b> and <b>EKOS Console</b> to Applications.</p>
<p>When it finishes, EKOS Setup opens: a page in your browser for your EKOS home, the web console,
a first project and your AI agents.</p>
</body></html>
EOF
cat > "$res/conclusion.html" <<'EOF'
<html><body style="font-family:-apple-system,sans-serif">
<h2>EKOS is installed</h2>
<p>EKOS Setup opens in Terminal and your browser. Later, open it from Applications, or run
<code>ekos setup</code> in a terminal.</p>
<p>To remove the program: <code>sudo /usr/local/ekos/uninstall.sh</code>. Your EKOS home
(<code>~/.ekos</code>) stays; <code>ekos uninstall</code> removes it.</p>
</body></html>
EOF
cat > "$work/distribution.xml" <<EOF
<?xml version="1.0" encoding="utf-8"?>
<installer-gui-script minSpecVersion="2">
  <title>EKOS</title>
  <welcome file="welcome.html" mime-type="text/html"/>
  <license file="LICENSE.txt"/>
  <conclusion file="conclusion.html" mime-type="text/html"/>
  <options customize="never" require-scripts="false" hostArchitectures="${host_arch}"/>
  <domains enable_anywhere="false" enable_currentUserHome="false" enable_localSystem="true"/>
  <volume-check><allowed-os-versions><os-version min="11.0"/></allowed-os-versions></volume-check>
  <choices-outline><line choice="default"><line choice="${id}"/></line></choices-outline>
  <choice id="default"/>
  <choice id="${id}" visible="false"><pkg-ref id="${id}"/></choice>
  <pkg-ref id="${id}" version="${pkgver}" onConclusion="none">ekos-component.pkg</pkg-ref>
</installer-gui-script>
EOF
file="$out/ekos-${version}-${target}.pkg"
productbuild --distribution "$work/distribution.xml" --resources "$res" \
  --package-path "$work/pkgs" "$file" >/dev/null
echo "$file"
