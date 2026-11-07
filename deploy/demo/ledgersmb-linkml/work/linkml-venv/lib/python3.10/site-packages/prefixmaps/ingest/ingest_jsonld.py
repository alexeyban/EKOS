"""Generic JSON-LD ingests."""

import json
from typing import Any, Dict, List, Optional, TextIO, Union

import requests

from prefixmaps.datamodel.context import Context

AT_CONTEXT = "@context"

PREFIXCC_EXCLUDE = [
    "bp",
    "terms",
    "dc",
    "ma",
    "ont",
    "fb",
    "obo",
    "http",
    "dcterm",
    "dc11",
    "uniprot",
    "go",
    "gold",
    "chebi",
]


def from_jsonld_context_url(url: str, name: str, excludes: Optional[List[str]] = None) -> Context:
    """
    Ingests from a remote JSON-LD context.

    :param url:
    :param name:
    :param excludes:
    :return:
    """
    response = requests.get(url=url)
    if name is None:
        name = url
    return from_jsonld_context(response.json(), name, excludes)


def from_jsonld_context_file(
    file: Union[TextIO, str], name: str, excludes: Optional[List[str]] = None
) -> Context:
    """
    Ingests from a local JSON-LD context.

    :param file:
    :param name:
    :param excludes:
    :return:
    """
    if isinstance(file, str):
        if name is None:
            name = file
        with open(file) as stream:
            return from_jsonld_context_file(stream, name, excludes)
    else:
        return from_jsonld_context(json.load(file), name, excludes)


def from_jsonld_context(
    jsonld_context: Dict[str, Any],
    name: str,
    excludes: Optional[List[str]] = None,
) -> Context:
    """
    Ingests from a JSON-LD context stored as a dictionary.

    .. note::

        Does not support JSON-LD 1.1 contexts.

    :param jsonld_context:
    :param name:
    :param excludes:
    :return:
    """
    if name is None:
        raise ValueError("Must pass name")
    ctxt = Context(name)
    if AT_CONTEXT in jsonld_context:
        for k, v in jsonld_context[AT_CONTEXT].items():
            if k in excludes:
                continue
            if isinstance(v, str):
                ctxt.add_prefix(k, v)
            else:
                raise NotImplementedError("JSONLD 1.1 contexts not implemented")
        return ctxt
    else:
        raise ValueError(f"Expected {AT_CONTEXT}")


def from_prefixcc() -> Context:
    """
    Ingests from prefix.cc.

    Note that prefix.cc is an extremely messy source, but it can be useful
    for semweb namespaces.

    In order to prioritize "better" prefixes we have an exclusion list, that attempts to
    exclude the most egregious entries, and prioritize more official sources. However,
    there are still no guarantees as to quality.

    Longer term we should focus on the curated linked_data.yaml file, and
    moving this to bioregistry.

    :return:
    """
    return from_jsonld_context_url("http://prefix.cc/context.jsonld", "prefixcc", PREFIXCC_EXCLUDE)
