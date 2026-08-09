# ParquetExportReader::read_latest_batch (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | method |

## Relationships

### Calls

- → ParquetExportReader::latest_batch_dir (`438cce02-db5c-56f5-ac38-615fa7bb51bc`)
- → ParquetExportReader::read_evidence (`52ef400c-3e08-57e7-9937-27669b2386fa`)
- → ParquetExportReader::read_relationships (`c79b4df5-78e2-5990-a627-2851c1a75901`)
- → ParquetExportReader::read_entities (`f0b31d24-4edc-544e-96ac-fc790b32605d`)

### Contains

- ← ekos/plugins/crypto/src/lib.rs (`83728c61-df4d-5ad6-81c7-5bf50ff761fb`)

## Diagram

```mermaid
graph TD
    n573d78a3871e5467b795efe45e871700["ParquetExportReader::read_latest_batch"]
    n83728c61df4d5ad681c75bf50ff761fb["ekos/plugins/crypto/src/lib.rs"]
    n83728c61df4d5ad681c75bf50ff761fb -->|Contains| n573d78a3871e5467b795efe45e871700
    n438cce02db5c56f5ac38615fa7bb51bc["ParquetExportReader::latest_batch_dir"]
    n573d78a3871e5467b795efe45e871700 -->|Calls| n438cce02db5c56f5ac38615fa7bb51bc
    n52ef400c3e0857e7993727669b2386fa["ParquetExportReader::read_evidence"]
    n573d78a3871e5467b795efe45e871700 -->|Calls| n52ef400c3e0857e7993727669b2386fa
    nc79b4df578e25990a6272851c1a75901["ParquetExportReader::read_relationships"]
    n573d78a3871e5467b795efe45e871700 -->|Calls| nc79b4df578e25990a6272851c1a75901
    nf0b31d244edc544e96acfc790b32605d["ParquetExportReader::read_entities"]
    n573d78a3871e5467b795efe45e871700 -->|Calls| nf0b31d244edc544e96acfc790b32605d
```

## Evidence

_No evidence cited._
