# migrate_to_v3 (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- → EntryType::as_str (`0beb5b76-38f4-53a2-9ee4-e1070cca9822`)
- → content_signature (`66c7da48-70e5-57fb-9882-5a5b05933963`)
- → Ledger::open (`1202f2b1-c8ed-5a89-aac3-5ef29891cb8b`)
- → Ledger::export_versions (`1ed3c4b0-eefc-5cee-8f3b-f559c0e5f97e`)
- → Ledger::object_count (`f92c42cd-96e2-5b55-8bc1-2184a7ea22d5`)
- → Ledger::relationship_count (`fd2750d9-0510-5e05-ac77-f3125db298a6`)
- → dir_bytes (`94d64c98-e745-574a-89b3-8734f99623eb`)

### Contains

- ← ekos/crates/ledger/src/lib.rs (`21938f45-b767-5933-9e71-12e15ff53eb1`)

## Diagram

```mermaid
graph TD
    n1dab3f65615b56e9ae9be92c32a2cb63["migrate_to_v3"]
    n21938f45b76759339e7112e15ff53eb1["ekos/crates/ledger/src/lib.rs"]
    n21938f45b76759339e7112e15ff53eb1 -->|Contains| n1dab3f65615b56e9ae9be92c32a2cb63
    n0beb5b7638f453a29ee4e1070cca9822["EntryType::as_str"]
    n1dab3f65615b56e9ae9be92c32a2cb63 -->|Calls| n0beb5b7638f453a29ee4e1070cca9822
    n66c7da4870e557fb98825a5b05933963["content_signature"]
    n1dab3f65615b56e9ae9be92c32a2cb63 -->|Calls| n66c7da4870e557fb98825a5b05933963
    n1202f2b1c8ed5a89aac35ef29891cb8b["Ledger::open"]
    n1dab3f65615b56e9ae9be92c32a2cb63 -->|Calls| n1202f2b1c8ed5a89aac35ef29891cb8b
    n1ed3c4b0eefc5cee8f3bf559c0e5f97e["Ledger::export_versions"]
    n1dab3f65615b56e9ae9be92c32a2cb63 -->|Calls| n1ed3c4b0eefc5cee8f3bf559c0e5f97e
    nf92c42cd96e25b558bc12184a7ea22d5["Ledger::object_count"]
    n1dab3f65615b56e9ae9be92c32a2cb63 -->|Calls| nf92c42cd96e25b558bc12184a7ea22d5
    nfd2750d905105e05ac77f3125db298a6["Ledger::relationship_count"]
    n1dab3f65615b56e9ae9be92c32a2cb63 -->|Calls| nfd2750d905105e05ac77f3125db298a6
    n94d64c98e745574a89b38734f99623eb["dir_bytes"]
    n1dab3f65615b56e9ae9be92c32a2cb63 -->|Calls| n94d64c98e745574a89b38734f99623eb
```

## Evidence

_No evidence cited._
