# self_counts (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- ← FactLedger::diff (`499f0ae3-66da-5863-8b37-46030a99f8e2`)
- → Inner::entities_with_attr (`b9448fb3-4dac-5309-ac98-0ad1ed35e6b0`)
- → Inner::reconstruct_at (`79bd7404-2990-533f-b4f5-b3d167543336`)

### Contains

- ← ekos/crates/ledger/src/fact_ledger.rs (`eadcb59e-818f-5d1e-af87-ff29aba11423`)

## Diagram

```mermaid
graph TD
    nce94f5294432555b895b41dffaad4ba6["self_counts"]
    neadcb59e818f5d1eaf87ff29aba11423["ekos/crates/ledger/src/fact_ledger.rs"]
    neadcb59e818f5d1eaf87ff29aba11423 -->|Contains| nce94f5294432555b895b41dffaad4ba6
    n499f0ae366da58638b3746030a99f8e2["FactLedger::diff"]
    n499f0ae366da58638b3746030a99f8e2 -->|Calls| nce94f5294432555b895b41dffaad4ba6
    nb9448fb34dac5309ac980ad1ed35e6b0["Inner::entities_with_attr"]
    nce94f5294432555b895b41dffaad4ba6 -->|Calls| nb9448fb34dac5309ac980ad1ed35e6b0
    n79bd74042990533fb4f5b3d167543336["Inner::reconstruct_at"]
    nce94f5294432555b895b41dffaad4ba6 -->|Calls| n79bd74042990533fb4f5b3d167543336
```

## Evidence

_No evidence cited._
