# status (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- → dir_size (`e58c8e45-8b1f-5641-90e0-3c1ee0c783db`)
- → print_storage_report (`d032d51f-6a39-5f79-8089-541c63e5c472`)

### Contains

- ← ekos/crates/cli/src/commands/ledger.rs (`00bf5c8a-7198-5df3-a6eb-5bf22bc8ddcb`)

## Diagram

```mermaid
graph TD
    nf4b534faf1045a49993e7b08b0b618c6["status"]
    n00bf5c8a71985df3a6eb5bf22bc8ddcb["ekos/crates/cli/src/commands/ledger.rs"]
    n00bf5c8a71985df3a6eb5bf22bc8ddcb -->|Contains| nf4b534faf1045a49993e7b08b0b618c6
    ne58c8e458b1f564190e03c1ee0c783db["dir_size"]
    nf4b534faf1045a49993e7b08b0b618c6 -->|Calls| ne58c8e458b1f564190e03c1ee0c783db
    nd032d51f6a395f798089541c63e5c472["print_storage_report"]
    nf4b534faf1045a49993e7b08b0b618c6 -->|Calls| nd032d51f6a395f798089541c63e5c472
```

## Evidence

_No evidence cited._
