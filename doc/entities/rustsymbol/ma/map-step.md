# map_step (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- ← parse_ktr (`ac857c0a-e7e1-5b8d-972d-dd8184852f15`)
- → extract_calculator (`175b4e7e-b982-5abc-9b19-cef4172fa469`)
- → extract_join (`450b2ac3-e3bb-5b8b-a2d1-ed9e281bc3b2`)
- → extract_group_by (`056c9294-b693-5d0d-84a7-1dca458e5118`)
- → child_text (`f70791ea-bde4-57ab-8af1-ccc69fa9f5a7`)
- → extract_filter_condition (`83af4161-71c8-51b2-8260-b9f08c4e8e42`)
- → xml_slice (`0c5e1b6a-7829-5a6f-a831-e20b9eeb1c27`)
- → extract_table_from_sql (`5f575d34-9287-5ba9-b81a-30f2e77ea92b`)
- → extract_stream_lookup (`2c163fbc-39ec-5e83-82a5-50dcc8f51cfb`)

### Contains

- ← ekos/crates/recovery/src/pentaho_analyzer.rs (`ce3d2f1b-e1c6-55d7-92bc-c1b69f76dbca`)

## Diagram

```mermaid
graph TD
    n47faefae62225ae2a4a99ec080997290["map_step"]
    nce3d2f1be1c655d792bcc1b69f76dbca["ekos/crates/recovery/src/pentaho_analyzer.rs"]
    nce3d2f1be1c655d792bcc1b69f76dbca -->|Contains| n47faefae62225ae2a4a99ec080997290
    nac857c0ae7e15b8d972ddd8184852f15["parse_ktr"]
    nac857c0ae7e15b8d972ddd8184852f15 -->|Calls| n47faefae62225ae2a4a99ec080997290
    n175b4e7eb9825abc9b19cef4172fa469["extract_calculator"]
    n47faefae62225ae2a4a99ec080997290 -->|Calls| n175b4e7eb9825abc9b19cef4172fa469
    n450b2ac3e3bb5b8ba2d1ed9e281bc3b2["extract_join"]
    n47faefae62225ae2a4a99ec080997290 -->|Calls| n450b2ac3e3bb5b8ba2d1ed9e281bc3b2
    n056c9294b6935d0d84a71dca458e5118["extract_group_by"]
    n47faefae62225ae2a4a99ec080997290 -->|Calls| n056c9294b6935d0d84a71dca458e5118
    nf70791eabde457ab8af1ccc69fa9f5a7["child_text"]
    n47faefae62225ae2a4a99ec080997290 -->|Calls| nf70791eabde457ab8af1ccc69fa9f5a7
    n83af416171c851b28260b9f08c4e8e42["extract_filter_condition"]
    n47faefae62225ae2a4a99ec080997290 -->|Calls| n83af416171c851b28260b9f08c4e8e42
    n0c5e1b6a78295a6fa831e20b9eeb1c27["xml_slice"]
    n47faefae62225ae2a4a99ec080997290 -->|Calls| n0c5e1b6a78295a6fa831e20b9eeb1c27
    n5f575d3492875ba9b81a30f2e77ea92b["extract_table_from_sql"]
    n47faefae62225ae2a4a99ec080997290 -->|Calls| n5f575d3492875ba9b81a30f2e77ea92b
    n2c163fbc39ec5e8382a550dcc8f51cfb["extract_stream_lookup"]
    n47faefae62225ae2a4a99ec080997290 -->|Calls| n2c163fbc39ec5e8382a550dcc8f51cfb
```

## Evidence

_No evidence cited._
