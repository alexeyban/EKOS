# get_string (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- ← ParquetExportReader::read_entities (`f0b31d24-4edc-544e-96ac-fc790b32605d`)
- ← ParquetExportReader::read_relationships (`c79b4df5-78e2-5990-a627-2851c1a75901`)
- ← ParquetExportReader::read_evidence (`52ef400c-3e08-57e7-9937-27669b2386fa`)

### Contains

- ← ekos/plugins/crypto/src/lib.rs (`83728c61-df4d-5ad6-81c7-5bf50ff761fb`)

## Diagram

```mermaid
graph TD
    nf54fbe87d44857108990eee46048c1d4["get_string"]
    n83728c61df4d5ad681c75bf50ff761fb["ekos/plugins/crypto/src/lib.rs"]
    n83728c61df4d5ad681c75bf50ff761fb -->|Contains| nf54fbe87d44857108990eee46048c1d4
    nf0b31d244edc544e96acfc790b32605d["ParquetExportReader::read_entities"]
    nf0b31d244edc544e96acfc790b32605d -->|Calls| nf54fbe87d44857108990eee46048c1d4
    nc79b4df578e25990a6272851c1a75901["ParquetExportReader::read_relationships"]
    nc79b4df578e25990a6272851c1a75901 -->|Calls| nf54fbe87d44857108990eee46048c1d4
    n52ef400c3e0857e7993727669b2386fa["ParquetExportReader::read_evidence"]
    n52ef400c3e0857e7993727669b2386fa -->|Calls| nf54fbe87d44857108990eee46048c1d4
```

## Evidence

_No evidence cited._
