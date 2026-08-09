# ekos-plugin-sql-dialect-postgres (Crate)

## Properties

| Key | Value |
|---|---|
| `description` | PostgreSQL SqlDialectParser plugin (RFC 0031) |
| `path` | ekos/plugins/sql-dialect-postgres |

## Relationships

### DependsOn

- ← ekos-recovery (`28244ebb-4e16-5e8d-a637-5e750d01f2b8`) — evidence: ekos-recovery depends on ekos-plugin-sql-dialect-postgres (path dependency)
- → ekos-sql-dialect-sdk (`bf4371bd-7cee-54d1-9457-06a1079a38cf`) — evidence: ekos-plugin-sql-dialect-postgres depends on ekos-sql-dialect-sdk (path dependency)
- → sqlparser (`bb72fd94-a6bc-5672-84ef-bdeeb20ad78c`) — evidence: ekos-plugin-sql-dialect-postgres depends on sqlparser 0.53

## Diagram

```mermaid
graph TD
    nff9a3a7c06105442ac0b210e45700aad["ekos-plugin-sql-dialect-postgres"]
    n28244ebb4e165e8da6375e750d01f2b8["ekos-recovery"]
    n28244ebb4e165e8da6375e750d01f2b8 -->|DependsOn| nff9a3a7c06105442ac0b210e45700aad
    nbf4371bd7cee54d1945706a1079a38cf["ekos-sql-dialect-sdk"]
    nff9a3a7c06105442ac0b210e45700aad -->|DependsOn| nbf4371bd7cee54d1945706a1079a38cf
    nbb72fd94a6bc567284efbdeeb20ad78c["sqlparser"]
    nff9a3a7c06105442ac0b210e45700aad -->|DependsOn| nbb72fd94a6bc567284efbdeeb20ad78c
```

## Evidence

- `8dce1dcc-d9ad-453f-98ba-b59444b521fc` — ekos-recovery depends on ekos-plugin-sql-dialect-postgres (path dependency) (confidence: 1.00)
- `bc938656-155b-473f-8027-31f18ea9fe69` — ekos-plugin-sql-dialect-postgres depends on ekos-sql-dialect-sdk (path dependency) (confidence: 1.00)
- `32d8c775-8ae3-4c4a-8341-a1aac614e861` — ekos-plugin-sql-dialect-postgres depends on sqlparser 0.53 (confidence: 1.00)
