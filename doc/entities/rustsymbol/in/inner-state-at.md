# Inner::state_at (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | method |

## Relationships

### Calls

- ← FactLedger::append_inner (`abbc13e0-8705-5d3c-aa78-d21e9a9882ee`)
- → fold_state (`42e75b53-6365-5fbc-83a1-26fcd87d8f3c`)
- → Inner::entity_entries (`1ada5b56-9514-5eae-b940-bf8f8ac90935`)
- ← Inner::reconstruct_at (`79bd7404-2990-533f-b4f5-b3d167543336`)

### Contains

- ← ekos/crates/ledger/src/fact_ledger.rs (`eadcb59e-818f-5d1e-af87-ff29aba11423`)

## Diagram

```mermaid
graph TD
    nf8ecc4120c5152758a0d2f41777af9ac["Inner::state_at"]
    neadcb59e818f5d1eaf87ff29aba11423["ekos/crates/ledger/src/fact_ledger.rs"]
    neadcb59e818f5d1eaf87ff29aba11423 -->|Contains| nf8ecc4120c5152758a0d2f41777af9ac
    nabbc13e087055d3caa78d21e9a9882ee["FactLedger::append_inner"]
    nabbc13e087055d3caa78d21e9a9882ee -->|Calls| nf8ecc4120c5152758a0d2f41777af9ac
    n42e75b5363655fbc83a126fcd87d8f3c["fold_state"]
    nf8ecc4120c5152758a0d2f41777af9ac -->|Calls| n42e75b5363655fbc83a126fcd87d8f3c
    n1ada5b5695145eaeb940bf8f8ac90935["Inner::entity_entries"]
    nf8ecc4120c5152758a0d2f41777af9ac -->|Calls| n1ada5b5695145eaeb940bf8f8ac90935
    n79bd74042990533fb4f5b3d167543336["Inner::reconstruct_at"]
    n79bd74042990533fb4f5b3d167543336 -->|Calls| nf8ecc4120c5152758a0d2f41777af9ac
```

## Evidence

_No evidence cited._
