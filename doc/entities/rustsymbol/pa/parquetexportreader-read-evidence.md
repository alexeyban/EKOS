# ParquetExportReader::read_evidence (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | method |

## Relationships

### Calls

- → get_string (`f54fbe87-d448-5710-8990-eee46048c1d4`)
- → read_rows (`2bca7d5f-e455-5aea-b4d1-308424636e35`)
- ← ParquetExportReader::read_latest_batch (`573d78a3-871e-5467-b795-efe45e871700`)

### Contains

- ← ekos/plugins/crypto/src/lib.rs (`83728c61-df4d-5ad6-81c7-5bf50ff761fb`)

## Diagram

```mermaid
graph TD
    n52ef400c3e0857e7993727669b2386fa["ParquetExportReader::read_evidence"]
    n83728c61df4d5ad681c75bf50ff761fb["ekos/plugins/crypto/src/lib.rs"]
    n83728c61df4d5ad681c75bf50ff761fb -->|Contains| n52ef400c3e0857e7993727669b2386fa
    nf54fbe87d44857108990eee46048c1d4["get_string"]
    n52ef400c3e0857e7993727669b2386fa -->|Calls| nf54fbe87d44857108990eee46048c1d4
    n2bca7d5fe4555aeab4d1308424636e35["read_rows"]
    n52ef400c3e0857e7993727669b2386fa -->|Calls| n2bca7d5fe4555aeab4d1308424636e35
    n573d78a3871e5467b795efe45e871700["ParquetExportReader::read_latest_batch"]
    n573d78a3871e5467b795efe45e871700 -->|Calls| n52ef400c3e0857e7993727669b2386fa
```

## Evidence

_No evidence cited._
