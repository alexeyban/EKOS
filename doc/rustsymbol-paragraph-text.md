# paragraph_text (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- ← DocxParser::parse (`183af0c4-fe42-5273-beff-98b642123c3e`)
- ← table_rows (`69fa56db-88c6-5487-8895-f8907d29dee8`)

### Contains

- ← ekos/plugins/localdocs/src/docx.rs (`cb8db45b-ec2c-50f2-8053-0fd63dba3355`)

## Diagram

```mermaid
graph TD
    na86ca4ac57eb51f7be2fad53d6c6ee3d["paragraph_text"]
    ncb8db45bec2c50f280530fd63dba3355["ekos/plugins/localdocs/src/docx.rs"]
    ncb8db45bec2c50f280530fd63dba3355 -->|Contains| na86ca4ac57eb51f7be2fad53d6c6ee3d
    n183af0c4fe425273beff98b642123c3e["DocxParser::parse"]
    n183af0c4fe425273beff98b642123c3e -->|Calls| na86ca4ac57eb51f7be2fad53d6c6ee3d
    n69fa56db88c654878895f8907d29dee8["table_rows"]
    n69fa56db88c654878895f8907d29dee8 -->|Calls| na86ca4ac57eb51f7be2fad53d6c6ee3d
```

## Evidence

_No evidence cited._
