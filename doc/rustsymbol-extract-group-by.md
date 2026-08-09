# extract_group_by (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- ← map_step (`47faefae-6222-5ae2-a4a9-9ec080997290`)
- → child_text (`f70791ea-bde4-57ab-8af1-ccc69fa9f5a7`)

### Contains

- ← ekos/crates/recovery/src/pentaho_analyzer.rs (`ce3d2f1b-e1c6-55d7-92bc-c1b69f76dbca`)

## Diagram

```mermaid
graph TD
    n056c9294b6935d0d84a71dca458e5118["extract_group_by"]
    nce3d2f1be1c655d792bcc1b69f76dbca["ekos/crates/recovery/src/pentaho_analyzer.rs"]
    nce3d2f1be1c655d792bcc1b69f76dbca -->|Contains| n056c9294b6935d0d84a71dca458e5118
    n47faefae62225ae2a4a99ec080997290["map_step"]
    n47faefae62225ae2a4a99ec080997290 -->|Calls| n056c9294b6935d0d84a71dca458e5118
    nf70791eabde457ab8af1ccc69fa9f5a7["child_text"]
    n056c9294b6935d0d84a71dca458e5118 -->|Calls| nf70791eabde457ab8af1ccc69fa9f5a7
```

## Evidence

_No evidence cited._
