# content_signature (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- → EntryType::as_str (`0beb5b76-38f4-53a2-9ee4-e1070cca9822`)
- ← Ledger::append_versioned (`fd02b8da-192d-585b-a46d-996b4095186c`)
- ← merge_branch (`16be84c8-16f2-5d63-8dff-104f7296fc29`)
- ← merge_stores (`35e9663b-3b6d-50ec-ad16-9721c45eb3d1`)
- ← migrate_to_v3 (`1dab3f65-615b-56e9-ae9b-e92c32a2cb63`)

### Contains

- ← ekos/crates/ledger/src/lib.rs (`21938f45-b767-5933-9e71-12e15ff53eb1`)

## Diagram

```mermaid
graph TD
    n66c7da4870e557fb98825a5b05933963["content_signature"]
    n21938f45b76759339e7112e15ff53eb1["ekos/crates/ledger/src/lib.rs"]
    n21938f45b76759339e7112e15ff53eb1 -->|Contains| n66c7da4870e557fb98825a5b05933963
    n0beb5b7638f453a29ee4e1070cca9822["EntryType::as_str"]
    n66c7da4870e557fb98825a5b05933963 -->|Calls| n0beb5b7638f453a29ee4e1070cca9822
    nfd02b8da192d585ba46d996b4095186c["Ledger::append_versioned"]
    nfd02b8da192d585ba46d996b4095186c -->|Calls| n66c7da4870e557fb98825a5b05933963
    n16be84c816f25d638dff104f7296fc29["merge_branch"]
    n16be84c816f25d638dff104f7296fc29 -->|Calls| n66c7da4870e557fb98825a5b05933963
    n35e9663b3b6d50ecad169721c45eb3d1["merge_stores"]
    n35e9663b3b6d50ecad169721c45eb3d1 -->|Calls| n66c7da4870e557fb98825a5b05933963
    n1dab3f65615b56e9ae9be92c32a2cb63["migrate_to_v3"]
    n1dab3f65615b56e9ae9be92c32a2cb63 -->|Calls| n66c7da4870e557fb98825a5b05933963
```

## Evidence

_No evidence cited._
