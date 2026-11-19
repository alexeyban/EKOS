#!/usr/bin/env bash
# Run every fixture through the ORIGINAL (wine-mono) and the Python PORT, and compare byte for byte.
set -u
HERE="$(cd "$(dirname "$0")/.." && pwd)"
PY="${PYTHON:-python3.13}"
export WINEPREFIX="${WINEPREFIX:-$HOME/.cache/ekos-characterize/prefix}" WINEDEBUG=-all
pass=0; fail=0
for f in "$HERE"/inputs/*.md; do
  n=$(basename "$f")
  case "$n" in 99-*) continue;; esac   # System.Random in EncodeEmailAddress: compared separately
  orig=$(wine "$HERE/work/original/mdcli.exe" < "$f" | sha256sum | cut -c1-12)
  port=$( (cd "$HERE/python-port" && $PY -m mdport < "$f") | sha256sum | cut -c1-12)
  if [ "$orig" = "$port" ]; then printf '  identical  %-28s sha256 %s\n' "$n" "$orig"; pass=$((pass+1))
  else printf '  DIFFERENT  %-28s original %s  port %s\n' "$n" "$orig" "$port"; fail=$((fail+1)); fi
done
echo "$pass identical, $fail different"
[ "$fail" = 0 ]
