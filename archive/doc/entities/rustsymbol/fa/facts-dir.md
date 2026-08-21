# facts_dir (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- ← uses_fact_engine (`10d67e5a-c754-5199-915e-23349038a6f5`)
- ← open_store (`ce911a52-3055-56e0-bea7-c15a5ff1d773`)
- ← store_display (`713ed7cc-5c75-533e-89bc-0a3cd1e3b880`)

### Contains

- ← ekos/crates/cli/src/commands/store.rs (`ce2ed217-2b42-5760-9d2e-e2ca1574a517`)

## Diagram

```mermaid
graph TD
    n83873731f1485047ad68b92b7feef390["facts_dir"]
    nce2ed2172b4257609d2ee2ca1574a517["ekos/crates/cli/src/commands/store.rs"]
    nce2ed2172b4257609d2ee2ca1574a517 -->|Contains| n83873731f1485047ad68b92b7feef390
    n10d67e5ac7545199915e23349038a6f5["uses_fact_engine"]
    n10d67e5ac7545199915e23349038a6f5 -->|Calls| n83873731f1485047ad68b92b7feef390
    nce911a52305556e0bea7c15a5ff1d773["open_store"]
    nce911a52305556e0bea7c15a5ff1d773 -->|Calls| n83873731f1485047ad68b92b7feef390
    n713ed7cc5c75533e89bc0a3cd1e3b880["store_display"]
    n713ed7cc5c75533e89bc0a3cd1e3b880 -->|Calls| n83873731f1485047ad68b92b7feef390
```

## Evidence

_No evidence cited._
