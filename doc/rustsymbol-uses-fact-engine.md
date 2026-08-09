# uses_fact_engine (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- → facts_dir (`83873731-f148-5047-ad68-b92b7feef390`)
- ← open_store (`ce911a52-3055-56e0-bea7-c15a5ff1d773`)
- ← store_display (`713ed7cc-5c75-533e-89bc-0a3cd1e3b880`)

### Contains

- ← ekos/crates/cli/src/commands/store.rs (`ce2ed217-2b42-5760-9d2e-e2ca1574a517`)

## Diagram

```mermaid
graph TD
    n10d67e5ac7545199915e23349038a6f5["uses_fact_engine"]
    nce2ed2172b4257609d2ee2ca1574a517["ekos/crates/cli/src/commands/store.rs"]
    nce2ed2172b4257609d2ee2ca1574a517 -->|Contains| n10d67e5ac7545199915e23349038a6f5
    n83873731f1485047ad68b92b7feef390["facts_dir"]
    n10d67e5ac7545199915e23349038a6f5 -->|Calls| n83873731f1485047ad68b92b7feef390
    nce911a52305556e0bea7c15a5ff1d773["open_store"]
    nce911a52305556e0bea7c15a5ff1d773 -->|Calls| n10d67e5ac7545199915e23349038a6f5
    n713ed7cc5c75533e89bc0a3cd1e3b880["store_display"]
    n713ed7cc5c75533e89bc0a3cd1e3b880 -->|Calls| n10d67e5ac7545199915e23349038a6f5
```

## Evidence

_No evidence cited._
