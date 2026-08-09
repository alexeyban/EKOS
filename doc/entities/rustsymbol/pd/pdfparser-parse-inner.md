# PdfParser::parse_inner (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | method |

## Relationships

### Calls

- ← PdfParser::parse (`1299777d-de94-5dcc-a207-66cde26f7a41`)
- → extract_tables (`0c7c90b3-163e-50de-80ad-2b1b6ca89b49`)
- → extract_sections (`03bcb42a-299d-5751-b9f5-0ebb5987a1ec`)

### Contains

- ← ekos/plugins/localdocs/src/pdf.rs (`c8cae073-dd9b-50d2-8942-335d3cdf47b3`)

## Diagram

```mermaid
graph TD
    n890fac4f26ec5bd0af92a13e19654f8e["PdfParser::parse_inner"]
    nc8cae073dd9b50d28942335d3cdf47b3["ekos/plugins/localdocs/src/pdf.rs"]
    nc8cae073dd9b50d28942335d3cdf47b3 -->|Contains| n890fac4f26ec5bd0af92a13e19654f8e
    n1299777dde945dcca20766cde26f7a41["PdfParser::parse"]
    n1299777dde945dcca20766cde26f7a41 -->|Calls| n890fac4f26ec5bd0af92a13e19654f8e
    n0c7c90b3163e50de80ad2b1b6ca89b49["extract_tables"]
    n890fac4f26ec5bd0af92a13e19654f8e -->|Calls| n0c7c90b3163e50de80ad2b1b6ca89b49
    n03bcb42a299d5751b9f50ebb5987a1ec["extract_sections"]
    n890fac4f26ec5bd0af92a13e19654f8e -->|Calls| n03bcb42a299d5751b9f50ebb5987a1ec
```

## Evidence

_No evidence cited._
