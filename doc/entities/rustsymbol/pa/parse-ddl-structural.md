# parse_ddl_structural (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- ← SqlAnalyzerPass::run (`9650fe07-6a5c-57dc-9a45-705995b82a4a`)
- → add_fk_relationship (`1a03d472-0f17-5c60-ac70-af44732a3110`)
- → col_names (`60432b6b-4858-52ab-8fa5-4825c5600764`)
- → columns_json (`01ed4254-b135-5b4e-a18a-dd6e9163f9ce`)

### Contains

- ← ekos/crates/recovery/src/sql_analyzer.rs (`cd768c5e-1640-51c3-b6ec-cb7ac78ade6d`)

## Diagram

```mermaid
graph TD
    n627d6f75f3e852778b29ff55036731c3["parse_ddl_structural"]
    ncd768c5e164051c3b6eccb7ac78ade6d["ekos/crates/recovery/src/sql_analyzer.rs"]
    ncd768c5e164051c3b6eccb7ac78ade6d -->|Contains| n627d6f75f3e852778b29ff55036731c3
    n9650fe076a5c57dc9a45705995b82a4a["SqlAnalyzerPass::run"]
    n9650fe076a5c57dc9a45705995b82a4a -->|Calls| n627d6f75f3e852778b29ff55036731c3
    n1a03d4720f175c60ac70af44732a3110["add_fk_relationship"]
    n627d6f75f3e852778b29ff55036731c3 -->|Calls| n1a03d4720f175c60ac70af44732a3110
    n60432b6b485852ab8fa54825c5600764["col_names"]
    n627d6f75f3e852778b29ff55036731c3 -->|Calls| n60432b6b485852ab8fa54825c5600764
    n01ed4254b1355b4ea18add6e9163f9ce["columns_json"]
    n627d6f75f3e852778b29ff55036731c3 -->|Calls| n01ed4254b1355b4ea18add6e9163f9ce
```

## Evidence

_No evidence cited._
