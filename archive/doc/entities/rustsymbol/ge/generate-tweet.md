# generate_tweet (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- → validate_tweet (`37c3c5db-a1d7-5bdc-b147-227ec222d7dd`)
- → draft_once (`3638e3bf-a432-54bc-8ec8-3db1c55ae4e1`)

### Contains

- ← ekos/crates/marketing/src/tweet.rs (`3372ee6e-2a1d-50b2-a3ae-b17eb421301b`)

## Diagram

```mermaid
graph TD
    nd45d31e2f90450098d24d48d8d3b2063["generate_tweet"]
    n3372ee6e2a1d50b2a3aeb17eb421301b["ekos/crates/marketing/src/tweet.rs"]
    n3372ee6e2a1d50b2a3aeb17eb421301b -->|Contains| nd45d31e2f90450098d24d48d8d3b2063
    n37c3c5dba1d75bdcb147227ec222d7dd["validate_tweet"]
    nd45d31e2f90450098d24d48d8d3b2063 -->|Calls| n37c3c5dba1d75bdcb147227ec222d7dd
    n3638e3bfa43254bc8ec83db1c55ae4e1["draft_once"]
    nd45d31e2f90450098d24d48d8d3b2063 -->|Calls| n3638e3bfa43254bc8ec83db1c55ae4e1
```

## Evidence

_No evidence cited._
