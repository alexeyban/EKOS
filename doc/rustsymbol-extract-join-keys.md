# extract_join_keys (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- ← extract_join (`450b2ac3-e3bb-5b8b-a2d1-ed9e281bc3b2`)
- → child_text (`f70791ea-bde4-57ab-8af1-ccc69fa9f5a7`)

### Contains

- ← ekos/crates/recovery/src/pentaho_analyzer.rs (`ce3d2f1b-e1c6-55d7-92bc-c1b69f76dbca`)

## Diagram

```mermaid
graph TD
    n73352edd35a859e4aff44f4ac9548170["extract_join_keys"]
    nce3d2f1be1c655d792bcc1b69f76dbca["ekos/crates/recovery/src/pentaho_analyzer.rs"]
    nce3d2f1be1c655d792bcc1b69f76dbca -->|Contains| n73352edd35a859e4aff44f4ac9548170
    n450b2ac3e3bb5b8ba2d1ed9e281bc3b2["extract_join"]
    n450b2ac3e3bb5b8ba2d1ed9e281bc3b2 -->|Calls| n73352edd35a859e4aff44f4ac9548170
    nf70791eabde457ab8af1ccc69fa9f5a7["child_text"]
    n73352edd35a859e4aff44f4ac9548170 -->|Calls| nf70791eabde457ab8af1ccc69fa9f5a7
```

## Evidence

_No evidence cited._
