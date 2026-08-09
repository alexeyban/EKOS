# ParquetExportReader::read_relationships (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | method |

## Relationships

### Calls

- → read_rows (`2bca7d5f-e455-5aea-b4d1-308424636e35`)
- → get_string (`f54fbe87-d448-5710-8990-eee46048c1d4`)
- → get_string_list (`0e16c45a-8f3d-5203-b28e-9c2b4e5ca6f2`)
- ← ParquetExportReader::read_latest_batch (`573d78a3-871e-5467-b795-efe45e871700`)

### Contains

- ← ekos/plugins/crypto/src/lib.rs (`83728c61-df4d-5ad6-81c7-5bf50ff761fb`)

## Diagram

```mermaid
graph TD
    nc79b4df578e25990a6272851c1a75901["ParquetExportReader::read_relationships"]
    n83728c61df4d5ad681c75bf50ff761fb["ekos/plugins/crypto/src/lib.rs"]
    n83728c61df4d5ad681c75bf50ff761fb -->|Contains| nc79b4df578e25990a6272851c1a75901
    n2bca7d5fe4555aeab4d1308424636e35["read_rows"]
    nc79b4df578e25990a6272851c1a75901 -->|Calls| n2bca7d5fe4555aeab4d1308424636e35
    nf54fbe87d44857108990eee46048c1d4["get_string"]
    nc79b4df578e25990a6272851c1a75901 -->|Calls| nf54fbe87d44857108990eee46048c1d4
    n0e16c45a8f3d5203b28e9c2b4e5ca6f2["get_string_list"]
    nc79b4df578e25990a6272851c1a75901 -->|Calls| n0e16c45a8f3d5203b28e9c2b4e5ca6f2
    n573d78a3871e5467b795efe45e871700["ParquetExportReader::read_latest_batch"]
    n573d78a3871e5467b795efe45e871700 -->|Calls| nc79b4df578e25990a6272851c1a75901
```

## Evidence

_No evidence cited._
