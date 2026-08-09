# write_run (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- → project (`775a5c11-b65e-541d-a531-4ca75476a2b6`)
- → write_run_raw (`be4b7353-317e-58f9-a870-099ca471e2d5`)
- ← FactIndexes::add_runs (`4f62bd8f-a002-5bbb-8bc9-5c708f2849a5`)

### Contains

- ← ekos/crates/ledger/src/index.rs (`a43fa387-2cac-5166-bfc2-ae4c965fc2ac`)

## Diagram

```mermaid
graph TD
    n944c015cad505e5592387d57ee4ed67b["write_run"]
    na43fa3872cac5166bfc2ae4c965fc2ac["ekos/crates/ledger/src/index.rs"]
    na43fa3872cac5166bfc2ae4c965fc2ac -->|Contains| n944c015cad505e5592387d57ee4ed67b
    n775a5c11b65e541da5314ca75476a2b6["project"]
    n944c015cad505e5592387d57ee4ed67b -->|Calls| n775a5c11b65e541da5314ca75476a2b6
    nbe4b7353317e58f9a870099ca471e2d5["write_run_raw"]
    n944c015cad505e5592387d57ee4ed67b -->|Calls| nbe4b7353317e58f9a870099ca471e2d5
    n4f62bd8fa0025bbb8bc95c708f2849a5["FactIndexes::add_runs"]
    n4f62bd8fa0025bbb8bc95c708f2849a5 -->|Calls| n944c015cad505e5592387d57ee4ed67b
```

## Evidence

_No evidence cited._
