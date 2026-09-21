#!/usr/bin/env bash
# Later: string the step PNGs into a GIF / short video.  usage: make_gif.sh [seconds-per-frame] (default 3)
set -euo pipefail
D="$(cd "$(dirname "$0")/../../.." && pwd)/docs/presentations/assets/binary-demo"
T="${1:-3}"
cd "$D"
ls [0-9][0-9]-*.png | grep -v rendered-browser | sed "s/.*/file '&'\nduration $T/" > /tmp/binary-demo-frames.txt
ffmpeg -y -loglevel error -f concat -safe 0 -i /tmp/binary-demo-frames.txt -vf "scale=1440:-2:flags=lanczos,pad=ceil(iw/2)*2:ceil(ih/2)*2:color=0b0a12,fps=10" compiled-app-to-python.mp4
ffmpeg -y -loglevel error -i compiled-app-to-python.mp4 -vf "fps=2,scale=960:-1:flags=lanczos" compiled-app-to-python.gif
echo "wrote $D/compiled-app-to-python.{mp4,gif}"
