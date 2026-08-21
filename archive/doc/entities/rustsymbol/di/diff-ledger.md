# diff_ledger (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- → Ledger::object_count (`f92c42cd-96e2-5b55-8bc1-2184a7ea22d5`)
- → Ledger::relationship_count (`fd2750d9-0510-5e05-ac77-f3125db298a6`)
- → Ledger::versions_in_window (`972c0223-2c64-54bc-b774-890fc6b61ab1`)
- ← Ledger::diff_impl (`ee9781b6-a015-5509-9d34-9aab6595bac6`)

### Contains

- ← ekos/crates/ledger/src/lib.rs (`21938f45-b767-5933-9e71-12e15ff53eb1`)

## Diagram

```mermaid
graph TD
    nefce5b16727058feb278442b178d7df3["diff_ledger"]
    n21938f45b76759339e7112e15ff53eb1["ekos/crates/ledger/src/lib.rs"]
    n21938f45b76759339e7112e15ff53eb1 -->|Contains| nefce5b16727058feb278442b178d7df3
    nf92c42cd96e25b558bc12184a7ea22d5["Ledger::object_count"]
    nefce5b16727058feb278442b178d7df3 -->|Calls| nf92c42cd96e25b558bc12184a7ea22d5
    nfd2750d905105e05ac77f3125db298a6["Ledger::relationship_count"]
    nefce5b16727058feb278442b178d7df3 -->|Calls| nfd2750d905105e05ac77f3125db298a6
    n972c02232c6454bcb774890fc6b61ab1["Ledger::versions_in_window"]
    nefce5b16727058feb278442b178d7df3 -->|Calls| n972c02232c6454bcb774890fc6b61ab1
    nee9781b6a01555099d349aab6595bac6["Ledger::diff_impl"]
    nee9781b6a01555099d349aab6595bac6 -->|Calls| nefce5b16727058feb278442b178d7df3
```

## Evidence

_No evidence cited._
