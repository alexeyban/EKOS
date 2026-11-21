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
ITEM_GLOB = "*.item"
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
    for item in project.rglob(ITEM_GLOB):
        parts = item.relative_to(project).parts
        if not SKIPPED_DIRS.intersection(parts):
            kinds[parts[0]] += 1
    return dict(sorted(kinds.items()))


# Studio's line styles (`EConnectionType`), measured on fixture B and the joblet fixtures.
LINE_STYLES = {"1": "SUBJOB_OK", "3": "COMPONENT_OK", "4": "SUBJOB_ERROR", "5": "COMPONENT_ERROR",
               "6": "RUN_IF", "7": "ITERATE", "8": "LOOKUP"}
PORT_CONNECTORS = ("INPUT", "OUTPUT", "TRIGGER_INPUT", "TRIGGER_OUTPUT")


def link_kind(connection: ET.Element) -> str:
    name = connection.get("connectorName", "")
    style = connection.get("lineStyle")
    if name == "FLOW" and style == "8":
        return "LOOKUP"
    # A link out of a joblet use is named after the port; its line style says what it is.
    if name.rstrip("0123456789").rstrip("_") in PORT_CONNECTORS:
        return LINE_STYLES.get(style, "FLOW")
    return name


def process_root(path: Path) -> ET.Element | None:
    """The `ProcessType` / `JobletProcess` of an item; Studio wraps a joblet in `xmi:XMI`."""
    root = ET.parse(path).getroot()
    if root.tag.endswith("XMI"):
        return next((c for c in root if c.tag.endswith(("ProcessType", "JobletProcess"))), None)
    return root


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
    jobs = sorted((project / "process").rglob(ITEM_GLOB)) if (project / "process").is_dir() else []
    for job in jobs:
        job_census(job, totals)
    components = totals.pop("components")
    joblet_items = (
        sorted((project / "joblets").rglob(ITEM_GLOB)) if (project / "joblets").is_dir() else []
    )
    joblet_names = {j.name.rsplit("_", 1)[0] for j in joblet_items if process_root(j) is not None}
    joblet_uses = sum(n for t, n in components.items() if t in joblet_names)
    return {
        "items": item_kinds(project),
        "jobs": len(jobs),
        "joblets": len(joblet_items),
        "joblet_uses": joblet_uses,
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
