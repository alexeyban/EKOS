# FactLedger::diff (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | method |

## Relationships

### Calls

- → Inner::reconstruct_at (`79bd7404-2990-533f-b4f5-b3d167543336`)
- → self_counts (`ce94f529-4432-555b-895b-41dffaad4ba6`)

### Contains

- ← ekos/crates/ledger/src/fact_ledger.rs (`eadcb59e-818f-5d1e-af87-ff29aba11423`)

## Diagram

```mermaid
graph TD
    n499f0ae366da58638b3746030a99f8e2["FactLedger::diff"]
    neadcb59e818f5d1eaf87ff29aba11423["ekos/crates/ledger/src/fact_ledger.rs"]
    neadcb59e818f5d1eaf87ff29aba11423 -->|Contains| n499f0ae366da58638b3746030a99f8e2
    n79bd74042990533fb4f5b3d167543336["Inner::reconstruct_at"]
    n499f0ae366da58638b3746030a99f8e2 -->|Calls| n79bd74042990533fb4f5b3d167543336
    nce94f5294432555b895b41dffaad4ba6["self_counts"]
    n499f0ae366da58638b3746030a99f8e2 -->|Calls| nce94f5294432555b895b41dffaad4ba6
```

## Evidence

_No evidence cited._
