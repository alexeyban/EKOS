# FactLedger::typed_current (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | method |

## Relationships

### Calls

- ← FactLedger::get_object (`7c9ba14e-5785-5789-88ed-e471ea3451de`)
- ← FactLedger::get_evidence (`448d1f84-aec7-5f32-9175-799189f617d4`)
- ← FactLedger::get_event (`afd17061-be0e-5b46-9e93-044793d5fac7`)
- ← FactLedger::get_relationship (`c6fb9c16-6ed5-5cea-98ef-75a6ed96b979`)
- → Inner::reconstruct_at (`79bd7404-2990-533f-b4f5-b3d167543336`)
- → kind_of_payload (`abd8ce5a-e663-50ce-a42f-e2c4a13c43fb`)

### Contains

- ← ekos/crates/ledger/src/fact_ledger.rs (`eadcb59e-818f-5d1e-af87-ff29aba11423`)

## Diagram

```mermaid
graph TD
    n70c6fe5b85675988b867f2d5db8b76a8["FactLedger::typed_current"]
    neadcb59e818f5d1eaf87ff29aba11423["ekos/crates/ledger/src/fact_ledger.rs"]
    neadcb59e818f5d1eaf87ff29aba11423 -->|Contains| n70c6fe5b85675988b867f2d5db8b76a8
    n7c9ba14e5785578988ede471ea3451de["FactLedger::get_object"]
    n7c9ba14e5785578988ede471ea3451de -->|Calls| n70c6fe5b85675988b867f2d5db8b76a8
    n448d1f84aec75f329175799189f617d4["FactLedger::get_evidence"]
    n448d1f84aec75f329175799189f617d4 -->|Calls| n70c6fe5b85675988b867f2d5db8b76a8
    nafd17061be0e5b469e93044793d5fac7["FactLedger::get_event"]
    nafd17061be0e5b469e93044793d5fac7 -->|Calls| n70c6fe5b85675988b867f2d5db8b76a8
    nc6fb9c166ed55cea98ef75a6ed96b979["FactLedger::get_relationship"]
    nc6fb9c166ed55cea98ef75a6ed96b979 -->|Calls| n70c6fe5b85675988b867f2d5db8b76a8
    n79bd74042990533fb4f5b3d167543336["Inner::reconstruct_at"]
    n70c6fe5b85675988b867f2d5db8b76a8 -->|Calls| n79bd74042990533fb4f5b3d167543336
    nabd8ce5ae66350cea42fe2c4a13c43fb["kind_of_payload"]
    n70c6fe5b85675988b867f2d5db8b76a8 -->|Calls| nabd8ce5ae66350cea42fe2c4a13c43fb
```

## Evidence

_No evidence cited._
