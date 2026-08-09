# extract_stream_lookup (RustSymbol)

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
    n2c163fbc39ec5e8382a550dcc8f51cfb["extract_stream_lookup"]
    nce3d2f1be1c655d792bcc1b69f76dbca["ekos/crates/recovery/src/pentaho_analyzer.rs"]
    nce3d2f1be1c655d792bcc1b69f76dbca -->|Contains| n2c163fbc39ec5e8382a550dcc8f51cfb
    n47faefae62225ae2a4a99ec080997290["map_step"]
    n47faefae62225ae2a4a99ec080997290 -->|Calls| n2c163fbc39ec5e8382a550dcc8f51cfb
    nf70791eabde457ab8af1ccc69fa9f5a7["child_text"]
    n2c163fbc39ec5e8382a550dcc8f51cfb -->|Calls| nf70791eabde457ab8af1ccc69fa9f5a7
```

## Evidence

_No evidence cited._
