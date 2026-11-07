from typing import Any, Dict, TextIO, Union

import requests

from prefixmaps.data import data_path
from prefixmaps.datamodel.context import Context


def from_linkml_url(url: str, name: str = None) -> Context:
    response = requests.get(url=url)
    if name is None:
        name = url
    return from_linkml_file(response.text, name)


def from_linkml_file(file: Union[TextIO, str], name: str = None) -> Context:
    import yaml

    if isinstance(file, str):
        with open(file) as stream:
            return from_linkml_file(stream, name)
    else:
        return from_linkml(yaml.safe_load(file), name)


def from_linkml(obj: Dict[str, Any], name: str = None) -> Context:
    if name is None:
        name = obj["name"]
    ctxt = Context(name)
    for k, v in obj["prefixes"].items():
        ctxt.add_prefix(k, v)
    return ctxt


def from_semweb_curated() -> Context:
    """
    Ingests the curated SemWeb context.

    In future this may migrate upstream.
    :return:
    """
    return from_linkml_file(str(data_path / "linked_data.curated.yaml"))
