"""
Cache function for rdflib-jsonld

json-ld conversions can often involve hundreds of thousands of JSON objects, each with its own context(s).  It is
horribly inefficient to fetch the context for each go-around.  This function caches contexts.
"""

from rdflib.parser import URLInputSource
from rdflib_jsonld import util

context_cache: dict[str, dict] = {}

util_fcn = util.source_to_json


def _source_to_json(source):
    global context_cache

    if isinstance(source, (URLInputSource, str)) and source in context_cache:
        return context_cache[source]

    rval = util_fcn(source)

    if isinstance(source, (URLInputSource, str)):
        context_cache[source] = rval
    return rval


util.source_to_json = _source_to_json
