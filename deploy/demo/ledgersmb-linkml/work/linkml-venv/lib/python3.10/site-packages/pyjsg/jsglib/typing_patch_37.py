import sys
import typing
from collections.abc import Iterable, Callable

from typing import Union, get_origin
# Typing_patch module for Python 3.7

if sys.version_info >= (3, 7):
    from typing import Any, ForwardRef, _eval_type


def proc_forward(etype, namespace: dict[str, Any]):
    """ Resolve etype to an actual type if it is a forward reference """
    if type(etype) is ForwardRef:
        return _eval_type(etype, namespace, namespace)
    # Namespace can be None, for example in the test_simple_object test.
    if namespace is not None and is_union(etype):
        # This prevents resolving __args__ to the current namespace. Previously in the test it could be that an old
        # namespace was used which would cause errors as though the same class was used, it was an instance in a
        # stale namespace. This ensures that the comparison always happens against the current namespace, even when
        # modules are dynamically loaded. Another solution would be to create forked tests (test running in different
        # threads, but this is not cross platform compatible and the pytest-forked repo is also a bit outdated.
        # This way we control the solution.
        new_args = tuple(
            namespace.get(t.__forward_arg__, t) if type(t) is ForwardRef
            else namespace.get(t.__name__, t) if isinstance(t, type)
            else t
            for t in etype.__args__
        )
        return typing.Union[new_args]
    return etype


def is_union(etype) -> bool:
    """ Determine whether etype is a Union """
    origin_type = get_origin(etype)
    if origin_type:
        return get_origin(etype).__name__ == 'Union'
    return False


def is_dict(etype) -> bool:
    """ Determine whether etype is a Dict """
    return issubclass(type(etype), dict)


def is_iterable(etype) -> bool:
    """ Determine whether etype is a list or other iterable """
    return getattr(etype, '__origin__', None) is not None and issubclass(etype.__origin__, Iterable)


def union_conforms(element: Union, etype, namespace: dict[str, Any], conforms: Callable) -> bool:
    """ Determine whether element conforms to at least one of the types in etype

    :param element: element to test
    :param etype: type to test against
    :param namespace: Namespace to use for resolving forward references
    :param conforms: conformance test function
    :return: True if element conforms to at least one type in etype
    """
    return any(conforms(element, t, namespace) for t in etype.__args__)
