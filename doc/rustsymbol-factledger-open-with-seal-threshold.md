# FactLedger::open_with_seal_threshold (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | method |

## Relationships

### Calls

- ← FactLedger::open (`0b6a6624-b311-5cb0-84ca-cc0ba0b33ed1`)
- → Inner::index_object (`c32e3f6e-7f6e-585f-ae03-f82f97de91ed`)
- → kind_of_payload (`abd8ce5a-e663-50ce-a42f-e2c4a13c43fb`)
- → Inner::reconstruct_at (`79bd7404-2990-533f-b4f5-b3d167543336`)

### Contains

- ← ekos/crates/ledger/src/fact_ledger.rs (`eadcb59e-818f-5d1e-af87-ff29aba11423`)

## Diagram

```mermaid
graph TD
    n50a7d9c47eb25d0c9c805e2982e59574["FactLedger::open_with_seal_threshold"]
    neadcb59e818f5d1eaf87ff29aba11423["ekos/crates/ledger/src/fact_ledger.rs"]
    neadcb59e818f5d1eaf87ff29aba11423 -->|Contains| n50a7d9c47eb25d0c9c805e2982e59574
    n0b6a6624b3115cb084cacc0ba0b33ed1["FactLedger::open"]
    n0b6a6624b3115cb084cacc0ba0b33ed1 -->|Calls| n50a7d9c47eb25d0c9c805e2982e59574
    nc32e3f6e7f6e585fae03f82f97de91ed["Inner::index_object"]
    n50a7d9c47eb25d0c9c805e2982e59574 -->|Calls| nc32e3f6e7f6e585fae03f82f97de91ed
    nabd8ce5ae66350cea42fe2c4a13c43fb["kind_of_payload"]
    n50a7d9c47eb25d0c9c805e2982e59574 -->|Calls| nabd8ce5ae66350cea42fe2c4a13c43fb
    n79bd74042990533fb4f5b3d167543336["Inner::reconstruct_at"]
    n50a7d9c47eb25d0c9c805e2982e59574 -->|Calls| n79bd74042990533fb4f5b3d167543336
```

## Evidence

_No evidence cited._
