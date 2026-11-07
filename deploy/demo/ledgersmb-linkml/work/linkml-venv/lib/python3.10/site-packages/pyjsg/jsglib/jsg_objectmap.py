from typing import Any, Type, TextIO

from pyjsg.jsglib.jsg_object import JSGObject
from pyjsg.jsglib.conformance import conforms
from pyjsg.jsglib.jsg_strings import JSGString
from pyjsg.jsglib.jsg_validateable import JSGValidateable
from pyjsg.jsglib.jsg_anytype import AnyType
from pyjsg.jsglib.jsg_null import JSGNull

from pyjsg.jsglib.logger import Logger


class JSGObjectMap(JSGObject):
    """
    An object map is a JsonObj with constraints on the attribute names, value types of both
    """
    _name_filter: type(JSGString) = None
    _value_type: type(JSGValidateable) = AnyType

    _min: int = 0
    _max: int | None = None

    _strict = False

    def __setattr__(self, key: str, value: Any):
        """
        Screen attributes for name and type.  Anything starting with underscore ('_') goes, anything in the IGNORE list
        and anything declared in the __init_ signature
        :param key:
        :param value:
        :return:
        """

        if not key.startswith("_") and not self._context.unvalidated_parm(key):
            if self._name_filter is not None:
                if not isinstance(key, self._name_filter):
                    raise ValueError(f"Illegal Object Map key: {key}={value}")
            if not conforms(value, self._value_type, self._context.NAMESPACE):
                raise ValueError("Illegal value type {} = {}".format(key, value))
            if not isinstance(value, JSGValidateable):
                value = self._value_type('', self._context, value) \
                    if self._value_type is AnyType else self._value_type(value)
        self[key] = value

    def __getattribute__(self, item) -> Any:
        rval = super().__getattribute__(item)
        return rval.val if type(rval) is AnyType else None if rval is JSGNull else rval

    def __getitem__(self, item):
        rval = super().__getitem__(item)
        return rval.val if type(rval) is AnyType else None if rval is JSGNull else rval

    def _is_valid_element(self, log: Logger, name: str, entry: Type[JSGValidateable]) -> bool:
        if self._name_filter is not None:
            if not self._name_filter.matches(name):
                if log.log(f"Illegal Object Map key: {name}={entry}"):
                    return False
        return super()._is_valid_element(log, name, entry)

    def _is_valid(self, log_file: Logger | TextIO | None = None) -> bool:
        log = Logger() if log_file is None else log_file if isinstance(log_file, Logger) else Logger(log_file)
        nerrors = log.nerrors
        nitems = len(self._strip_nones(self.__dict__))
        if nitems < self._min:
            if log.log(f"Number of elements is {nitems} which is less than the minimum number ({self._min})"):
                return False
        if self._max is not None and nitems > self._max:
            if log.log(f"Number of elements is {nitems} which is greater than the minimum number ({self._max})"):
                return False
        super()._is_valid(log)
        return nerrors == log.nerrors