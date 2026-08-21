# Inner::relationship_candidates (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | method |

## Relationships

### Calls

- ← FactLedger::relationships_for (`9a0d2288-3396-581c-9545-542f0a759e37`)
- ← FactLedger::relationships_at (`c47392f6-8e4b-54df-9316-0196d42d6f5d`)

### Contains

- ← ekos/crates/ledger/src/fact_ledger.rs (`eadcb59e-818f-5d1e-af87-ff29aba11423`)

## Diagram

```mermaid
graph TD
    nd69a321275e058c2aef0332cec180e53["Inner::relationship_candidates"]
    neadcb59e818f5d1eaf87ff29aba11423["ekos/crates/ledger/src/fact_ledger.rs"]
    neadcb59e818f5d1eaf87ff29aba11423 -->|Contains| nd69a321275e058c2aef0332cec180e53
    n9a0d22883396581c9545542f0a759e37["FactLedger::relationships_for"]
    n9a0d22883396581c9545542f0a759e37 -->|Calls| nd69a321275e058c2aef0332cec180e53
    nc47392f68e4b54df93160196d42d6f5d["FactLedger::relationships_at"]
    nc47392f68e4b54df93160196d42d6f5d -->|Calls| nd69a321275e058c2aef0332cec180e53
```

## Evidence

_No evidence cited._
