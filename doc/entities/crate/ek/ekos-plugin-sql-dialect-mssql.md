# ekos-plugin-sql-dialect-mssql (Crate)

## Properties

| Key | Value |
|---|---|
| `description` | MSSQL (T-SQL) SqlDialectParser plugin (RFC 0039) |
| `path` | ekos/plugins/sql-dialect-mssql |

## Relationships

### DependsOn

- ← ekos-recovery (`28244ebb-4e16-5e8d-a637-5e750d01f2b8`) — evidence: ekos-recovery depends on ekos-plugin-sql-dialect-mssql (path dependency)
- → ekos-sql-dialect-sdk (`bf4371bd-7cee-54d1-9457-06a1079a38cf`) — evidence: ekos-plugin-sql-dialect-mssql depends on ekos-sql-dialect-sdk (path dependency)
- → sqlparser (`bb72fd94-a6bc-5672-84ef-bdeeb20ad78c`) — evidence: ekos-plugin-sql-dialect-mssql depends on sqlparser 0.53

## Diagram

```mermaid
graph TD
    n05ad9d89d39c5316b4132903b6b557db["ekos-plugin-sql-dialect-mssql"]
    n28244ebb4e165e8da6375e750d01f2b8["ekos-recovery"]
    n28244ebb4e165e8da6375e750d01f2b8 -->|DependsOn| n05ad9d89d39c5316b4132903b6b557db
    nbf4371bd7cee54d1945706a1079a38cf["ekos-sql-dialect-sdk"]
    n05ad9d89d39c5316b4132903b6b557db -->|DependsOn| nbf4371bd7cee54d1945706a1079a38cf
    nbb72fd94a6bc567284efbdeeb20ad78c["sqlparser"]
    n05ad9d89d39c5316b4132903b6b557db -->|DependsOn| nbb72fd94a6bc567284efbdeeb20ad78c
```

## Evidence

- `ec66ef4c-5e4d-4866-b99b-18c56cbd0891` — ekos-recovery depends on ekos-plugin-sql-dialect-mssql (path dependency) (confidence: 1.00)
- `72ab73b1-7288-4551-a812-fb4d3cde32f6` — ekos-plugin-sql-dialect-mssql depends on ekos-sql-dialect-sdk (path dependency) (confidence: 1.00)
- `4b0385d2-27b5-4fd6-9bfb-804f15a24974` — ekos-plugin-sql-dialect-mssql depends on sqlparser 0.53 (confidence: 1.00)
