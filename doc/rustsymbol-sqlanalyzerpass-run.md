# SqlAnalyzerPass::run (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | method |

## Relationships

### Calls

- → apply_llm_enrichment (`42cf8fb1-8f3d-51ce-b3c7-1b465fd82f18`)
- → parse_ddl_structural (`627d6f75-f3e8-5277-8b29-ff55036731c3`)

### Contains

- ← ekos/crates/recovery/src/sql_analyzer.rs (`cd768c5e-1640-51c3-b6ec-cb7ac78ade6d`)

## Diagram

```mermaid
graph TD
    n9650fe076a5c57dc9a45705995b82a4a["SqlAnalyzerPass::run"]
    ncd768c5e164051c3b6eccb7ac78ade6d["ekos/crates/recovery/src/sql_analyzer.rs"]
    ncd768c5e164051c3b6eccb7ac78ade6d -->|Contains| n9650fe076a5c57dc9a45705995b82a4a
    n42cf8fb18f3d51ceb3c71b465fd82f18["apply_llm_enrichment"]
    n9650fe076a5c57dc9a45705995b82a4a -->|Calls| n42cf8fb18f3d51ceb3c71b465fd82f18
    n627d6f75f3e852778b29ff55036731c3["parse_ddl_structural"]
    n9650fe076a5c57dc9a45705995b82a4a -->|Calls| n627d6f75f3e852778b29ff55036731c3
```

## Evidence

_No evidence cited._
