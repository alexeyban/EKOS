from csv import DictWriter
from typing import TextIO

from prefixmaps.datamodel.context import Context, PrefixExpansion, StatusType

STATUS_TYPE_ORDER = {
    StatusType.canonical: 0,
    StatusType.prefix_alias: 1,
    StatusType.namespace_alias: 2,
    StatusType.multi_alias: 3,
}


def _key(pe: PrefixExpansion):
    return pe.prefix.casefold(), STATUS_TYPE_ORDER[pe.status]


def context_to_file(
    context: Context, file: TextIO, *, include_expansion_source: bool = False
) -> None:
    """
    Writes a context to a file

    :param context:
    :param file:
    :param include_expansion_source: If true, include a "source" column. This is useful for
        writing merged contexts since it says the highest priority simple context
        from which the row corresponding to a :class:`PrefixExpansion` came.
    :return:
    """
    field_names = ["context", "prefix", "namespace", "status"]
    if include_expansion_source:
        field_names.append("expansion_source")
    writer = DictWriter(file, fieldnames=field_names)
    writer.writeheader()
    for pe in sorted(context.prefix_expansions, key=_key):
        row = vars(pe)
        row["status"] = pe.status.value
        if not include_expansion_source:
            row.pop("expansion_source", None)
        writer.writerow(row)
