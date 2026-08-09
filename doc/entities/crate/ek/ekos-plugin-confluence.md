# ekos-plugin-confluence (Crate)

## Properties

| Key | Value |
|---|---|
| `description` | Confluence page observer plugin (proof-of-concept — see RFC 0022) |
| `path` | ekos/plugins/confluence |

## Relationships

### DependsOn

- ← ekos (`abd31cd9-b31d-54c5-87cd-8a4a5b9a30a0`) — evidence: ekos depends on ekos-plugin-confluence (path dependency)
- → async-trait (`7c905290-824b-57e0-a559-15ff1499b4d6`) — evidence: ekos-plugin-confluence depends on async-trait 0.1
- → ekos-artifact (`8806bf54-6364-5052-b85a-cef4344e8f19`) — evidence: ekos-plugin-confluence depends on ekos-artifact (path dependency)
- → ekos-common (`dc169f0a-98f1-5c7c-8dd0-1dbc8504e9c9`) — evidence: ekos-plugin-confluence depends on ekos-common (path dependency)
- → ekos-observation-sdk (`9a955a3a-55bc-587a-942d-fb81d6260052`) — evidence: ekos-plugin-confluence depends on ekos-observation-sdk (path dependency)
- → reqwest (`70acdf50-6295-5f0c-9157-cb9866d0ec23`) — evidence: ekos-plugin-confluence depends on reqwest 0.12
- → serde_json (`02548eee-6478-5324-851f-3a6329ccfeca`) — evidence: ekos-plugin-confluence depends on serde 1
- → thiserror (`bbefe8a3-f76e-509f-910b-b439cd7eeff6`) — evidence: ekos-plugin-confluence depends on thiserror 2
- → tokio (`4a4e8d5c-5102-5d1d-addd-d7fb0bc8d7b9`) — evidence: ekos-plugin-confluence depends on tokio 1
- → tracing (`4282d266-2850-5d04-a992-0f0a204788aa`) — evidence: ekos-plugin-confluence depends on tracing 0.1

## Diagram

```mermaid
graph TD
    ne8d1a3c9e7b25084bdfc569e7b604054["ekos-plugin-confluence"]
    nabd31cd9b31d54c587cd8a4a5b9a30a0["ekos"]
    nabd31cd9b31d54c587cd8a4a5b9a30a0 -->|DependsOn| ne8d1a3c9e7b25084bdfc569e7b604054
    n7c905290824b57e0a55915ff1499b4d6["async-trait"]
    ne8d1a3c9e7b25084bdfc569e7b604054 -->|DependsOn| n7c905290824b57e0a55915ff1499b4d6
    n8806bf5463645052b85acef4344e8f19["ekos-artifact"]
    ne8d1a3c9e7b25084bdfc569e7b604054 -->|DependsOn| n8806bf5463645052b85acef4344e8f19
    ndc169f0a98f15c7c8dd01dbc8504e9c9["ekos-common"]
    ne8d1a3c9e7b25084bdfc569e7b604054 -->|DependsOn| ndc169f0a98f15c7c8dd01dbc8504e9c9
    n9a955a3a55bc587a942dfb81d6260052["ekos-observation-sdk"]
    ne8d1a3c9e7b25084bdfc569e7b604054 -->|DependsOn| n9a955a3a55bc587a942dfb81d6260052
    n70acdf5062955f0c9157cb9866d0ec23["reqwest"]
    ne8d1a3c9e7b25084bdfc569e7b604054 -->|DependsOn| n70acdf5062955f0c9157cb9866d0ec23
    n02548eee64785324851f3a6329ccfeca["serde_json"]
    ne8d1a3c9e7b25084bdfc569e7b604054 -->|DependsOn| n02548eee64785324851f3a6329ccfeca
    nbbefe8a3f76e509f910bb439cd7eeff6["thiserror"]
    ne8d1a3c9e7b25084bdfc569e7b604054 -->|DependsOn| nbbefe8a3f76e509f910bb439cd7eeff6
    n4a4e8d5c51025d1dadddd7fb0bc8d7b9["tokio"]
    ne8d1a3c9e7b25084bdfc569e7b604054 -->|DependsOn| n4a4e8d5c51025d1dadddd7fb0bc8d7b9
    n4282d26628505d04a9920f0a204788aa["tracing"]
    ne8d1a3c9e7b25084bdfc569e7b604054 -->|DependsOn| n4282d26628505d04a9920f0a204788aa
```

## Evidence

- `8c03991c-c092-4d78-9de6-907b3e0f13dd` — ekos depends on ekos-plugin-confluence (path dependency) (confidence: 1.00)
- `07996fba-3646-413d-b48e-e69e64c04602` — ekos-plugin-confluence depends on async-trait 0.1 (confidence: 1.00)
- `6c5cb0f2-acaa-449f-b3ee-0f9afec981fc` — ekos-plugin-confluence depends on ekos-artifact (path dependency) (confidence: 1.00)
- `abc5e3da-90d7-4fa1-a1e3-99b11d75aedf` — ekos-plugin-confluence depends on ekos-common (path dependency) (confidence: 1.00)
- `a5c71b71-e32b-48f4-b823-bc9ba20a4d00` — ekos-plugin-confluence depends on ekos-observation-sdk (path dependency) (confidence: 1.00)
- `4876f5a7-d44a-4117-9047-9f24d780fdcc` — ekos-plugin-confluence depends on reqwest 0.12 (confidence: 1.00)
- `a34508d1-56f4-42ab-b546-6002dc7d247e` — ekos-plugin-confluence depends on serde 1 (confidence: 1.00)
- `cdaffc50-3d5e-4629-bde8-342c1be8a7e0` — ekos-plugin-confluence depends on serde_json 1 (confidence: 1.00)
- `b87db043-49b2-4c35-9bc3-584494d66e58` — ekos-plugin-confluence depends on thiserror 2 (confidence: 1.00)
- `c2962c3b-e15a-4b01-8350-758c05e97bae` — ekos-plugin-confluence depends on tokio 1 (confidence: 1.00)
- `ca38d654-3b59-4534-9825-cb37d6455812` — ekos-plugin-confluence depends on tracing 0.1 (confidence: 1.00)
