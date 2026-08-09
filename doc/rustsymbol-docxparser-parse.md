# DocxParser::parse (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | method |

## Relationships

### Calls

- → extract_media_images (`6e44c2bd-fcda-5588-b6b0-0796a82e8fc7`)
- → paragraph_text (`a86ca4ac-57eb-51f7-be2f-ad53d6c6ee3d`)
- → table_rows (`69fa56db-88c6-5487-8895-f8907d29dee8`)

### Contains

- ← ekos/plugins/localdocs/src/docx.rs (`cb8db45b-ec2c-50f2-8053-0fd63dba3355`)

## Diagram

```mermaid
graph TD
    n183af0c4fe425273beff98b642123c3e["DocxParser::parse"]
    ncb8db45bec2c50f280530fd63dba3355["ekos/plugins/localdocs/src/docx.rs"]
    ncb8db45bec2c50f280530fd63dba3355 -->|Contains| n183af0c4fe425273beff98b642123c3e
    n6e44c2bdfcda5588b6b00796a82e8fc7["extract_media_images"]
    n183af0c4fe425273beff98b642123c3e -->|Calls| n6e44c2bdfcda5588b6b00796a82e8fc7
    na86ca4ac57eb51f7be2fad53d6c6ee3d["paragraph_text"]
    n183af0c4fe425273beff98b642123c3e -->|Calls| na86ca4ac57eb51f7be2fad53d6c6ee3d
    n69fa56db88c654878895f8907d29dee8["table_rows"]
    n183af0c4fe425273beff98b642123c3e -->|Calls| n69fa56db88c654878895f8907d29dee8
```

## Evidence

_No evidence cited._
