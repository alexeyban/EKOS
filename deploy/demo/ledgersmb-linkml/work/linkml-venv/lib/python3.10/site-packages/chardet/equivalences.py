"""Backward-compatibility shim for ``chardet.equivalences``.

This module's contents were split in chardet 7.5: accuracy-evaluation tables
and predicates moved to :mod:`chardet.evaluation`, and public-API encoding-name
remapping moved to :mod:`chardet.output_names`.  This module re-exports both
sets of names so existing callers keep working.

.. deprecated:: 7.5
    Import from :mod:`chardet.evaluation` (``is_correct``,
    ``is_equivalent_detection``, ``is_language_equivalent``, ``SUPERSETS``,
    ``BIDIRECTIONAL_GROUPS``, ``LANGUAGE_EQUIVALENCES``) or
    :mod:`chardet.output_names` (``apply_compat_names``,
    ``apply_preferred_superset``, ``PREFERRED_SUPERSET``) instead.
    Will be removed in chardet 8.0.
"""

from __future__ import annotations

import warnings

from chardet.evaluation import (
    BIDIRECTIONAL_GROUPS,
    LANGUAGE_EQUIVALENCES,
    SUPERSETS,
    is_correct,
    is_equivalent_detection,
    is_language_equivalent,
)
from chardet.output_names import (
    _COMPAT_NAMES,
    PREFERRED_SUPERSET,
    apply_compat_names,
    apply_legacy_rename,
    apply_preferred_superset,
)

warnings.warn(
    "chardet.equivalences is deprecated; import from chardet.evaluation "
    "(is_correct, is_equivalent_detection, is_language_equivalent, SUPERSETS, "
    "BIDIRECTIONAL_GROUPS, LANGUAGE_EQUIVALENCES) or chardet.output_names "
    "(apply_compat_names, apply_preferred_superset, PREFERRED_SUPERSET) "
    "instead. Will be removed in chardet 8.0.",
    DeprecationWarning,
    stacklevel=2,
)

__all__ = [
    "BIDIRECTIONAL_GROUPS",
    "LANGUAGE_EQUIVALENCES",
    "PREFERRED_SUPERSET",
    "SUPERSETS",
    "apply_compat_names",
    "apply_legacy_rename",
    "apply_preferred_superset",
    "is_correct",
    "is_equivalent_detection",
    "is_language_equivalent",
]

# _COMPAT_NAMES is intentionally re-exported (with leading underscore) for
# external test suites that may already depend on it.
_ = _COMPAT_NAMES
