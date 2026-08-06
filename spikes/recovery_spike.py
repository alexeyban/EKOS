#!/usr/bin/env python3
"""
Phase -1 Spike: Knowledge Recovery from SQL

Throwaway script. Feeds ecommerce.sql to Claude and measures how well the
LLM extracts business entities, relationships, and evidence fragments.

Results feed into RFC 0003 (KIR shape) and RFC 0008 (LLM policy).

Usage:
    ANTHROPIC_API_KEY=<key> python3 docs/spikes/recovery_spike.py

Output:
    - Extracted entities as JSON
    - Extracted FK relationships as JSON
    - Precision/recall against the golden labels at the bottom of this file
    - A write-up section to copy into docs/spikes/recovery-spike.md
"""

import json
import os
import sys

SQL_FILE = "tests/fixtures/ecommerce.sql"

PROMPT_TEMPLATE = """\
You are an expert data architect analyzing a SQL schema.

Given the following SQL DDL, extract all business entities and their relationships.

For each entity, provide:
- name: The business concept name (not necessarily the table name)
- table: The actual table name
- description: One sentence describing the business concept

For each relationship, provide:
- from: Source entity name
- to: Target entity name
- kind: One of [ForeignKey, DependsOn, Contains, OwnedBy]
- via: The FK column(s) or join description
- evidence: The exact SQL fragment that supports this relationship

Respond ONLY with a JSON object in this exact shape:
{{
  "entities": [
    {{"name": "...", "table": "...", "description": "..."}}
  ],
  "relationships": [
    {{"from": "...", "to": "...", "kind": "...", "via": "...", "evidence": "..."}}
  ]
}}

SQL Schema:
{sql}
"""

# Golden labels for precision/recall evaluation
GOLDEN_ENTITIES = {
    "Customer", "Product", "Category", "Order", "OrderItem", "Payment"
}

GOLDEN_RELATIONSHIPS = {
    ("Order", "Customer"),
    ("OrderItem", "Order"),
    ("OrderItem", "Product"),
    ("Product", "Category"),
    ("Category", "Category"),
    ("Payment", "Order"),
}


def call_claude(sql: str) -> dict:
    try:
        import anthropic
    except ImportError:
        sys.exit("Install anthropic: pip install anthropic")

    client = anthropic.Anthropic(api_key=os.environ["ANTHROPIC_API_KEY"])
    prompt = PROMPT_TEMPLATE.format(sql=sql)

    message = client.messages.create(
        model="claude-sonnet-4-6",
        max_tokens=2048,
        messages=[{"role": "user", "content": prompt}],
    )

    text = message.content[0].text.strip()
    # Strip markdown code fences if present
    if text.startswith("```"):
        text = text.split("```")[1]
        if text.startswith("json"):
            text = text[4:]
    return json.loads(text)


def evaluate(result: dict):
    extracted_entities = {e["name"] for e in result.get("entities", [])}
    extracted_rels = {
        (r["from"], r["to"]) for r in result.get("relationships", [])
    }

    # Entity metrics
    tp_e = len(extracted_entities & GOLDEN_ENTITIES)
    fp_e = len(extracted_entities - GOLDEN_ENTITIES)
    fn_e = len(GOLDEN_ENTITIES - extracted_entities)
    prec_e = tp_e / (tp_e + fp_e) if (tp_e + fp_e) > 0 else 0
    rec_e  = tp_e / (tp_e + fn_e) if (tp_e + fn_e) > 0 else 0

    # Relationship metrics (order-insensitive pair)
    norm_golden = GOLDEN_RELATIONSHIPS | {(b, a) for a, b in GOLDEN_RELATIONSHIPS}
    tp_r = sum(1 for r in extracted_rels if r in norm_golden or (r[1], r[0]) in norm_golden)
    fp_r = len(extracted_rels) - tp_r
    fn_r = len(GOLDEN_RELATIONSHIPS) - tp_r
    prec_r = tp_r / (tp_r + fp_r) if (tp_r + fp_r) > 0 else 0
    rec_r  = tp_r / (tp_r + fn_r) if (tp_r + fn_r) > 0 else 0

    return {
        "entities": {"precision": prec_e, "recall": rec_e, "tp": tp_e, "fp": fp_e, "fn": fn_e},
        "relationships": {"precision": prec_r, "recall": rec_r, "tp": tp_r, "fp": fp_r, "fn": fn_r},
        "extracted_entities": sorted(extracted_entities),
        "missing_entities": sorted(GOLDEN_ENTITIES - extracted_entities),
        "extra_entities": sorted(extracted_entities - GOLDEN_ENTITIES),
    }


def main():
    if "ANTHROPIC_API_KEY" not in os.environ:
        sys.exit("Set ANTHROPIC_API_KEY before running this spike.")

    sql = open(SQL_FILE).read()
    print("Calling Claude…")
    result = call_claude(sql)

    print("\n── Extracted entities ──")
    for e in result.get("entities", []):
        print(f"  {e['name']} ({e['table']}): {e['description']}")

    print("\n── Extracted relationships ──")
    for r in result.get("relationships", []):
        print(f"  {r['from']} → {r['to']} [{r['kind']}] via {r['via']}")
        print(f"    evidence: {r['evidence'][:80]}")

    metrics = evaluate(result)
    print("\n── Evaluation ──")
    print(f"  Entities:      precision={metrics['entities']['precision']:.2f}  recall={metrics['entities']['recall']:.2f}")
    print(f"  Relationships: precision={metrics['relationships']['precision']:.2f}  recall={metrics['relationships']['recall']:.2f}")
    if metrics["missing_entities"]:
        print(f"  Missing: {metrics['missing_entities']}")
    if metrics["extra_entities"]:
        print(f"  Extra:   {metrics['extra_entities']}")

    print("\n── Full JSON result ──")
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()
