# ensure_statement_separators (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- → starts_with_keyword (`b60eebb1-ab9e-50e7-a550-162f7c6481ec`)
- → ends_with_set_op_keyword (`167e3cae-ee0f-52f4-bf81-52199410ffe7`)

### Contains

- ← ekos/crates/recovery/src/statement_repair.rs (`3ce1a143-e6f7-5a08-a3ba-cef397eb9447`)

## Diagram

```mermaid
graph TD
    n1b2dc403f43f5719912dcb9f12d3cadb["ensure_statement_separators"]
    n3ce1a143e6f75a08a3bacef397eb9447["ekos/crates/recovery/src/statement_repair.rs"]
    n3ce1a143e6f75a08a3bacef397eb9447 -->|Contains| n1b2dc403f43f5719912dcb9f12d3cadb
    nb60eebb1ab9e50e7a550162f7c6481ec["starts_with_keyword"]
    n1b2dc403f43f5719912dcb9f12d3cadb -->|Calls| nb60eebb1ab9e50e7a550162f7c6481ec
    n167e3caeee0f52f4bf8152199410ffe7["ends_with_set_op_keyword"]
    n1b2dc403f43f5719912dcb9f12d3cadb -->|Calls| n167e3caeee0f52f4bf8152199410ffe7
```

## Evidence

_No evidence cited._
