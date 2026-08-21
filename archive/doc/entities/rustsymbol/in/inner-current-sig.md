# Inner::current_sig (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | method |

## Relationships

### Calls

- ← FactLedger::current_signature (`ce34047f-7c08-56f8-8b0f-7bb23b073282`)
- ← FactLedger::append_inner (`abbc13e0-8705-5d3c-aa78-d21e9a9882ee`)
- → Inner::reconstruct_at (`79bd7404-2990-533f-b4f5-b3d167543336`)

### Contains

- ← ekos/crates/ledger/src/fact_ledger.rs (`eadcb59e-818f-5d1e-af87-ff29aba11423`)

## Diagram

```mermaid
graph TD
    nba605f6f3be3568994ce047beed35236["Inner::current_sig"]
    neadcb59e818f5d1eaf87ff29aba11423["ekos/crates/ledger/src/fact_ledger.rs"]
    neadcb59e818f5d1eaf87ff29aba11423 -->|Contains| nba605f6f3be3568994ce047beed35236
    nce34047f7c0856f88b0f7bb23b073282["FactLedger::current_signature"]
    nce34047f7c0856f88b0f7bb23b073282 -->|Calls| nba605f6f3be3568994ce047beed35236
    nabbc13e087055d3caa78d21e9a9882ee["FactLedger::append_inner"]
    nabbc13e087055d3caa78d21e9a9882ee -->|Calls| nba605f6f3be3568994ce047beed35236
    n79bd74042990533fb4f5b3d167543336["Inner::reconstruct_at"]
    nba605f6f3be3568994ce047beed35236 -->|Calls| n79bd74042990533fb4f5b3d167543336
```

## Evidence

_No evidence cited._
