"""Ingests from triples using the SHACL PrefixDeclarations data model."""

from typing import Any, TextIO, Union

import rdflib
from rdflib import Graph, Literal

from prefixmaps.datamodel.context import Context


def _literal_value(v: Any) -> str:
    if isinstance(v, Literal):
        return v.value
    else:
        raise ValueError(f"Expected literal: {v}")


def from_shacl_url(url: str, name: str) -> Context:
    """
    Creates a context from a remote URL with turtle using SHACL vocabulary

    :param url:
    :param name:
    :return:
    """
    g = Graph()
    g.parse(url)
    return from_shacl_graph(g, name)


def from_shacl_file(file: Union[TextIO, str], name: str, format=None) -> Context:
    """
    Creates a context from a local turtle using SHACL vocabulary

    :param file:
    :param name:
    :param format:
    :return:
    """
    g = Graph()
    g.parse(file, format=format)
    return from_shacl_graph(g, name)


def from_shacl_graph(g: Graph, name: str) -> Context:
    """
    Creates a context from a graph using SHACL vocabulary

    :param g:
    :param name:
    :return:
    """
    shacl = rdflib.Namespace("http://www.w3.org/ns/shacl#")
    if name is None:
        raise ValueError("Must pass name")
    ctxt = Context(name)
    for subject, _, prefix in g.triples((None, shacl.prefix, None)):
        namespaces = list(g.objects(subject, shacl.namespace))
        if len(namespaces) != 1:
            raise ValueError(f"Expected exactly one ns for {prefix}, got: {namespaces}")
        ns = namespaces[0]
        ctxt.add_prefix(_literal_value(prefix), _literal_value(ns))
    return ctxt


def from_obo() -> Context:
    """
    Creates a context from official OBO SHACL namespace PURL.

    :return:
    """
    return from_shacl_url("http://obofoundry.org/registry/obo_prefixes.ttl", "obo")
