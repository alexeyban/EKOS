# Inner::tx_at (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | method |

## Relationships

### Calls

- ← FactLedger::object_at (`57760afc-d78b-5593-ba0e-9d8b30d725ce`)
- ← FactLedger::relationships_at (`c47392f6-8e4b-54df-9316-0196d42d6f5d`)

### Contains

- ← ekos/crates/ledger/src/fact_ledger.rs (`eadcb59e-818f-5d1e-af87-ff29aba11423`)

## Diagram

```mermaid
graph TD
    n33bdc704df715f9fadc3cd05c98590a2["Inner::tx_at"]
    neadcb59e818f5d1eaf87ff29aba11423["ekos/crates/ledger/src/fact_ledger.rs"]
    neadcb59e818f5d1eaf87ff29aba11423 -->|Contains| n33bdc704df715f9fadc3cd05c98590a2
    n57760afcd78b5593ba0e9d8b30d725ce["FactLedger::object_at"]
    n57760afcd78b5593ba0e9d8b30d725ce -->|Calls| n33bdc704df715f9fadc3cd05c98590a2
    nc47392f68e4b54df93160196d42d6f5d["FactLedger::relationships_at"]
    nc47392f68e4b54df93160196d42d6f5d -->|Calls| n33bdc704df715f9fadc3cd05c98590a2
```

## Evidence

_No evidence cited._
