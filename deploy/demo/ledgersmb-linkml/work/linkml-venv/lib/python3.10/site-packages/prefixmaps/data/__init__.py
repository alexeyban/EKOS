from pathlib import Path
from typing import Mapping

__all__ = [
    "data_path",
    "context_paths",
]

data_path = Path(__file__).parent

#: A mapping from contexts to their paths
context_paths: Mapping[str, Path] = {path.stem: path for path in data_path.glob("*.csv")}
