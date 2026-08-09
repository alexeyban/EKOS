# ekos-docs-gen (Crate)

## Properties

| Key | Value |
|---|---|
| `description` | RFC 0035 — Generated Documentation: renders compiled ledger objects as Markdown/HTML/diagrams |
| `path` | ekos/crates/docs-gen |

## Relationships

### DependsOn

- ← ekos (`abd31cd9-b31d-54c5-87cd-8a4a5b9a30a0`) — evidence: ekos depends on ekos-docs-gen (path dependency)
- → ekos-kir (`7e3bc0de-d888-55cd-aa9a-f333f6e2cbb2`) — evidence: ekos-docs-gen depends on ekos-kir (path dependency)
- → serde_json (`02548eee-6478-5324-851f-3a6329ccfeca`) — evidence: ekos-docs-gen depends on serde_json 1

## Diagram

```mermaid
graph TD
    nee66e2d3bd7f53c2a9f97dcb7cba59b3["ekos-docs-gen"]
    nabd31cd9b31d54c587cd8a4a5b9a30a0["ekos"]
    nabd31cd9b31d54c587cd8a4a5b9a30a0 -->|DependsOn| nee66e2d3bd7f53c2a9f97dcb7cba59b3
    n7e3bc0ded88855cdaa9af333f6e2cbb2["ekos-kir"]
    nee66e2d3bd7f53c2a9f97dcb7cba59b3 -->|DependsOn| n7e3bc0ded88855cdaa9af333f6e2cbb2
    n02548eee64785324851f3a6329ccfeca["serde_json"]
    nee66e2d3bd7f53c2a9f97dcb7cba59b3 -->|DependsOn| n02548eee64785324851f3a6329ccfeca
```

## Evidence

- `3b137327-aae5-4fb7-911d-7bf218554325` — ekos depends on ekos-docs-gen (path dependency) (confidence: 1.00)
- `c3cdc77d-380f-4e1f-893d-17b6207b06e8` — ekos-docs-gen depends on ekos-kir (path dependency) (confidence: 1.00)
- `07b738b5-6ebb-4f0f-af2e-618027ac94e6` — ekos-docs-gen depends on serde_json 1 (confidence: 1.00)
