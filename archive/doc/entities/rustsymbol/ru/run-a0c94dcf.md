# run (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- → Check::ok (`5da0003d-2839-5411-a0da-7f330c797391`)
- → Check::fail (`2825aa30-cb08-5ec6-9b4b-65a4a8900de9`)

### Contains

- ← ekos/crates/cli/src/commands/doctor.rs (`117003db-05ca-5009-9ea3-90c845aff5f4`)

## Diagram

```mermaid
graph TD
    na0c94dcffccb5968a4717dfd7b14aa6f["run"]
    n117003db05ca50099ea390c845aff5f4["ekos/crates/cli/src/commands/doctor.rs"]
    n117003db05ca50099ea390c845aff5f4 -->|Contains| na0c94dcffccb5968a4717dfd7b14aa6f
    n5da0003d28395411a0da7f330c797391["Check::ok"]
    na0c94dcffccb5968a4717dfd7b14aa6f -->|Calls| n5da0003d28395411a0da7f330c797391
    n2825aa30cb085ec69b4b65a4a8900de9["Check::fail"]
    na0c94dcffccb5968a4717dfd7b14aa6f -->|Calls| n2825aa30cb085ec69b4b65a4a8900de9
```

## Evidence

_No evidence cited._
