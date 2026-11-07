"""Steps of `scripts/ossie-validate.sh` that read or edit Ossie documents (RFC 0245).

Run with the validator's environment (PyYAML available). Every check exits non-zero with a
message when it fails; nothing here talks to EKOS or the network.

    ossie_e2e.py break <in.yaml> <out.yaml>          a copy the official validator must reject
    ossie_e2e.py check-first <export.json>           the schema-only export
    ossie_e2e.py check-ingest <export.json>          the export after the Ossie model was ingested
    ossie_e2e.py review <export.json> <edited.json>  a reviewer's edits to the EKOS extensions
    ossie_e2e.py check-review <export.json>          the confirmed export after the import
    ossie_e2e.py plant <model.yaml>                  two disagreements with the schema, in place
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

import yaml


def fail(msg: str) -> None:
    sys.exit(f"FAIL: {msg}")


def load(path: str) -> dict:
    text = Path(path).read_text()
    return json.loads(text) if path.endswith(".json") else yaml.safe_load(text)


def dataset(m: dict, name: str) -> dict:
    for d in m.get("datasets", []):
        if d["name"] == name:
            return d
    fail(f"no dataset {name!r}")
    raise AssertionError


def field(d: dict, name: str) -> dict:
    for f in d.get("fields", []):
        if f["name"] == name:
            return f
    fail(f"no field {name!r} in {d['name']!r}")
    raise AssertionError


def ekos(element: dict) -> dict:
    for e in element.get("custom_extensions", []):
        if e["vendor_name"] == "EKOS":
            return json.loads(e["data"])
    fail(f"{element.get('name')!r} has no EKOS extension")
    raise AssertionError


def set_ekos(element: dict, data: dict) -> None:
    for e in element["custom_extensions"]:
        if e["vendor_name"] == "EKOS":
            e["data"] = json.dumps(data)


def synonyms(element: dict) -> list[str]:
    ctx = element.get("ai_context")
    return ctx.get("synonyms", []) if isinstance(ctx, dict) else []


def cmd_break(src: str, out: str) -> None:
    m = load(src)
    m.setdefault("relationships", []).append(
        {
            "name": "planted_bad_relationship",
            "from": m["datasets"][0]["name"],
            "to": "no_such_dataset",
            "from_columns": ["x"],
            "to_columns": ["y", "z"],
        }
    )
    Path(out).write_text(yaml.safe_dump(m, sort_keys=False))


def cmd_check_first(path: str) -> None:
    m = load(path)
    names = sorted(d["name"] for d in m["datasets"])
    want = ["customer", "date_dim", "item", "preferred_customers", "store", "store_sales"]
    if names != want:
        fail(f"datasets {names}, expected {want}")
    sales = dataset(m, "store_sales")
    if sales.get("primary_key") != ["ss_item_sk", "ss_ticket_number"]:
        fail(f"store_sales primary key {sales.get('primary_key')}")
    if sales.get("metrics"):
        fail("a metric appeared before any Ossie model was ingested: EKOS must never invent one")
    if len(m.get("relationships", [])) != 4:
        fail(f"{len(m.get('relationships', []))} relationships, expected the 4 foreign keys")


def cmd_check_ingest(path: str) -> None:
    m = load(path)
    sales = dataset(m, "store_sales")
    metrics = [x["name"] for x in sales.get("metrics", [])]
    if metrics != ["total_quantity"]:
        fail(f"store_sales metrics {metrics}, expected the model's total_quantity")
    if ekos(sales["metrics"][0]).get("origin") != "ossie:semantic/tpcds.ossie.yaml":
        fail("the carried metric does not name the Ossie file it came from")
    if "POS data" not in synonyms(sales):
        fail(f"store_sales synonyms {synonyms(sales)} lack the model's 'POS data'")
    if "shoppers" not in synonyms(dataset(m, "customer")):
        fail("customer synonyms not carried")
    if dataset(m, "customer").get("metrics"):
        fail("customer has a computed field in the model: nothing may be carried for it")
    if m.get("metrics"):
        fail("model-scoped metrics name the file's datasets and must not be exported")


def cmd_review(src: str, out: str) -> None:
    m = load(src)
    customer = dataset(m, "customer")
    flag = field(customer, "c_preferred_cust_flag")
    data = ekos(flag)
    for e in data["enum_meanings"]:
        if e["value"] == "'Y'":
            e["status"] = "confirmed"
        elif e["value"] == "'N'":
            e["label"] = "regular customer"
    set_ekos(flag, data)
    data = ekos(customer)
    data["concepts"][0]["status"] = "rejected"
    data["concepts"][0]["note"] = "a filter, not a business concept"
    set_ekos(customer, data)
    Path(out).write_text(json.dumps(m, indent=2))


def cmd_check_review(path: str) -> None:
    m = load(path)
    customer = dataset(m, "customer")
    got = field(customer, "c_preferred_cust_flag").get("ai_context", {}).get("instructions")
    want = "Codes: 'N' = regular customer; 'Y' = preferred customer."
    if got != want:
        fail(f"confirmed codes {got!r}, expected {want!r}")
    ctx = customer.get("ai_context")
    if isinstance(ctx, dict) and "PreferredCustomers" in (ctx.get("instructions") or ""):
        fail("the rejected concept is still in the confirmed model")


def cmd_plant(path: str) -> None:
    m = load(path)
    store = dataset(m, "store")
    store["fields"].append(
        {"name": "s_manager", "expression": {"sql": "s_manager"}, "datatype": "String"}
    )
    field(store, "s_city")["datatype"] = "Integer"
    Path(path).write_text(yaml.safe_dump(m, sort_keys=False))


COMMANDS = {
    "break": cmd_break,
    "check-first": cmd_check_first,
    "check-ingest": cmd_check_ingest,
    "review": cmd_review,
    "check-review": cmd_check_review,
    "plant": cmd_plant,
}

if __name__ == "__main__":
    if len(sys.argv) < 2 or sys.argv[1] not in COMMANDS:
        sys.exit(__doc__)
    COMMANDS[sys.argv[1]](*sys.argv[2:])
