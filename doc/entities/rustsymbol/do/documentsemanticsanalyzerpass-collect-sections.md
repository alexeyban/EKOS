# DocumentSemanticsAnalyzerPass::collect_sections (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | method |

## Relationships

### Calls

- → sections_from_graph (`0ad06336-77bc-5090-8763-fb0959286b4f`)
- ← DocumentSemanticsAnalyzerPass::run (`1f105b9f-acbe-584a-bbbd-f0af139b4dc0`)

### Contains

- ← ekos/crates/recovery/src/document_semantics_analyzer.rs (`62e92526-f096-5ed2-bc72-0bdae8703aa3`)

## Diagram

```mermaid
graph TD
    nc02745f995045e9e878841ab8d507547["DocumentSemanticsAnalyzerPass::collect_sections"]
    n62e92526f0965ed2bc720bdae8703aa3["ekos/crates/recovery/src/document_semantics_analyzer.rs"]
    n62e92526f0965ed2bc720bdae8703aa3 -->|Contains| nc02745f995045e9e878841ab8d507547
    n0ad0633677bc50908763fb0959286b4f["sections_from_graph"]
    nc02745f995045e9e878841ab8d507547 -->|Calls| n0ad0633677bc50908763fb0959286b4f
    n1f105b9facbe584abbbdf0af139b4dc0["DocumentSemanticsAnalyzerPass::run"]
    n1f105b9facbe584abbbdf0af139b4dc0 -->|Calls| nc02745f995045e9e878841ab8d507547
```

## Evidence

_No evidence cited._
