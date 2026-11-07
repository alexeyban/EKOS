from pyjsg.jsglib.jsg_context import JSGContext
from pyjsg.jsglib.jsg_validateable import JSGValidateable
from pyjsg.jsglib.loader import Logger
from pyjsg.jsglib.conformance import conforms


class JSGArray(list, JSGValidateable):
    def __init__(self, variable_name: str, context: JSGContext, typ, min_: int, max_: int | None,
                 value: list | None) -> None:
        """ Construct an array holder

        :param variable_name: Name assigned to the variable.  Used for error reporting
        :param context: Supporting context
        :param typ: array type
        :param min_: minimum number of elements
        :param max_: maximum number of elements (None means '*')
        :param value: Initializer
        """
        self._variable_name = variable_name
        self._context = context
        self._type = typ
        self._min: int = min_
        self._max: int | None = max_
        if value is not None:
            isvalid, errors = self._validate(value)
            if not isvalid:
                raise ValueError("\n".join(errors))
        super().__init__([] if value is None else value)    # Construct the actual list

    def _is_valid(self, log: Logger | None = None) -> bool:
        """ Determine whether the current contents are valid """
        return self._validate(self, log)[0]

    def _validate(self, val: list, log: Logger | None = None) -> tuple[bool, list[str]]:
        """ Determine whether val is a valid instance of this array

        :returns: Success indicator and error list """
        errors = []
        if not isinstance(val, list):
            errors.append(f"{self._variable_name}: {repr(val)} is not an array")
        else:
            for i in range(0, len(val)):
                v = val[i]
                if not conforms(v, self._type, self._context.NAMESPACE):
                    errors.append(f"{self._variable_name} element {i}: {v} is not a {self._type.__name__}")

            if len(val) < self._min:
                errors.append(
                    f"{self._variable_name}: at least {self._min} value{'s' if self._min > 1 else ''} required - "
                    f"element has {len(val) if len(val) else 'none'}")
            if self._max is not None and len(val) > self._max:
                errors.append(
                    f"{self._variable_name}: no more than {self._max} values permitted - element has {len(val)}")

        if log:
            for error in errors:
                log.log(error)
        return not bool(errors), errors


class ArrayWrapperMeta(type):
    variable_name: str
    context: JSGContext
    typ: type
    min: int
    max: int | None

    def __instancecheck__(self, instance: list) -> bool:
        if not isinstance(instance, list):
            return False
        for element in instance:
            if not conforms(element, self.typ, self.context.NAMESPACE):
                return False
        return len(instance) >= self.min and (not self.max or len(instance) <= self.max)


class ArrayWrapper(metaclass=ArrayWrapperMeta):
    variable_name: str
    context: JSGContext
    typ: type
    min: int
    max: int | None

    def __new__(cls, value):
        return JSGArray(cls.variable_name, cls.context, cls.typ, cls.min, cls.max, value)


def ArrayFactory(name: str, context: JSGContext, typ: type, min_: int, max_: int | None) -> type(ArrayWrapper):
    factory = type(name, (ArrayWrapper,), dict())
    factory.variable_name = name
    factory.context = context
    factory.typ = typ
    factory.min = min_
    factory.max = max_
    return factory
