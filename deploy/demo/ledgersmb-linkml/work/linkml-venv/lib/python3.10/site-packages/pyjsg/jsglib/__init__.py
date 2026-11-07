import sys

from .conformance import conforms
from .jsg_strings import JSGPattern, JSGString, Boolean, Integer, Number, String, JSGPatternedValMeta
from .jsg_anytype import AnyType, AnyTypeFactory
from .jsg_array import JSGArray,ArrayFactory
from .jsg_object import JSGObject, Object, ObjectFactory
from .jsg_objectmap import JSGObjectMap
from .loader import isinstance_, load, loads
from .jsg_validateable import JSGValidateable
from .empty import Empty
from .jsg_null import JSGNull
from .jsg_context import JSGContext

if sys.version_info < (3, 7):
    from typing import _ForwardRef as ForwardRef
    from .typing_patch_36 import *
else:
    from typing import ForwardRef
    from .typing_patch_37 import *
