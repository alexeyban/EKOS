# merge_stores (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- → Ledger::all_objects (`d640b0e7-cfd1-5693-8c96-022d84598df3`)
- → Ledger::all_relationships (`a4b19ba4-2ef5-50a4-a90e-2107e783f4c8`)
- → Ledger::append_object (`b71bb7ad-337a-518f-9b6e-316178f45928`)
- → Ledger::get_relationship (`c9bf9448-5b90-56cb-b8bb-9a80138af70e`)
- → Ledger::get_object (`bc4b77e9-6e8d-54b0-aa9a-8fc066a535b3`)
- → Ledger::append_relationship (`7cfea349-6f7b-5501-8b20-1291291d672b`)
- → content_signature (`66c7da48-70e5-57fb-9882-5a5b05933963`)

### Contains

- ← ekos/crates/ledger/src/lib.rs (`21938f45-b767-5933-9e71-12e15ff53eb1`)

## Diagram

```mermaid
graph TD
    n35e9663b3b6d50ecad169721c45eb3d1["merge_stores"]
    n21938f45b76759339e7112e15ff53eb1["ekos/crates/ledger/src/lib.rs"]
    n21938f45b76759339e7112e15ff53eb1 -->|Contains| n35e9663b3b6d50ecad169721c45eb3d1
    nd640b0e7cfd156938c96022d84598df3["Ledger::all_objects"]
    n35e9663b3b6d50ecad169721c45eb3d1 -->|Calls| nd640b0e7cfd156938c96022d84598df3
    na4b19ba42ef550a4a90e2107e783f4c8["Ledger::all_relationships"]
    n35e9663b3b6d50ecad169721c45eb3d1 -->|Calls| na4b19ba42ef550a4a90e2107e783f4c8
    nb71bb7ad337a518f9b6e316178f45928["Ledger::append_object"]
    n35e9663b3b6d50ecad169721c45eb3d1 -->|Calls| nb71bb7ad337a518f9b6e316178f45928
    nc9bf94485b9056cbb8bb9a80138af70e["Ledger::get_relationship"]
    n35e9663b3b6d50ecad169721c45eb3d1 -->|Calls| nc9bf94485b9056cbb8bb9a80138af70e
    nbc4b77e96e8d54b0aa9a8fc066a535b3["Ledger::get_object"]
    n35e9663b3b6d50ecad169721c45eb3d1 -->|Calls| nbc4b77e96e8d54b0aa9a8fc066a535b3
    n7cfea3496f7b55018b201291291d672b["Ledger::append_relationship"]
    n35e9663b3b6d50ecad169721c45eb3d1 -->|Calls| n7cfea3496f7b55018b201291291d672b
    n66c7da4870e557fb98825a5b05933963["content_signature"]
    n35e9663b3b6d50ecad169721c45eb3d1 -->|Calls| n66c7da4870e557fb98825a5b05933963
```

## Evidence

_No evidence cited._
