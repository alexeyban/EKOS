# extract_join (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- ← map_step (`47faefae-6222-5ae2-a4a9-9ec080997290`)
- → child_text (`f70791ea-bde4-57ab-8af1-ccc69fa9f5a7`)
- → extract_join_keys (`73352edd-35a8-59e4-aff4-4f4ac9548170`)

### Contains

- ← ekos/crates/recovery/src/pentaho_analyzer.rs (`ce3d2f1b-e1c6-55d7-92bc-c1b69f76dbca`)

## Diagram

```mermaid
graph TD
    n450b2ac3e3bb5b8ba2d1ed9e281bc3b2["extract_join"]
    nce3d2f1be1c655d792bcc1b69f76dbca["ekos/crates/recovery/src/pentaho_analyzer.rs"]
    nce3d2f1be1c655d792bcc1b69f76dbca -->|Contains| n450b2ac3e3bb5b8ba2d1ed9e281bc3b2
    n47faefae62225ae2a4a99ec080997290["map_step"]
    n47faefae62225ae2a4a99ec080997290 -->|Calls| n450b2ac3e3bb5b8ba2d1ed9e281bc3b2
    nf70791eabde457ab8af1ccc69fa9f5a7["child_text"]
    n450b2ac3e3bb5b8ba2d1ed9e281bc3b2 -->|Calls| nf70791eabde457ab8af1ccc69fa9f5a7
    n73352edd35a859e4aff44f4ac9548170["extract_join_keys"]
    n450b2ac3e3bb5b8ba2d1ed9e281bc3b2 -->|Calls| n73352edd35a859e4aff44f4ac9548170
```

## Evidence

_No evidence cited._
