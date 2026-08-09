# Inner::entities_with_attr (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | method |

## Relationships

### Calls

- ← FactLedger::object_count (`7b06e1e8-cc08-5acd-90b9-1e5223ddf52a`)
- ← FactLedger::relationship_count (`9930e168-65a3-57b3-95cc-29f7cd3aac53`)
- ← self_counts (`ce94f529-4432-555b-895b-41dffaad4ba6`)

### Contains

- ← ekos/crates/ledger/src/fact_ledger.rs (`eadcb59e-818f-5d1e-af87-ff29aba11423`)

## Diagram

```mermaid
graph TD
    nb9448fb34dac5309ac980ad1ed35e6b0["Inner::entities_with_attr"]
    neadcb59e818f5d1eaf87ff29aba11423["ekos/crates/ledger/src/fact_ledger.rs"]
    neadcb59e818f5d1eaf87ff29aba11423 -->|Contains| nb9448fb34dac5309ac980ad1ed35e6b0
    n7b06e1e8cc085acd90b91e5223ddf52a["FactLedger::object_count"]
    n7b06e1e8cc085acd90b91e5223ddf52a -->|Calls| nb9448fb34dac5309ac980ad1ed35e6b0
    n9930e16865a357b395cc29f7cd3aac53["FactLedger::relationship_count"]
    n9930e16865a357b395cc29f7cd3aac53 -->|Calls| nb9448fb34dac5309ac980ad1ed35e6b0
    nce94f5294432555b895b41dffaad4ba6["self_counts"]
    nce94f5294432555b895b41dffaad4ba6 -->|Calls| nb9448fb34dac5309ac980ad1ed35e6b0
```

## Evidence

_No evidence cited._
