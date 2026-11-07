import json
from pathlib import Path
from typing import Dict, Any, List, Optional

import requests
from contextlib import closing
import logging

PREFIX_MAP = Dict[str, Any]

this_path = Path(__file__).parent


class CurieError(Exception):
    """base class"""

class NoExpansion(CurieError):
    """Thrown if no prefix exists."""
    def __init__(self, prefix, id):
        self.prefix = prefix
        self.id = id


class NoContraction(CurieError):
    """Thrown if no prefix matches."""
    def __init__(self, uri):
        self.uri = uri


class NoPrefix(CurieError):
    """Thrown if no prefix matches."""
    def __init__(self, uri):
        self.uri = uri


class AmbiguousPrefix(CurieError):
    """Thrown if multiple prefix matches."""
    def __init__(self, uri, curies):
        self.uri = uri
        self.curies = curies


class InvalidSyntax(CurieError):
    """Thrown if curie does not contain ":" ."""
    def __init__(self, id):
        self.id = id


def read_local_jsonld_context(fn) -> PREFIX_MAP:
    """
    Reads a prefix map from a JSON-LD context file from local disk
    """

    with open(fn) as file:
        return extract_prefixmap(json.load(file))


def read_remote_jsonld_context(url: str) -> PREFIX_MAP:
    """
    Returns a prefix map from a JSON-LD context from a URL

    e.g https://raw.githubusercontent.com/prefixcommons/biocontext/master/registry/monarch_context.jsonld
    """
    with closing(requests.get(url, stream=False)) as resp:
        # TODO: redirects
        if resp.status_code == 200:
            return extract_prefixmap(resp.json())
        else:
            logging.error("Cannot fetch: {}".format(url))


def extract_prefixmap(obj: Dict[str, Any]) -> PREFIX_MAP:
    if '@context' in obj:
        return obj['@context']
    else:
        return obj


def read_biocontext(name: str) -> PREFIX_MAP:
    """
    Uses prefixcommons registry

    E.g. monarch_context
    """
    path_to_jsonld = str(this_path / 'registry' / f"{name}.jsonld")
    with open(path_to_jsonld) as file:
        return extract_prefixmap(json.load(file))
    #return read_remote_jsonld_context("https://raw.githubusercontent.com/prefixcommons/biocontext/master/registry/"+name+".jsonld")
        

# TODO: configration
default_curie_maps = [read_biocontext('monarch_context'), read_biocontext('obo_context')]


def get_prefixes(cmaps: Optional[List[PREFIX_MAP]] =None):
    if cmaps is None:
        cmaps = default_curie_maps
    prefixes = []
    for cmap in cmaps:
        prefixes += cmap.keys()
    return prefixes
        

def contract_uri(uri, cmaps: Optional[List[PREFIX_MAP]] = None, strict=False, shortest=True):
    """
    Contracts a URI/IRI to a CURIE/identifier

    Note if there are ambiguous rules it is possible to have multiple (e.g. GO:nnnn and OBO:GO_nnnn),
    the strict and shortest arguments can be used to control behavior here

    Arguments
    ---------
    cmaps : list
        list of context maps
    strict: boolean
        if true, throw error if URI does not contract to exactly one CURIE. Default: False
    shortest: boolean
        if true, filter list to only include shortest. Default: True
    returns:
       a list of possible CURIES

    If strict is True, then exactly one result expected.

    """
    if cmaps is None:
        cmaps = default_curie_maps
    curies = set()
    for cmap in cmaps:
        for (k,v) in cmap.items():
            if isinstance(v, str):
                if (uri.startswith(v)):
                    curies.add(uri.replace(v, k+":"))
    curies = list(curies)
    if shortest:
        if len(curies) > 1:
            le = min(len(x) for x in curies)
            curies = [x for x in curies if len(x) == le]
    if strict:
        if len(curies) == 0:
            raise NoPrefix(uri)
        if len(curies) > 1:
            raise AmbiguousPrefix(uri, curies)
    return curies


def expand_uri(id, cmaps: Optional[List[PREFIX_MAP]] = None, strict=False):
    """
    Expands a CURIE/identifier to a URI
    """
    if cmaps is None:
        cmaps = default_curie_maps
    if id.find(":") == -1:
        if strict:
            raise InvalidSyntax(id)
        else:
            return id
    [prefix,localid] = id.split(":",1)
    for cmap in cmaps:
        if prefix in cmap:
            return cmap[prefix] + localid
    if strict:
        raise NoExpansion(prefix, id)
    else:
        return id

