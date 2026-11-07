from csv import DictReader
from pathlib import Path
from typing import List, TextIO, Union

import yaml
from curies import Converter

from prefixmaps.data import data_path
from prefixmaps.datamodel.context import CONTEXT, Context, PrefixExpansion, StatusType

__all__ = [
    "load_multi_context",
    "load_context",
    "load_converter",
]


def context_path(name: CONTEXT) -> Path:
    """
    Get the path in which the context datafile lives

    :param name:
    :return:
    """
    return data_path / f"{name}.csv"


def load_converter(names: Union[CONTEXT, List[CONTEXT]], refresh: bool = False) -> Converter:
    """Get a converter."""
    if isinstance(names, str):
        return load_context(names, refresh=refresh).as_converter()
    return load_multi_context(names, refresh=refresh).as_converter()


def load_multi_context(names: List[CONTEXT], refresh=False) -> Context:
    """
    Merges multiple contexts

    :param names:
    :param refresh: if True, fetch from upstream
    :return:
    """
    if len(names) == 1:
        return load_context(names[0], refresh=refresh)
    name = "+".join(names)
    ctxt = Context(name)
    for n in names:
        ctxt.combine(load_context(n, refresh=refresh))
    return ctxt


def load_context(name: CONTEXT, refresh=False) -> Context:
    """
    Loads a context by name from standard location

    :param name:
    :param refresh: if True, fetch from upstream
    :return:
    """
    if refresh:
        from prefixmaps.ingest.etl_runner import load_context_from_source

        return load_context_from_source(name)
    else:
        with open(data_path / f"{name}.csv", encoding="utf-8") as file:
            return context_from_file(name, file)


def context_from_file(name: CONTEXT, file: TextIO) -> Context:
    """
    Loads a context from a file

    :param name:
    :param file:
    :return:
    """
    reader = DictReader(file)
    context = Context(name=name)
    for row in reader:
        row["status"] = StatusType[row["status"]]
        pe = PrefixExpansion(**row)
        context.prefix_expansions.append(pe)
    return context


def load_contexts_meta() -> List[Context]:
    """
    Returns metadata for all contexts

    :param name:
    :return:
    """
    objs = yaml.safe_load(open(data_path / "contexts.curated.yaml"))
    ctxts = []
    for obj in objs:
        ctxt = Context(**obj)
        ctxts.append(ctxt)
    return ctxts
