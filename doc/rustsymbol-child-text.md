# child_text (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- ← parse_ktr (`ac857c0a-e7e1-5b8d-972d-dd8184852f15`)
- ← map_step (`47faefae-6222-5ae2-a4a9-9ec080997290`)
- ← extract_filter_condition (`83af4161-71c8-51b2-8260-b9f08c4e8e42`)
- ← extract_calculator (`175b4e7e-b982-5abc-9b19-cef4172fa469`)
- ← extract_join (`450b2ac3-e3bb-5b8b-a2d1-ed9e281bc3b2`)
- ← extract_join_keys (`73352edd-35a8-59e4-aff4-4f4ac9548170`)
- ← extract_stream_lookup (`2c163fbc-39ec-5e83-82a5-50dcc8f51cfb`)
- ← extract_group_by (`056c9294-b693-5d0d-84a7-1dca458e5118`)

### Contains

- ← ekos/crates/recovery/src/pentaho_analyzer.rs (`ce3d2f1b-e1c6-55d7-92bc-c1b69f76dbca`)

## Diagram

```mermaid
graph TD
    nf70791eabde457ab8af1ccc69fa9f5a7["child_text"]
    nce3d2f1be1c655d792bcc1b69f76dbca["ekos/crates/recovery/src/pentaho_analyzer.rs"]
    nce3d2f1be1c655d792bcc1b69f76dbca -->|Contains| nf70791eabde457ab8af1ccc69fa9f5a7
    nac857c0ae7e15b8d972ddd8184852f15["parse_ktr"]
    nac857c0ae7e15b8d972ddd8184852f15 -->|Calls| nf70791eabde457ab8af1ccc69fa9f5a7
    n47faefae62225ae2a4a99ec080997290["map_step"]
    n47faefae62225ae2a4a99ec080997290 -->|Calls| nf70791eabde457ab8af1ccc69fa9f5a7
    n83af416171c851b28260b9f08c4e8e42["extract_filter_condition"]
    n83af416171c851b28260b9f08c4e8e42 -->|Calls| nf70791eabde457ab8af1ccc69fa9f5a7
    n175b4e7eb9825abc9b19cef4172fa469["extract_calculator"]
    n175b4e7eb9825abc9b19cef4172fa469 -->|Calls| nf70791eabde457ab8af1ccc69fa9f5a7
    n450b2ac3e3bb5b8ba2d1ed9e281bc3b2["extract_join"]
    n450b2ac3e3bb5b8ba2d1ed9e281bc3b2 -->|Calls| nf70791eabde457ab8af1ccc69fa9f5a7
    n73352edd35a859e4aff44f4ac9548170["extract_join_keys"]
    n73352edd35a859e4aff44f4ac9548170 -->|Calls| nf70791eabde457ab8af1ccc69fa9f5a7
    n2c163fbc39ec5e8382a550dcc8f51cfb["extract_stream_lookup"]
    n2c163fbc39ec5e8382a550dcc8f51cfb -->|Calls| nf70791eabde457ab8af1ccc69fa9f5a7
    n056c9294b6935d0d84a71dca458e5118["extract_group_by"]
    n056c9294b6935d0d84a71dca458e5118 -->|Calls| nf70791eabde457ab8af1ccc69fa9f5a7
```

## Evidence

_No evidence cited._
