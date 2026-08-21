# dir_size (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- ← status (`f4b534fa-f104-5a49-993e-7b08b0b618c6`)
- ← print_storage_report (`d032d51f-6a39-5f79-8089-541c63e5c472`)
- → dir_size (`e58c8e45-8b1f-5641-90e0-3c1ee0c783db`)

### Contains

- ← ekos/crates/cli/src/commands/ledger.rs (`00bf5c8a-7198-5df3-a6eb-5bf22bc8ddcb`)

## Diagram

```mermaid
graph TD
    ne58c8e458b1f564190e03c1ee0c783db["dir_size"]
    n00bf5c8a71985df3a6eb5bf22bc8ddcb["ekos/crates/cli/src/commands/ledger.rs"]
    n00bf5c8a71985df3a6eb5bf22bc8ddcb -->|Contains| ne58c8e458b1f564190e03c1ee0c783db
    nf4b534faf1045a49993e7b08b0b618c6["status"]
    nf4b534faf1045a49993e7b08b0b618c6 -->|Calls| ne58c8e458b1f564190e03c1ee0c783db
    nd032d51f6a395f798089541c63e5c472["print_storage_report"]
    nd032d51f6a395f798089541c63e5c472 -->|Calls| ne58c8e458b1f564190e03c1ee0c783db
    ne58c8e458b1f564190e03c1ee0c783db -->|Calls| ne58c8e458b1f564190e03c1ee0c783db
```

## Evidence

_No evidence cited._
