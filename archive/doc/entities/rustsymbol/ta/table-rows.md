# table_rows (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- ← DocxParser::parse (`183af0c4-fe42-5273-beff-98b642123c3e`)
- → paragraph_text (`a86ca4ac-57eb-51f7-be2f-ad53d6c6ee3d`)

### Contains

- ← ekos/plugins/localdocs/src/docx.rs (`cb8db45b-ec2c-50f2-8053-0fd63dba3355`)

## Diagram

```mermaid
graph TD
    n69fa56db88c654878895f8907d29dee8["table_rows"]
    ncb8db45bec2c50f280530fd63dba3355["ekos/plugins/localdocs/src/docx.rs"]
    ncb8db45bec2c50f280530fd63dba3355 -->|Contains| n69fa56db88c654878895f8907d29dee8
    n183af0c4fe425273beff98b642123c3e["DocxParser::parse"]
    n183af0c4fe425273beff98b642123c3e -->|Calls| n69fa56db88c654878895f8907d29dee8
    na86ca4ac57eb51f7be2fad53d6c6ee3d["paragraph_text"]
    n69fa56db88c654878895f8907d29dee8 -->|Calls| na86ca4ac57eb51f7be2fad53d6c6ee3d
```

## Evidence

_No evidence cited._
