# parse_kettle_xml (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- ← PentahoAnalyzerPass::run (`144440b5-5a39-53d1-8f76-7f19376b0341`)
- → parse_kjb (`cc76104d-128b-5cc0-bcc8-9f5a4f895888`)
- → parse_ktr (`ac857c0a-e7e1-5b8d-972d-dd8184852f15`)

### Contains

- ← ekos/crates/recovery/src/pentaho_analyzer.rs (`ce3d2f1b-e1c6-55d7-92bc-c1b69f76dbca`)

## Diagram

```mermaid
graph TD
    nb954f76cb28d578ba82bcf5b8d11dec0["parse_kettle_xml"]
    nce3d2f1be1c655d792bcc1b69f76dbca["ekos/crates/recovery/src/pentaho_analyzer.rs"]
    nce3d2f1be1c655d792bcc1b69f76dbca -->|Contains| nb954f76cb28d578ba82bcf5b8d11dec0
    n144440b55a3953d18f767f19376b0341["PentahoAnalyzerPass::run"]
    n144440b55a3953d18f767f19376b0341 -->|Calls| nb954f76cb28d578ba82bcf5b8d11dec0
    ncc76104d128b5cc0bcc89f5a4f895888["parse_kjb"]
    nb954f76cb28d578ba82bcf5b8d11dec0 -->|Calls| ncc76104d128b5cc0bcc89f5a4f895888
    nac857c0ae7e15b8d972ddd8184852f15["parse_ktr"]
    nb954f76cb28d578ba82bcf5b8d11dec0 -->|Calls| nac857c0ae7e15b8d972ddd8184852f15
```

## Evidence

_No evidence cited._
