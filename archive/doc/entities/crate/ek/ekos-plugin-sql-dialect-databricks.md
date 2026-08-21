# ekos-plugin-sql-dialect-databricks (Crate)

## Properties

| Key | Value |
|---|---|
| `description` | Databricks (Spark SQL) SqlDialectParser plugin (RFC 0039) |
| `path` | ekos/plugins/sql-dialect-databricks |

## Relationships

### DependsOn

- ← ekos-recovery (`28244ebb-4e16-5e8d-a637-5e750d01f2b8`) — evidence: ekos-recovery depends on ekos-plugin-sql-dialect-databricks (path dependency)
- → ekos-sql-dialect-sdk (`bf4371bd-7cee-54d1-9457-06a1079a38cf`) — evidence: ekos-plugin-sql-dialect-databricks depends on ekos-sql-dialect-sdk (path dependency)
- → sqlparser (`bb72fd94-a6bc-5672-84ef-bdeeb20ad78c`) — evidence: ekos-plugin-sql-dialect-databricks depends on sqlparser 0.53

## Diagram

```mermaid
graph TD
    n920f420348d45079a5ee41b212c4858c["ekos-plugin-sql-dialect-databricks"]
    n28244ebb4e165e8da6375e750d01f2b8["ekos-recovery"]
    n28244ebb4e165e8da6375e750d01f2b8 -->|DependsOn| n920f420348d45079a5ee41b212c4858c
    nbf4371bd7cee54d1945706a1079a38cf["ekos-sql-dialect-sdk"]
    n920f420348d45079a5ee41b212c4858c -->|DependsOn| nbf4371bd7cee54d1945706a1079a38cf
    nbb72fd94a6bc567284efbdeeb20ad78c["sqlparser"]
    n920f420348d45079a5ee41b212c4858c -->|DependsOn| nbb72fd94a6bc567284efbdeeb20ad78c
```

## Evidence

- `4355cf7d-96c3-48bf-8b02-86beae0a2694` — ekos-recovery depends on ekos-plugin-sql-dialect-databricks (path dependency) (confidence: 1.00)
- `ccc55b4a-d6e9-4880-89c8-52aa896e77a1` — ekos-plugin-sql-dialect-databricks depends on ekos-sql-dialect-sdk (path dependency) (confidence: 1.00)
- `4f3db0ae-aeb3-4054-a11b-9dd91cd16ca9` — ekos-plugin-sql-dialect-databricks depends on sqlparser 0.53 (confidence: 1.00)
