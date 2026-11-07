from .datamodel.context import Context, PrefixExpansion, StatusType
from .io.parser import load_context, load_converter, load_multi_context

try:
    from importlib.metadata import version
except ImportError:  # for Python<3.8
    from importlib_metadata import version

__all__ = [
    "load_converter",
    "load_context",
    "load_multi_context",
    "Context",
    "StatusType",
    "PrefixExpansion",
]
__version__ = version(__name__)
