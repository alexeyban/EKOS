from typing import Any, TextIO

from pyjsg.jsglib.empty import Empty
from pyjsg.jsglib.jsg_validateable import JSGValidateable
from pyjsg.jsglib.jsg_context import JSGContext
from pyjsg.jsglib.jsg_array import JSGArray
from pyjsg.jsglib.jsg_null import JSGNull
from pyjsg.jsglib.jsg_strings import Boolean, Integer, Number, String
from pyjsg.jsglib.logger import Logger

any_types: list[type(JSGValidateable)] = [JSGNull, Boolean, Integer, Number, String]


class AnyTypeMeta(type):
    def __instancecheck__(self, instance) -> bool:
        return instance is not Empty


class AnyType(JSGValidateable, metaclass=AnyTypeMeta):
    """ Wild card type - can hold any JSON type including JSGNull

    Assigning 'None' or 'JSGNull' to an AnyType attribute creates a JSGNull
    Assigning 'Empty' to an AnyType attribute removes it

    """
    _strict = False

    def __init__(self, variable_name: str, context: JSGContext,  value: Any, **kwargs):
        """ Construct a wild card variable

        :param variable_name: name of attribute for error reporting
        :param context: context for use in JSGArrays
        :param val: value
        :param kwargs: named arguments
        """
        match = False
        if isinstance(value, list):
            self.val = JSGArray(variable_name, context, AnyType, 0, None, value)
        else:
            for t in any_types:
                if isinstance(value, t):
                    self.val = t(value)
                    match = True
                    break
            if not match:
                self.val = value

        super().__init__(**kwargs)

    def _is_valid(self, log: TextIO | Logger | None = None) -> bool:
        return self.val is not Empty

    def __str__(self):
        return str(self.val)


class AnyTypeWrapperMeta(type):
    variable_name: str
    context: JSGContext

    def __instancecheck__(self, instance: Any) -> bool:
        return instance is not Empty


class AnyTypeWrapper(metaclass=AnyTypeWrapperMeta):
    variable_name: str
    context: JSGContext

    def __new__(cls, value):
        return AnyType(cls.variable_name, cls.context, value)


def AnyTypeFactory(name: str, context: JSGContext) -> type(AnyTypeWrapper):
    factory = type(name, (AnyTypeWrapper,), dict())
    factory.variable_name = name
    factory.context = context
    return factory
