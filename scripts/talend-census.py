#!/usr/bin/env python3
"""RFC 0262 Part 0: census of the Talend fixture projects.

Reads the projects `scripts/talend-fixtures.sh` fetched into `.local/talend-fixtures/` and writes
`evals/talend/census.json`: counts only, never content (the projects are third-party). The census
is the denominator for every Talend coverage figure and the oracle the reader's gated tests compare
against, so it is computed here with Python's own XML parser, independently of the Rust reader.

Usage: `python3 scripts/talend-census.py` (no arguments; paths are fixed).
"""

from __future__ import annotations

import collections
import json
import xml.etree.ElementTree as ET
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
FIXTURES = ROOT / ".local" / "talend-fixtures"
OUT = ROOT / "evals" / "talend" / "census.json"
SKIPPED_DIRS = {"poms", "temp", "target", ".git"}
TRIGGERS = {"COMPONENT_OK", "COMPONENT_ERROR", "SUBJOB_OK", "SUBJOB_ERROR", "RUN_IF"}


def projects(fixture: Path) -> list[Path]:
    """Every directory holding a `talend.project`, outside generated and vendor folders."""
    found = []
    for marker in sorted(fixture.rglob("talend.project")):
        if not SKIPPED_DIRS.intersection(marker.relative_to(fixture).parts):
            found.append(marker.parent)
    return found


def item_kinds(project: Path) -> dict[str, int]:
    kinds: collections.Counter[str] = collections.Counter()
    for item in project.rglob("*.item"):
        parts = item.relative_to(project).parts
        if not SKIPPED_DIRS.intersection(parts):
            kinds[parts[0]] += 1
    return dict(sorted(kinds.items()))


def link_kind(connection: ET.Element) -> str:
    name = connection.get("connectorName", "")
    if name == "FLOW" and connection.get("lineStyle") == "8":
        return "LOOKUP"
    return name


def job_census(job: Path, totals: dict) -> None:
    tree = ET.parse(job).getroot()
    for node in tree.findall("node"):
        totals["components"][node.get("componentName")] += 1
        for param in node.findall("elementParameter"):
            if param.get("name") == "DATA_ACTION":
                totals["data_actions"][param.get("value")] += 1
        mapper = node.find("nodeData")
        if mapper is not None:
            totals["tmap_inputs"] += len(mapper.findall("inputTables"))
            totals["tmap_outputs"] += len(mapper.findall("outputTables"))
            totals["tmap_expressions"] += sum(
                1
                for e in mapper.iter("mapperTableEntries")
                if (e.get("expression") or "").strip()
            )
    for connection in tree.findall("connection"):
        kind = link_kind(connection)
        bucket = "triggers" if kind in TRIGGERS else "row_links"
        totals[bucket][kind] += 1


def project_census(project: Path) -> dict:
    totals: dict = {
        "components": collections.Counter(),
        "row_links": collections.Counter(),
        "triggers": collections.Counter(),
        "data_actions": collections.Counter(),
        "tmap_inputs": 0,
        "tmap_outputs": 0,
        "tmap_expressions": 0,
    }
    jobs = sorted((project / "process").rglob("*.item")) if (project / "process").is_dir() else []
    for job in jobs:
        job_census(job, totals)
    components = totals.pop("components")
    return {
        "items": item_kinds(project),
        "jobs": len(jobs),
        "components": sum(components.values()),
        "component_types": dict(sorted(components.items())),
        "links": sum(totals["row_links"].values()) + sum(totals["triggers"].values()),
        "row_links": dict(sorted(totals.pop("row_links").items())),
        "triggers": dict(sorted(totals.pop("triggers").items())),
        "data_actions": dict(sorted(totals.pop("data_actions").items())),
        **totals,
    }


def commit(fixture: Path) -> str:
    """The checked-out commit: the fixtures are detached checkouts, so `.git/HEAD` is the hash."""
    return (fixture / ".git" / "HEAD").read_text().strip()


def main() -> int:
    census = {}
    for fixture in sorted(p for p in FIXTURES.iterdir() if p.is_dir()):
        census[fixture.name] = {
            "commit": commit(fixture),
            "projects": {
                str(p.relative_to(fixture)): project_census(p) for p in projects(fixture)
            },
        }
    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_text(json.dumps(census, indent=2) + "\n")
    print(f"wrote {OUT.relative_to(ROOT)}: {', '.join(census)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
