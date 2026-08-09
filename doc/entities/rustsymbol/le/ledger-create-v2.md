# Ledger::create_v2 (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | method |

## Relationships

### Calls

- → Codec::zstd (`90b5c6e9-ab20-5f91-af23-dfa3538059e2`)
- → init_schema_v2 (`229e4c1f-d398-5b84-8348-2003c40d9865`)
- ← migrate_to_v2 (`fee5c44c-a2e1-59db-bf5d-b63aff20f8c9`)

### Contains

- ← ekos/crates/ledger/src/lib.rs (`21938f45-b767-5933-9e71-12e15ff53eb1`)

## Diagram

```mermaid
graph TD
    ne0a15224826758c69f12b6f33a379ceb["Ledger::create_v2"]
    n21938f45b76759339e7112e15ff53eb1["ekos/crates/ledger/src/lib.rs"]
    n21938f45b76759339e7112e15ff53eb1 -->|Contains| ne0a15224826758c69f12b6f33a379ceb
    n90b5c6e9ab205f91af23dfa3538059e2["Codec::zstd"]
    ne0a15224826758c69f12b6f33a379ceb -->|Calls| n90b5c6e9ab205f91af23dfa3538059e2
    n229e4c1fd3985b8483482003c40d9865["init_schema_v2"]
    ne0a15224826758c69f12b6f33a379ceb -->|Calls| n229e4c1fd3985b8483482003c40d9865
    nfee5c44ca2e159dbbf5db63aff20f8c9["migrate_to_v2"]
    nfee5c44ca2e159dbbf5db63aff20f8c9 -->|Calls| ne0a15224826758c69f12b6f33a379ceb
```

## Evidence

_No evidence cited._
