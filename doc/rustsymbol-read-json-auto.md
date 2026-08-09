# read_json_auto (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- → read_json_zst (`1567b100-c499-5068-8613-ef76e744a280`)
- → zst_sibling (`eddec3cb-1d3e-5218-8af7-3692a9d8531d`)

### Contains

- ← ekos/crates/common/src/compress.rs (`99637da4-0489-5fca-ba15-b1144f48c3cc`)

## Diagram

```mermaid
graph TD
    nd2c994b8f8ac565db197a798dd05cf96["read_json_auto"]
    n99637da404895fcaba15b1144f48c3cc["ekos/crates/common/src/compress.rs"]
    n99637da404895fcaba15b1144f48c3cc -->|Contains| nd2c994b8f8ac565db197a798dd05cf96
    n1567b100c49950688613ef76e744a280["read_json_zst"]
    nd2c994b8f8ac565db197a798dd05cf96 -->|Calls| n1567b100c49950688613ef76e744a280
    neddec3cb1d3e52188af73692a9d8531d["zst_sibling"]
    nd2c994b8f8ac565db197a798dd05cf96 -->|Calls| neddec3cb1d3e52188af73692a9d8531d
```

## Evidence

_No evidence cited._
