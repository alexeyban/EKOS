import json
import types

from pyjsg.jsglib.jsg_validateable import JSGValidateable
from .logger import *

from .typing_patch_37 import is_union

UNKNOWN_TYPE_EXCEPTION = "Type '{}' is undefined"


def loads_loader(load_module: types.ModuleType, pairs: dict[str, str]) -> JSGValidateable | None:
    """json loader objecthook

    :param load_module: Module that contains the various types
    :param pairs: key/value tuples (In our case, they are str/str)
    :return:
    """
    cntxt = load_module._CONTEXT

    # If the type element is a member of the JSON, load it
    possible_type = pairs[cntxt.TYPE] if cntxt.TYPE in pairs else None
    target_class = getattr(load_module, possible_type, None) if isinstance(possible_type, str) else None
    if target_class:
        return target_class(**pairs)

    # See whether there are any exception types that are valid for the incoming data
    for type_exception in cntxt.TYPE_EXCEPTIONS:
        if not hasattr(load_module, type_exception):
            raise ValueError(UNKNOWN_TYPE_EXCEPTION.format(type_exception))
        target_class = getattr(load_module, type_exception)
        target_strict = target_class._strict
        target_class._strict = False
        try:
            rval = target_class(**pairs)
        finally:
            target_class._strict = target_strict
        if is_valid(rval):
            return rval

    # If there is not a type variable and nothing fits, just load up the first (and perhaps only) exception
    # It will later fail any is_valid tests
    if not cntxt.TYPE and cntxt.TYPE_EXCEPTIONS:
        return getattr(load_module, cntxt.TYPE_EXCEPTIONS[0])(**pairs)

    if cntxt.TYPE in pairs:
        raise ValueError(f'Unknown reference type: "{cntxt.TYPE}": "{pairs[cntxt.TYPE]}"')
    else:
        raise ValueError(f'Missing "{cntxt.TYPE}" element')


def loads(s: str, load_module: types.ModuleType, **kwargs):
    """ Convert a JSON string into a JSGObject

    :param s: string representation of JSON document
    :param load_module: module that contains declarations for types
    :param kwargs: arguments see: json.load for details
    :return: JSGObject representing the json string
    """
    return json.loads(s, object_hook=lambda pairs: loads_loader(load_module, pairs), **kwargs)


def load(fp: TextIO | str, load_module: types.ModuleType, **kwargs):
    """ Convert a file name or file-like object containing stringified JSON into a JSGObject

    :param fp: file-like object to deserialize
    :param load_module: module that contains declarations for types
    :param kwargs: arguments see: json.load for details
    :return: JSGObject representing the json string
    """
    if isinstance(fp, str):
        with open(fp) as f:
            return loads(f.read(), load_module, **kwargs)
    else:
        return loads(fp.read(), load_module, **kwargs)


def isinstance_(x, A_tuple):
    """ native isinstance_ with the test for typing.Union overridden """
    if is_union(A_tuple):
        return any(isinstance_(x, t) for t in A_tuple.__args__)
    elif getattr(A_tuple, '__origin__', None) is not None:
        return isinstance(x, A_tuple.__origin__)
    else:
        return isinstance(x, A_tuple)


def is_valid(obj: JSGValidateable, log: TextIO | Logger | None = None) -> bool:
    """ Determine whether obj is valid

    :param obj: Object to validate
    :param log: Logger to record validation failures.  If absent, no information is recorded
    """
    return obj._is_valid(log)
