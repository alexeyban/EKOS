# Ledger::relationship_count (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | method |

## Relationships

### Calls

- ← migrate_to_v2 (`fee5c44c-a2e1-59db-bf5d-b63aff20f8c9`)
- ← diff_ledger (`efce5b16-7270-58fe-b278-442b178d7df3`)
- ← migrate_to_v3 (`1dab3f65-615b-56e9-ae9b-e92c32a2cb63`)

### Contains

- ← ekos/crates/ledger/src/lib.rs (`21938f45-b767-5933-9e71-12e15ff53eb1`)

## Diagram

```mermaid
graph TD
    nfd2750d905105e05ac77f3125db298a6["Ledger::relationship_count"]
    n21938f45b76759339e7112e15ff53eb1["ekos/crates/ledger/src/lib.rs"]
    n21938f45b76759339e7112e15ff53eb1 -->|Contains| nfd2750d905105e05ac77f3125db298a6
    nfee5c44ca2e159dbbf5db63aff20f8c9["migrate_to_v2"]
    nfee5c44ca2e159dbbf5db63aff20f8c9 -->|Calls| nfd2750d905105e05ac77f3125db298a6
    nefce5b16727058feb278442b178d7df3["diff_ledger"]
    nefce5b16727058feb278442b178d7df3 -->|Calls| nfd2750d905105e05ac77f3125db298a6
    n1dab3f65615b56e9ae9be92c32a2cb63["migrate_to_v3"]
    n1dab3f65615b56e9ae9be92c32a2cb63 -->|Calls| nfd2750d905105e05ac77f3125db298a6
```

## Evidence

_No evidence cited._
