# print_storage_report (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- ← status (`f4b534fa-f104-5a49-993e-7b08b0b618c6`)
- → dir_size (`e58c8e45-8b1f-5641-90e0-3c1ee0c783db`)

### Contains

- ← ekos/crates/cli/src/commands/ledger.rs (`00bf5c8a-7198-5df3-a6eb-5bf22bc8ddcb`)

## Diagram

```mermaid
graph TD
    nd032d51f6a395f798089541c63e5c472["print_storage_report"]
    n00bf5c8a71985df3a6eb5bf22bc8ddcb["ekos/crates/cli/src/commands/ledger.rs"]
    n00bf5c8a71985df3a6eb5bf22bc8ddcb -->|Contains| nd032d51f6a395f798089541c63e5c472
    nf4b534faf1045a49993e7b08b0b618c6["status"]
    nf4b534faf1045a49993e7b08b0b618c6 -->|Calls| nd032d51f6a395f798089541c63e5c472
    ne58c8e458b1f564190e03c1ee0c783db["dir_size"]
    nd032d51f6a395f798089541c63e5c472 -->|Calls| ne58c8e458b1f564190e03c1ee0c783db
```

## Evidence

_No evidence cited._
