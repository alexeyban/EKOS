# parse (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- → split_once_any_dash (`48dcf9a6-293c-53ff-8cce-1ef20a74619e`)
- → extract_section (`9f6135bc-5557-5a5d-b240-e103151eb503`)

### Contains

- ← ekos/crates/marketing/src/devlog.rs (`4ca01e5b-d21d-5312-ac99-c6aa65d7d8d0`)

## Diagram

```mermaid
graph TD
    n620e68e7cc685327bcdb235f2a72f686["parse"]
    n4ca01e5bd21d5312ac99c6aa65d7d8d0["ekos/crates/marketing/src/devlog.rs"]
    n4ca01e5bd21d5312ac99c6aa65d7d8d0 -->|Contains| n620e68e7cc685327bcdb235f2a72f686
    n48dcf9a6293c53ff8cce1ef20a74619e["split_once_any_dash"]
    n620e68e7cc685327bcdb235f2a72f686 -->|Calls| n48dcf9a6293c53ff8cce1ef20a74619e
    n9f6135bc55575a5db240e103151eb503["extract_section"]
    n620e68e7cc685327bcdb235f2a72f686 -->|Calls| n9f6135bc55575a5db240e103151eb503
```

## Evidence

_No evidence cited._
