# open_store (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- → facts_dir (`83873731-f148-5047-ad68-b92b7feef390`)
- → uses_fact_engine (`10d67e5a-c754-5199-915e-23349038a6f5`)

### Contains

- ← ekos/crates/cli/src/commands/store.rs (`ce2ed217-2b42-5760-9d2e-e2ca1574a517`)

## Diagram

```mermaid
graph TD
    nce911a52305556e0bea7c15a5ff1d773["open_store"]
    nce2ed2172b4257609d2ee2ca1574a517["ekos/crates/cli/src/commands/store.rs"]
    nce2ed2172b4257609d2ee2ca1574a517 -->|Contains| nce911a52305556e0bea7c15a5ff1d773
    n83873731f1485047ad68b92b7feef390["facts_dir"]
    nce911a52305556e0bea7c15a5ff1d773 -->|Calls| n83873731f1485047ad68b92b7feef390
    n10d67e5ac7545199915e23349038a6f5["uses_fact_engine"]
    nce911a52305556e0bea7c15a5ff1d773 -->|Calls| n10d67e5ac7545199915e23349038a6f5
```

## Evidence

_No evidence cited._
