# extract_tables (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- ← PdfParser::parse_inner (`890fac4f-26ec-5bd0-af92-a13e19654f8e`)
- → split_table_row (`4868b5eb-e464-546c-b3d6-13dcd647b357`)
- → has_uniform_column_count (`4fc2a426-f74b-5e8e-b20d-e13e5d5492fc`)

### Contains

- ← ekos/plugins/localdocs/src/pdf.rs (`c8cae073-dd9b-50d2-8942-335d3cdf47b3`)

## Diagram

```mermaid
graph TD
    n0c7c90b3163e50de80ad2b1b6ca89b49["extract_tables"]
    nc8cae073dd9b50d28942335d3cdf47b3["ekos/plugins/localdocs/src/pdf.rs"]
    nc8cae073dd9b50d28942335d3cdf47b3 -->|Contains| n0c7c90b3163e50de80ad2b1b6ca89b49
    n890fac4f26ec5bd0af92a13e19654f8e["PdfParser::parse_inner"]
    n890fac4f26ec5bd0af92a13e19654f8e -->|Calls| n0c7c90b3163e50de80ad2b1b6ca89b49
    n4868b5ebe464546cb3d613dcd647b357["split_table_row"]
    n0c7c90b3163e50de80ad2b1b6ca89b49 -->|Calls| n4868b5ebe464546cb3d613dcd647b357
    n4fc2a426f74b5e8eb20de13e5d5492fc["has_uniform_column_count"]
    n0c7c90b3163e50de80ad2b1b6ca89b49 -->|Calls| n4fc2a426f74b5e8eb20de13e5d5492fc
```

## Evidence

_No evidence cited._
