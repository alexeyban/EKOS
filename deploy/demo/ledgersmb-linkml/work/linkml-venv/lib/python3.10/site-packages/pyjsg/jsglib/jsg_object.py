from collections import OrderedDict
from typing import Any, cast, Type, TextIO

from jsonasobj import JsonObj

from pyjsg.jsglib.jsg_array import JSGArray
from pyjsg.jsglib.jsg_strings import JSGString
from pyjsg.jsglib.jsg_context import JSGContext
from pyjsg.jsglib.jsg_null import JSGNull
from pyjsg.jsglib.jsg_validateable import JSGValidateable
from pyjsg.jsglib.empty import Empty
from pyjsg.jsglib import AnyType
from pyjsg.jsglib.logger import Logger
from pyjsg.jsglib.conformance import conforms

from .typing_patch_37 import is_union, proc_forward


class JSGObjectMeta(type):
    _reference_types: list["JSGObject"] = []
    _reference_names: list[str]                 # Names of objects in _reference_types

    def __init__(cls, *args, **kwargs):
        super().__init__(*args, **kwargs)
        cls._reference_names = [t.__name__ for t in cls._reference_types]


class JSGObject(JsonObj, JSGValidateable, metaclass=JSGObjectMeta):
    """
    JSGObject is a JsonObj with constraints.

    Note that methods and variables in JSGObject should always begin with "_", as we currently restrict the set of
    JSON names to those that begin with [a-zA-Z]
    """
    _reference_types: list["JSGObject"] = []        # Types that can be used as compound constructors
    _reference_names: list[str] = []                # Names of the reference types for assignment testing
    _members: dict[str, type] = {}                  # Names of actual member elements
    _strict: bool = True                            # True means no additional members allowed, False means "open"

    def __init__(self, context: JSGContext, **kwargs):
        """ Generic constructor

        :param context: Context for TYPE and IGNORE variables
        :param kwargs: Initial values - object specific
        """
        JsonObj.__init__(self)
        self._context = context
        if self._class_name not in context.TYPE_EXCEPTIONS and context.TYPE:
            self[context.TYPE] = self._class_name
        for k, v in kwargs.items():
            setattr(self, k, kwargs[k])

    def __setattr__(self, key: str, value: Any) -> None:
        """ Attribute setter.  Any attribute that is part of the members list or not validated passes.  Otherwise
        setting is only allowed if the class level _strict mode is False

        :param key:
        :param value:
        :return:
        """
        # Note: The initial startswith below is the only way we can SEE _members or _context...

        if key.startswith('_') or key in self._members or self._context.unvalidated_parm(key):
            if key in self._members:
                vtype = self._members[key]
                # Empty can be used to "remove" any type
                if value is Empty:
                    self[key] = Empty
                # None is a valid value for some types (JSGNull, AnyType) but removes the other types
                elif value is None:
                    self[key] = self._map_jsg_type(key, value, vtype)
                else:
                    self[key] = self._jsg_type_for(key, value, vtype)
            else:
                cur_val = self.__dict__.get(key)
                if cur_val is None or not issubclass(type(cur_val), JSGString):
                    self[key] = value
                else:
                    self[key] = cur_val.__class__(value)
        elif self._strict:
            raise ValueError("Unknown attribute: {}={}".format(key, value))
        else:
            super().__setattr__(key, value)

    def __getattribute__(self, item: str) -> Any:
        if item[0] == '_':
            return super().__getattribute__(item)
        if item in super().__dict__:
            rval = super().__getattribute__(item)
        else:
            return None
        return rval.val if type(rval) is AnyType else None if rval is JSGNull else rval

    def __getitem__(self, item) -> Any:
        rval = super().__getitem__(item)
        return rval.val if type(rval) is AnyType else None if rval is JSGNull else rval

    def __delattr__(self, item):
        from pyjsg.jsglib.jsg_strings import JSGString
        attr = getattr(self, item)
        setattr(self, item, type(attr)(None) if issubclass(type(attr), JSGString) else None)

    @staticmethod
    def _strip_nones(d: dict[str, Any])-> dict[str, Any]:
        """
        An attribute with type None is equivalent to an absent attribute.
        :param d: Object with attributes
        :return: Object dictionary w/ Nones and underscores removed
        """
        return OrderedDict({k: None if isinstance(v, JSGNull) else v for k, v in d.items()
                            if not k.startswith("_") and v is not None and v is not Empty and
                            (issubclass(type(v), JSGObject) or
                             (not issubclass(type(v), JSGString) or v.val is not None) and
                             (not issubclass(type(v), AnyType) or v.val is not Empty))})

    @staticmethod
    def _test(entry, log: Logger) -> bool:
        """Test whether entry conforms to its type

        :param entry: entry to test
        :param log: place to record issues
        :return: True if it meets requirements
        """
        if isinstance(entry, JSGValidateable):
            if not entry._is_valid(log) and not log.logging:
                return False
        return True

    def _default(self, obj: object):
        """ Return a serializable version of obj. Overrides JsonObj _default method
        :param obj: Object to be serialized
        :return: Serialized version of obj
        """
        return None if obj is JSGNull else obj.val if type(obj) is AnyType else \
            JSGObject._strip_nones(obj.__dict__) if isinstance(obj, JsonObj) \
            else cast(JSGString, obj).val if issubclass(type(obj), JSGString) else str(obj)

    def _is_valid_element(self, log: Logger, name: str, val: Type[JSGValidateable]) -> bool:
        if name not in self._members:
            return any(e._is_valid_element for e in self._reference_types)
        else:
            etype = self._members[name]
            if etype is JSGNull:
                return val is JSGNull or val is None
            if val is None and type(etype) is type(AnyType):
                return False
            if val is not None and val is not Empty and isinstance(val, JSGArray):
                if not val._validate(cast(list, val), log)[0] and not log.logging:
                    return False
            elif not conforms(val, etype, self._context.NAMESPACE):        # Note: None and absent are equivalent
                if val is None or val is Empty:
                    if log.log("{}: Missing required field: '{}'".format(self.__class__.__name__, name)):
                        return False
                else:
                    if log.log("{}: Type mismatch for {}. Expecting: {} Got: {}"
                               .format(self.__class__.__name__, name, etype, type(val))):
                        return False
            elif val is not None and not self._test(val, log):  # Make sure that entry conforms to its own type
                return False
        return True

    def _is_valid(self, log_file: Logger | TextIO | None = None) -> bool:
        log = Logger() if log_file is None else log_file if isinstance(log_file, Logger) else Logger(log_file)
        nerrors = log.nerrors

        if self._context.TYPE and getattr(self, self._context.TYPE, "") != self._class_name \
                and self._class_name not in self._context.TYPE_EXCEPTIONS:
            if log.log("Type mismatch - Expected: {} Actual: {}"
                       .format(self._class_name, getattr(self, self._context.TYPE))):
                return False

        for name in self._members.keys():
            entry = getattr(self, name)
            if not self._is_valid_element(log, name, entry):
                return False

        if self._strict:
            # Test each attribute against the schema
            for k, v in self._strip_nones(self.__dict__).items():
                if k not in self._members and k != self._context.TYPE \
                        and k not in self._context.IGNORE and k != "@context":
                    if not self._is_valid_element(log, k, v):
                        if log.log("Extra element: {}: {}".format(k, v)):
                            return False

        return log.nerrors == nerrors

    def _map_jsg_type(self, name: str, element: Any, poss_types: type | tuple[type]) -> JSGValidateable | None:
        def _wrap(ty, el):
            # The first option is when we are assigning already loaded types
            # The second option addresses the optional situation
            return el if isinstance(el, JSGValidateable) or ty is type(None) else ty(el)

        for typ in poss_types if isinstance(poss_types, tuple) else (poss_types, ):
            if isinstance(typ, str):
                if typ in self._context.NAMESPACE:
                    typ = self._context.NAMESPACE[typ]
                else:
                    raise ValueError(f"Unknown type: {typ}")
            typ = proc_forward(typ, self._context.NAMESPACE)
            if is_union(typ):
                for t in typ.__args__:
                    et = self._map_jsg_type(name, element, t)
                    if et is not None:
                        return et
            elif conforms(element, typ, self._context.NAMESPACE):
                return _wrap(typ, element)
        return None

    def _jsg_type_for(self, name: str, element: Any, poss_types: type | tuple[type]) -> JSGValidateable:
        et = self._map_jsg_type(name, element, poss_types)
        if et is not None:
            return et
        raise ValueError(f"Wrong type for {name}: {Logger.json_repr(element)} - expected:"
                         f" {poss_types} got {type(element).__name__}")


class ObjectWrapperMeta(type):
    variable_name: str
    context: JSGContext
    typ: type

    def __instancecheck__(self, element: object) -> bool:
        return conforms(element, JSGObject, self.context.NAMESPACE)


class ObjectWrapper(metaclass=ObjectWrapperMeta):
    variable_name: str
    context: JSGContext
    typ: type

    def __new__(cls, value):
        return cls.typ(cls.variable_name, cls.context, value)


def ObjectFactory(name: str, context: JSGContext, typ: type) -> type(ObjectWrapper):
    factory = type(name, (ObjectWrapper, ), dict())
    factory.variable_name = name
    factory.context = context
    factory.typ = typ
    return factory


class Object(JSGObject):
    """ Object is an non-validated inner construct.  Object must still conform to a JSG object definition, but it
    can be *any* OBJECT
    """
    _strict = False

    def __init__(self, variable_name: str,  _context: JSGContext, value: JsonObj | None):
        self._variable_name = variable_name
        if value is not None and not isinstance(value, JsonObj):
            raise ValueError(f'{variable_name}: Invalid {self._class_name} value: "{value}"')
        super().__init__(_context, **({} if value is None else value.__dict__))
