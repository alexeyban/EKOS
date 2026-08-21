# SapODataClient::get_json (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | method |

## Relationships

### Calls

- ← SapODataClient::list_business_objects (`53d863ca-386a-5e9e-81e9-aca8faaee7a1`)
- ← SapODataClient::list_organizational_units (`ecbe65cb-b5e4-57ad-aa7a-3c07155c1b75`)

### Contains

- ← ekos/plugins/sap/src/lib.rs (`8ce136d7-2eb9-53fd-90c2-7d0d00aeb27d`)

## Diagram

```mermaid
graph TD
    n2bc9e5ae60cd5c5f833dac55e39e19f3["SapODataClient::get_json"]
    n8ce136d72eb953fd90c27d0d00aeb27d["ekos/plugins/sap/src/lib.rs"]
    n8ce136d72eb953fd90c27d0d00aeb27d -->|Contains| n2bc9e5ae60cd5c5f833dac55e39e19f3
    n53d863ca386a5e9e81e9aca8faaee7a1["SapODataClient::list_business_objects"]
    n53d863ca386a5e9e81e9aca8faaee7a1 -->|Calls| n2bc9e5ae60cd5c5f833dac55e39e19f3
    necbe65cbb5e457adaa7a3c07155c1b75["SapODataClient::list_organizational_units"]
    necbe65cbb5e457adaa7a3c07155c1b75 -->|Calls| n2bc9e5ae60cd5c5f833dac55e39e19f3
```

## Evidence

_No evidence cited._
