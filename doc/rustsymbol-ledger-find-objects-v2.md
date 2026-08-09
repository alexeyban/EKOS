# Ledger::find_objects_v2 (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | method |

## Relationships

### Calls

- ← Ledger::find_objects (`00beea88-f9ec-5c1b-88bb-7bc1fedb8fa8`)
- → EntryType::as_str (`0beb5b76-38f4-53a2-9ee4-e1070cca9822`)
- → Ledger::query_payloads (`b8401b6d-6d8d-5633-9b6a-27c093ab2db6`)

### Contains

- ← ekos/crates/ledger/src/lib.rs (`21938f45-b767-5933-9e71-12e15ff53eb1`)

## Diagram

```mermaid
graph TD
    nc1f796f94eda5e58bfc190620e984000["Ledger::find_objects_v2"]
    n21938f45b76759339e7112e15ff53eb1["ekos/crates/ledger/src/lib.rs"]
    n21938f45b76759339e7112e15ff53eb1 -->|Contains| nc1f796f94eda5e58bfc190620e984000
    n00beea88f9ec5c1b88bb7bc1fedb8fa8["Ledger::find_objects"]
    n00beea88f9ec5c1b88bb7bc1fedb8fa8 -->|Calls| nc1f796f94eda5e58bfc190620e984000
    n0beb5b7638f453a29ee4e1070cca9822["EntryType::as_str"]
    nc1f796f94eda5e58bfc190620e984000 -->|Calls| n0beb5b7638f453a29ee4e1070cca9822
    nb8401b6d6d8d56339b6a27c093ab2db6["Ledger::query_payloads"]
    nc1f796f94eda5e58bfc190620e984000 -->|Calls| nb8401b6d6d8d56339b6a27c093ab2db6
```

## Evidence

_No evidence cited._
