# FactIndexes::add_runs (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | method |

## Relationships

### Calls

- → write_run (`944c015c-ad50-5e55-9238-7d57ee4ed67b`)
- → IndexRun::open (`f1391221-58bc-5970-9495-3366e24cd6f1`)
- ← FactIndexes::build_from_batches (`01b36534-35a3-5736-8a1e-c727d7f48136`)

### Contains

- ← ekos/crates/ledger/src/index.rs (`a43fa387-2cac-5166-bfc2-ae4c965fc2ac`)

## Diagram

```mermaid
graph TD
    n4f62bd8fa0025bbb8bc95c708f2849a5["FactIndexes::add_runs"]
    na43fa3872cac5166bfc2ae4c965fc2ac["ekos/crates/ledger/src/index.rs"]
    na43fa3872cac5166bfc2ae4c965fc2ac -->|Contains| n4f62bd8fa0025bbb8bc95c708f2849a5
    n944c015cad505e5592387d57ee4ed67b["write_run"]
    n4f62bd8fa0025bbb8bc95c708f2849a5 -->|Calls| n944c015cad505e5592387d57ee4ed67b
    nf139122158bc597094953366e24cd6f1["IndexRun::open"]
    n4f62bd8fa0025bbb8bc95c708f2849a5 -->|Calls| nf139122158bc597094953366e24cd6f1
    n01b3653435a357368a1ec727d7f48136["FactIndexes::build_from_batches"]
    n01b3653435a357368a1ec727d7f48136 -->|Calls| n4f62bd8fa0025bbb8bc95c708f2849a5
```

## Evidence

_No evidence cited._
