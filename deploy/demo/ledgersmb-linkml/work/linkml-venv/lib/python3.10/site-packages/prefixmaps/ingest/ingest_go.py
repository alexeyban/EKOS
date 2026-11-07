"""Ingests the GO prefix registry."""

from typing import TextIO, Union

import requests
import yaml

from prefixmaps.datamodel.context import Context

URL = "https://raw.githubusercontent.com/geneontology/go-site/master/metadata/db-xrefs.yaml"  # noqa: E501


def parse_go_xrefs_from_remote() -> Context:
    r = requests.get(URL)
    return parse_go_xrefs(r.text)


def parse_go_xrefs(input: Union[str, TextIO]) -> Context:
    """
    Parse GO db-xrefs.yaml file.

    Note that most entries in the file are ignored. We only extract the
    "embedded JSON-LD context" which are those marked rdf_uri_prefix,
    which indicates the *semantic* expansions used in the triplestore.

    :param file:
    :return:
    """
    prefixes = yaml.safe_load(input)
    context = Context("go")
    for p in prefixes:
        if "rdf_uri_prefix" in p:
            ns = p["rdf_uri_prefix"]
            if ns[-1] not in ["/", "#", "_"]:
                ns += "/"
            context.add_prefix(p["database"], ns)
    return context
