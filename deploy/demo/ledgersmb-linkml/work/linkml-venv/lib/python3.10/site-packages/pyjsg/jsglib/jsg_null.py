from typing import TextIO

from pyjsg.jsglib.jsg_validateable import JSGValidateable
from pyjsg.jsglib.logger import Logger


class JSGNullMeta(type):
    def __instancecheck__(self, instance) -> bool:
        return instance is None or instance is JSGNull


class JSGNull(JSGValidateable, metaclass=JSGNullMeta):
    """ Wrapper for the JSON null class

    Assigning 'None' or 'JSGNull' to an attribute of type JSGNull causes it to exist
    Assigning 'Empty' to an attribute of type JSGNull removes it
    Assigning anything else is an error
    """
    val = None

    def __new__(cls, val):
        if val is None or val is JSGNull:
            return cls
        raise ValueError(f"Invalid Null Value: {val}")

    def _is_valid(self, log: TextIO | Logger | None = None) -> bool:
        return True
