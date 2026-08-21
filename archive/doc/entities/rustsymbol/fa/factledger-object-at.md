# FactLedger::object_at (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | method |

## Relationships

### Calls

- → kind_of_payload (`abd8ce5a-e663-50ce-a42f-e2c4a13c43fb`)
- → Inner::reconstruct_at (`79bd7404-2990-533f-b4f5-b3d167543336`)
- → Inner::tx_at (`33bdc704-df71-5f9f-adc3-cd05c98590a2`)

### Contains

- ← ekos/crates/ledger/src/fact_ledger.rs (`eadcb59e-818f-5d1e-af87-ff29aba11423`)

## Diagram

```mermaid
graph TD
    n57760afcd78b5593ba0e9d8b30d725ce["FactLedger::object_at"]
    neadcb59e818f5d1eaf87ff29aba11423["ekos/crates/ledger/src/fact_ledger.rs"]
    neadcb59e818f5d1eaf87ff29aba11423 -->|Contains| n57760afcd78b5593ba0e9d8b30d725ce
    nabd8ce5ae66350cea42fe2c4a13c43fb["kind_of_payload"]
    n57760afcd78b5593ba0e9d8b30d725ce -->|Calls| nabd8ce5ae66350cea42fe2c4a13c43fb
    n79bd74042990533fb4f5b3d167543336["Inner::reconstruct_at"]
    n57760afcd78b5593ba0e9d8b30d725ce -->|Calls| n79bd74042990533fb4f5b3d167543336
    n33bdc704df715f9fadc3cd05c98590a2["Inner::tx_at"]
    n57760afcd78b5593ba0e9d8b30d725ce -->|Calls| n33bdc704df715f9fadc3cd05c98590a2
```

## Evidence

_No evidence cited._
