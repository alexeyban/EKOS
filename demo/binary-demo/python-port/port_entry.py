"""Entry point for ekos-characterize: exposes the port's Markdown class from a single file path."""
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from mdport import Markdown  # noqa: E402,F401
