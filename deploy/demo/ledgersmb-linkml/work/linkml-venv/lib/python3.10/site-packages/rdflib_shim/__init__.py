from typing import Optional

from rdflib import Graph, __version__

# You can reference this variable to make sure that import cleanups don't remove
# the necessary import.  Example:
# import rdflib_shim
# shimed = rdflib_shim.RDFLIB_SHIM
RDFLIB_SHIM = True


class DecodableStr(str):
    """ A string with a decode method """
    def decode(self) -> str:
        return self


orig_serialize = Graph.serialize

if __version__.startswith("6."):
    """ Add a decode() method to what comes out of the serializer """


    def serialize_shim(*args, **kwargs) -> Optional[DecodableStr]:
        rval = orig_serialize(*args, **kwargs)
        return DecodableStr(rval) if isinstance(rval, str) else rval

    Graph.serialize = serialize_shim
elif __version__.startswith("5."):
    """ Change the serializer output to return a (decodable) string """
    def serialize_shim(*args, **kwargs) -> Optional[DecodableStr]:
        rval = orig_serialize(*args, **kwargs)
        return DecodableStr(rval.decode()) if isinstance(rval, bytes) else rval

    Graph.serialize = serialize_shim
