# sqlparser::dialect::Dialect (RustModule)

## Properties

_No compiled properties._

## Relationships

### DependsOn

- ← ekos/plugins/sql-dialect-mysql/src/lib.rs (`5d3dc4f8-6d1a-5470-8a65-53b6c2be6d34`)
- ← ekos/crates/recovery/src/sql_analyzer.rs (`cd768c5e-1640-51c3-b6ec-cb7ac78ade6d`)
- ← ekos/plugins/sql-dialect-mssql/src/lib.rs (`c59f040e-79f2-52ac-86f5-a2484a89b01f`)
- ← ekos/crates/sql-dialect-sdk/src/lib.rs (`bace7c71-d661-554c-84b2-2fafb6b80602`)
- ← ekos/plugins/sql-dialect-postgres/src/lib.rs (`4635063b-44e9-5cfb-97e8-4e39451ffe73`)
- ← ekos/plugins/sql-dialect-snowflake/src/lib.rs (`0417a4c7-1d0b-52f5-b2de-f52571e9dc2b`)
- ← ekos/crates/recovery/src/sql_transform_analyzer.rs (`987e3fbb-31ec-576d-b794-7e801336e8c8`)
- ← ekos/crates/recovery/src/sql_dialect_registry.rs (`6e4ae0e1-9d1b-562a-a409-406a2ee0181a`)
- ← ekos/plugins/sql-dialect-databricks/src/lib.rs (`e03746a2-2ca7-57cf-abcf-b40da11fd1d4`)

## Diagram

```mermaid
graph TD
    n4a5f3d830abb560d97d059c138fe4300["sqlparser::dialect::Dialect"]
    n5d3dc4f86d1a54708a6553b6c2be6d34["ekos/plugins/sql-dialect-mysql/src/lib.rs"]
    n5d3dc4f86d1a54708a6553b6c2be6d34 -->|DependsOn| n4a5f3d830abb560d97d059c138fe4300
    ncd768c5e164051c3b6eccb7ac78ade6d["ekos/crates/recovery/src/sql_analyzer.rs"]
    ncd768c5e164051c3b6eccb7ac78ade6d -->|DependsOn| n4a5f3d830abb560d97d059c138fe4300
    nc59f040e79f252ac86f5a2484a89b01f["ekos/plugins/sql-dialect-mssql/src/lib.rs"]
    nc59f040e79f252ac86f5a2484a89b01f -->|DependsOn| n4a5f3d830abb560d97d059c138fe4300
    nbace7c71d661554c84b22fafb6b80602["ekos/crates/sql-dialect-sdk/src/lib.rs"]
    nbace7c71d661554c84b22fafb6b80602 -->|DependsOn| n4a5f3d830abb560d97d059c138fe4300
    n4635063b44e95cfb97e84e39451ffe73["ekos/plugins/sql-dialect-postgres/src/lib.rs"]
    n4635063b44e95cfb97e84e39451ffe73 -->|DependsOn| n4a5f3d830abb560d97d059c138fe4300
    n0417a4c71d0b52f5b2def52571e9dc2b["ekos/plugins/sql-dialect-snowflake/src/lib.rs"]
    n0417a4c71d0b52f5b2def52571e9dc2b -->|DependsOn| n4a5f3d830abb560d97d059c138fe4300
    n987e3fbb31ec576db7947e801336e8c8["ekos/crates/recovery/src/sql_transform_analyzer.rs"]
    n987e3fbb31ec576db7947e801336e8c8 -->|DependsOn| n4a5f3d830abb560d97d059c138fe4300
    n6e4ae0e19d1b562aa409406a2ee0181a["ekos/crates/recovery/src/sql_dialect_registry.rs"]
    n6e4ae0e19d1b562aa409406a2ee0181a -->|DependsOn| n4a5f3d830abb560d97d059c138fe4300
    ne03746a22ca757cfabcfb40da11fd1d4["ekos/plugins/sql-dialect-databricks/src/lib.rs"]
    ne03746a22ca757cfabcfb40da11fd1d4 -->|DependsOn| n4a5f3d830abb560d97d059c138fe4300
```

## Evidence

_No evidence cited._
