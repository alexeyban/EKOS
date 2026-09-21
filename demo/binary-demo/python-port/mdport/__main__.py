"""`python -m mdport < in.md > out.html` — same contract as the original mdcli.exe."""

import sys

from .markdown import Markdown

data = sys.stdin.buffer.read().decode("utf-8")
sys.stdout.buffer.write(Markdown().transform(data).encode("utf-8"))
