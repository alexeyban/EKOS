# FactLedger::append_payload (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | method |

## Relationships

### Calls

- ← FactLedger::append_object (`80968d4b-8f2c-5ecc-9368-42863245986e`)
- ← FactLedger::append_evidence (`2e115223-2707-56f3-853a-b59319f23c31`)
- ← FactLedger::append_event (`ceeb005e-d8b7-5f22-84db-9ddcb1cad760`)
- ← FactLedger::append_relationship (`13eab4a6-a185-5803-a090-8def742c8045`)
- → FactLedger::append_inner (`abbc13e0-8705-5d3c-aa78-d21e9a9882ee`)

### Contains

- ← ekos/crates/ledger/src/fact_ledger.rs (`eadcb59e-818f-5d1e-af87-ff29aba11423`)

## Diagram

```mermaid
graph TD
    n59ba0d101ac65b4b96f4c69cac0c6d89["FactLedger::append_payload"]
    neadcb59e818f5d1eaf87ff29aba11423["ekos/crates/ledger/src/fact_ledger.rs"]
    neadcb59e818f5d1eaf87ff29aba11423 -->|Contains| n59ba0d101ac65b4b96f4c69cac0c6d89
    n80968d4b8f2c5ecc936842863245986e["FactLedger::append_object"]
    n80968d4b8f2c5ecc936842863245986e -->|Calls| n59ba0d101ac65b4b96f4c69cac0c6d89
    n2e115223270756f3853ab59319f23c31["FactLedger::append_evidence"]
    n2e115223270756f3853ab59319f23c31 -->|Calls| n59ba0d101ac65b4b96f4c69cac0c6d89
    nceeb005ed8b75f2284db9ddcb1cad760["FactLedger::append_event"]
    nceeb005ed8b75f2284db9ddcb1cad760 -->|Calls| n59ba0d101ac65b4b96f4c69cac0c6d89
    n13eab4a6a1855803a0908def742c8045["FactLedger::append_relationship"]
    n13eab4a6a1855803a0908def742c8045 -->|Calls| n59ba0d101ac65b4b96f4c69cac0c6d89
    nabbc13e087055d3caa78d21e9a9882ee["FactLedger::append_inner"]
    n59ba0d101ac65b4b96f4c69cac0c6d89 -->|Calls| nabbc13e087055d3caa78d21e9a9882ee
```

## Evidence

_No evidence cited._
