# project (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- → encode_key (`5f3049a2-8e65-5510-9ee3-ab92690f254a`)
- → stores_values (`bd8d2a77-ff4c-5ab3-b99f-39afccdb9f6e`)
- ← write_run (`944c015c-ad50-5e55-9238-7d57ee4ed67b`)

### Contains

- ← ekos/crates/ledger/src/index.rs (`a43fa387-2cac-5166-bfc2-ae4c965fc2ac`)

## Diagram

```mermaid
graph TD
    n775a5c11b65e541da5314ca75476a2b6["project"]
    na43fa3872cac5166bfc2ae4c965fc2ac["ekos/crates/ledger/src/index.rs"]
    na43fa3872cac5166bfc2ae4c965fc2ac -->|Contains| n775a5c11b65e541da5314ca75476a2b6
    n5f3049a28e6555109ee3ab92690f254a["encode_key"]
    n775a5c11b65e541da5314ca75476a2b6 -->|Calls| n5f3049a28e6555109ee3ab92690f254a
    nbd8d2a77ff4c5ab3b99f39afccdb9f6e["stores_values"]
    n775a5c11b65e541da5314ca75476a2b6 -->|Calls| nbd8d2a77ff4c5ab3b99f39afccdb9f6e
    n944c015cad505e5592387d57ee4ed67b["write_run"]
    n944c015cad505e5592387d57ee4ed67b -->|Calls| n775a5c11b65e541da5314ca75476a2b6
```

## Evidence

_No evidence cited._
