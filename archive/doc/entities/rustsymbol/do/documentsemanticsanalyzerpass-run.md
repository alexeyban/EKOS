# DocumentSemanticsAnalyzerPass::run (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | method |

## Relationships

### Calls

- → concept_kir_id (`ab81e306-d655-5dce-b154-06330611490b`)
- → normalize_concept_name (`15a9b648-bf80-53fb-b983-b06f32cf57ca`)
- → DocumentSemanticsAnalyzerPass::collect_sections (`c02745f9-9504-5e9e-8788-41ab8d507547`)

### Contains

- ← ekos/crates/recovery/src/document_semantics_analyzer.rs (`62e92526-f096-5ed2-bc72-0bdae8703aa3`)

## Diagram

```mermaid
graph TD
    n1f105b9facbe584abbbdf0af139b4dc0["DocumentSemanticsAnalyzerPass::run"]
    n62e92526f0965ed2bc720bdae8703aa3["ekos/crates/recovery/src/document_semantics_analyzer.rs"]
    n62e92526f0965ed2bc720bdae8703aa3 -->|Contains| n1f105b9facbe584abbbdf0af139b4dc0
    nab81e306d6555dceb15406330611490b["concept_kir_id"]
    n1f105b9facbe584abbbdf0af139b4dc0 -->|Calls| nab81e306d6555dceb15406330611490b
    n15a9b648bf8053fbb983b06f32cf57ca["normalize_concept_name"]
    n1f105b9facbe584abbbdf0af139b4dc0 -->|Calls| n15a9b648bf8053fbb983b06f32cf57ca
    nc02745f995045e9e878841ab8d507547["DocumentSemanticsAnalyzerPass::collect_sections"]
    n1f105b9facbe584abbbdf0af139b4dc0 -->|Calls| nc02745f995045e9e878841ab8d507547
```

## Evidence

_No evidence cited._
