# parse_kjb (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- ← parse_kettle_xml (`b954f76c-b28d-578b-a82b-cf5b8d11dec0`)
- → xml_slice (`0c5e1b6a-7829-5a6f-a831-e20b9eeb1c27`)

### Contains

- ← ekos/crates/recovery/src/pentaho_analyzer.rs (`ce3d2f1b-e1c6-55d7-92bc-c1b69f76dbca`)

## Diagram

```mermaid
graph TD
    ncc76104d128b5cc0bcc89f5a4f895888["parse_kjb"]
    nce3d2f1be1c655d792bcc1b69f76dbca["ekos/crates/recovery/src/pentaho_analyzer.rs"]
    nce3d2f1be1c655d792bcc1b69f76dbca -->|Contains| ncc76104d128b5cc0bcc89f5a4f895888
    nb954f76cb28d578ba82bcf5b8d11dec0["parse_kettle_xml"]
    nb954f76cb28d578ba82bcf5b8d11dec0 -->|Calls| ncc76104d128b5cc0bcc89f5a4f895888
    n0c5e1b6a78295a6fa831e20b9eeb1c27["xml_slice"]
    ncc76104d128b5cc0bcc89f5a4f895888 -->|Calls| n0c5e1b6a78295a6fa831e20b9eeb1c27
```

## Evidence

_No evidence cited._
