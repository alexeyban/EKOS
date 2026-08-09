# Ledger::append_versioned (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | method |

## Relationships

### Calls

- → Ledger::sig_param (`7fdb2fc5-6ebf-53bb-b836-1757fcdea43d`)
- → content_signature (`66c7da48-70e5-57fb-9882-5a5b05933963`)
- → Ledger::id_param (`f3cddd93-ac14-5ec3-bb44-011407d55f49`)
- ← Ledger::append (`7b3089e3-ed30-5f52-bc3c-a296097d3b8f`)
- ← Ledger::append_object (`b71bb7ad-337a-518f-9b6e-316178f45928`)
- ← Ledger::append_relationship (`7cfea349-6f7b-5501-8b20-1291291d672b`)

### Contains

- ← ekos/crates/ledger/src/lib.rs (`21938f45-b767-5933-9e71-12e15ff53eb1`)

## Diagram

```mermaid
graph TD
    nfd02b8da192d585ba46d996b4095186c["Ledger::append_versioned"]
    n21938f45b76759339e7112e15ff53eb1["ekos/crates/ledger/src/lib.rs"]
    n21938f45b76759339e7112e15ff53eb1 -->|Contains| nfd02b8da192d585ba46d996b4095186c
    n7fdb2fc56ebf53bbb8361757fcdea43d["Ledger::sig_param"]
    nfd02b8da192d585ba46d996b4095186c -->|Calls| n7fdb2fc56ebf53bbb8361757fcdea43d
    n66c7da4870e557fb98825a5b05933963["content_signature"]
    nfd02b8da192d585ba46d996b4095186c -->|Calls| n66c7da4870e557fb98825a5b05933963
    nf3cddd93ac145ec3bb44011407d55f49["Ledger::id_param"]
    nfd02b8da192d585ba46d996b4095186c -->|Calls| nf3cddd93ac145ec3bb44011407d55f49
    n7b3089e3ed305f52bc3ca296097d3b8f["Ledger::append"]
    n7b3089e3ed305f52bc3ca296097d3b8f -->|Calls| nfd02b8da192d585ba46d996b4095186c
    nb71bb7ad337a518f9b6e316178f45928["Ledger::append_object"]
    nb71bb7ad337a518f9b6e316178f45928 -->|Calls| nfd02b8da192d585ba46d996b4095186c
    n7cfea3496f7b55018b201291291d672b["Ledger::append_relationship"]
    n7cfea3496f7b55018b201291291d672b -->|Calls| nfd02b8da192d585ba46d996b4095186c
```

## Evidence

_No evidence cited._
