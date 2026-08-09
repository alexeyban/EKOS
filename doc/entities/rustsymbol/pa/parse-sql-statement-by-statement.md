# parse_sql_statement_by_statement (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- ← parse_sql_to_transform_graphs (`fbbf8304-b139-51e0-bbeb-a3ae80044130`)
- → dispatch_one_statement (`23b79c8c-1748-5c88-b609-df3f07bd4779`)

### Contains

- ← ekos/crates/recovery/src/sql_transform_analyzer.rs (`987e3fbb-31ec-576d-b794-7e801336e8c8`)

## Diagram

```mermaid
graph TD
    n5496bf596cfe52088e5c2ac011f7f9d8["parse_sql_statement_by_statement"]
    n987e3fbb31ec576db7947e801336e8c8["ekos/crates/recovery/src/sql_transform_analyzer.rs"]
    n987e3fbb31ec576db7947e801336e8c8 -->|Contains| n5496bf596cfe52088e5c2ac011f7f9d8
    nfbbf8304b13951e0bbeba3ae80044130["parse_sql_to_transform_graphs"]
    nfbbf8304b13951e0bbeba3ae80044130 -->|Calls| n5496bf596cfe52088e5c2ac011f7f9d8
    n23b79c8c17485c88b609df3f07bd4779["dispatch_one_statement"]
    n5496bf596cfe52088e5c2ac011f7f9d8 -->|Calls| n23b79c8c17485c88b609df3f07bd4779
```

## Evidence

_No evidence cited._
