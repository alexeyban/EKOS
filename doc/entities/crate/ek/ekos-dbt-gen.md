# ekos-dbt-gen (Crate)

## Properties

| Key | Value |
|---|---|
| `description` | RFC 0036 — Pentaho to dbt Model Export: renders compiled Transformation IR nodes as dbt SQL models + schema.yml |
| `path` | ekos/crates/dbt-gen |

## Relationships

### DependsOn

- ← ekos (`abd31cd9-b31d-54c5-87cd-8a4a5b9a30a0`) — evidence: ekos depends on ekos-dbt-gen (path dependency)
- → ekos-kir (`7e3bc0de-d888-55cd-aa9a-f333f6e2cbb2`) — evidence: ekos-dbt-gen depends on ekos-kir (path dependency)
- → serde_json (`02548eee-6478-5324-851f-3a6329ccfeca`) — evidence: ekos-dbt-gen depends on serde_json 1

## Diagram

```mermaid
graph TD
    n9b66a043a00958d6b44620001b04c706["ekos-dbt-gen"]
    nabd31cd9b31d54c587cd8a4a5b9a30a0["ekos"]
    nabd31cd9b31d54c587cd8a4a5b9a30a0 -->|DependsOn| n9b66a043a00958d6b44620001b04c706
    n7e3bc0ded88855cdaa9af333f6e2cbb2["ekos-kir"]
    n9b66a043a00958d6b44620001b04c706 -->|DependsOn| n7e3bc0ded88855cdaa9af333f6e2cbb2
    n02548eee64785324851f3a6329ccfeca["serde_json"]
    n9b66a043a00958d6b44620001b04c706 -->|DependsOn| n02548eee64785324851f3a6329ccfeca
```

## Evidence

- `27b8e9d8-ae6d-4dc4-a0fb-cd677d00db6d` — ekos depends on ekos-dbt-gen (path dependency) (confidence: 1.00)
- `0202851b-1b0f-4267-9815-811b0264b33d` — ekos-dbt-gen depends on ekos-kir (path dependency) (confidence: 1.00)
- `ff553ab7-cc9c-4608-8640-eec59ec28f9d` — ekos-dbt-gen depends on serde_json 1 (confidence: 1.00)
