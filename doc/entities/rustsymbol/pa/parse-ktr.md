# parse_ktr (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- ← parse_kettle_xml (`b954f76c-b28d-578b-a82b-cf5b8d11dec0`)
- → child_text (`f70791ea-bde4-57ab-8af1-ccc69fa9f5a7`)
- → map_step (`47faefae-6222-5ae2-a4a9-9ec080997290`)

### Contains

- ← ekos/crates/recovery/src/pentaho_analyzer.rs (`ce3d2f1b-e1c6-55d7-92bc-c1b69f76dbca`)

## Diagram

```mermaid
graph TD
    nac857c0ae7e15b8d972ddd8184852f15["parse_ktr"]
    nce3d2f1be1c655d792bcc1b69f76dbca["ekos/crates/recovery/src/pentaho_analyzer.rs"]
    nce3d2f1be1c655d792bcc1b69f76dbca -->|Contains| nac857c0ae7e15b8d972ddd8184852f15
    nb954f76cb28d578ba82bcf5b8d11dec0["parse_kettle_xml"]
    nb954f76cb28d578ba82bcf5b8d11dec0 -->|Calls| nac857c0ae7e15b8d972ddd8184852f15
    nf70791eabde457ab8af1ccc69fa9f5a7["child_text"]
    nac857c0ae7e15b8d972ddd8184852f15 -->|Calls| nf70791eabde457ab8af1ccc69fa9f5a7
    n47faefae62225ae2a4a99ec080997290["map_step"]
    nac857c0ae7e15b8d972ddd8184852f15 -->|Calls| n47faefae62225ae2a4a99ec080997290
```

## Evidence

_No evidence cited._
