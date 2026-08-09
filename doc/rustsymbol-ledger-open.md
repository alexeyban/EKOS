# Ledger::open (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | method |

## Relationships

### Calls

- → Ledger::migrate_fts_v2 (`2c3a50d1-1ba0-54fd-8509-2493f809dc4c`)
- → Codec::zstd (`90b5c6e9-ab20-5f91-af23-dfa3538059e2`)
- → init_schema_v2 (`229e4c1f-d398-5b84-8348-2003c40d9865`)
- → load_dictionary (`e3ae936e-e904-52b3-8b3e-900dd3178368`)
- ← migrate_to_v2 (`fee5c44c-a2e1-59db-bf5d-b63aff20f8c9`)
- ← migrate_to_v3 (`1dab3f65-615b-56e9-ae9b-e92c32a2cb63`)

### Contains

- ← ekos/crates/ledger/src/lib.rs (`21938f45-b767-5933-9e71-12e15ff53eb1`)

## Diagram

```mermaid
graph TD
    n1202f2b1c8ed5a89aac35ef29891cb8b["Ledger::open"]
    n21938f45b76759339e7112e15ff53eb1["ekos/crates/ledger/src/lib.rs"]
    n21938f45b76759339e7112e15ff53eb1 -->|Contains| n1202f2b1c8ed5a89aac35ef29891cb8b
    n2c3a50d11ba054fd85092493f809dc4c["Ledger::migrate_fts_v2"]
    n1202f2b1c8ed5a89aac35ef29891cb8b -->|Calls| n2c3a50d11ba054fd85092493f809dc4c
    n90b5c6e9ab205f91af23dfa3538059e2["Codec::zstd"]
    n1202f2b1c8ed5a89aac35ef29891cb8b -->|Calls| n90b5c6e9ab205f91af23dfa3538059e2
    n229e4c1fd3985b8483482003c40d9865["init_schema_v2"]
    n1202f2b1c8ed5a89aac35ef29891cb8b -->|Calls| n229e4c1fd3985b8483482003c40d9865
    ne3ae936ee90452b38b3e900dd3178368["load_dictionary"]
    n1202f2b1c8ed5a89aac35ef29891cb8b -->|Calls| ne3ae936ee90452b38b3e900dd3178368
    nfee5c44ca2e159dbbf5db63aff20f8c9["migrate_to_v2"]
    nfee5c44ca2e159dbbf5db63aff20f8c9 -->|Calls| n1202f2b1c8ed5a89aac35ef29891cb8b
    n1dab3f65615b56e9ae9be92c32a2cb63["migrate_to_v3"]
    n1dab3f65615b56e9ae9be92c32a2cb63 -->|Calls| n1202f2b1c8ed5a89aac35ef29891cb8b
```

## Evidence

_No evidence cited._
