from .typing_patch_37 import *
from typing import Any


def element_conforms(element, etype) -> bool:
    """ Determine whether element conforms to etype"""
    from pyjsg.jsglib import Empty

    if isinstance(element, etype):
        return True
    # This catches the Optional[] idiom
    if (element is None or element is Empty) and issubclass(etype, type(None)):
        return True
    elif element is Empty:
        return False
    elif isinstance(etype, type(type)) and (issubclass(etype, type(None))):
        return element is None
    elif element is None:
        return False
    else:
        return isinstance(element, etype)


def conforms(element, etype, namespace: dict[str, Any]) -> bool:
    """ Determine whether element conforms to etype

    :param element: Element to test for conformance
    :param etype: Type to test against
    :param namespace: Namespace to use to resolve forward references
    :return:
    """
    etype = proc_forward(etype, namespace)
    if is_union(etype):
        return union_conforms(element, etype, namespace, conforms)
    else:
        return element_conforms(element, etype)