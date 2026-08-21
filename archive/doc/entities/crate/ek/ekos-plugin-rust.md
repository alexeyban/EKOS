# ekos-plugin-rust (Crate)

## Properties

| Key | Value |
|---|---|
| `description` | Rust (.rs) observer plugin (RFC 0041) |
| `path` | ekos/plugins/rust |

## Relationships

### DependsOn

- ← ekos (`abd31cd9-b31d-54c5-87cd-8a4a5b9a30a0`) — evidence: ekos depends on ekos-plugin-rust (path dependency)
- → async-trait (`7c905290-824b-57e0-a559-15ff1499b4d6`) — evidence: ekos-plugin-rust depends on async-trait 0.1
- → ekos-artifact (`8806bf54-6364-5052-b85a-cef4344e8f19`) — evidence: ekos-plugin-rust depends on ekos-artifact (path dependency)
- → ekos-common (`dc169f0a-98f1-5c7c-8dd0-1dbc8504e9c9`) — evidence: ekos-plugin-rust depends on ekos-common (path dependency)
- → ekos-observation-sdk (`9a955a3a-55bc-587a-942d-fb81d6260052`) — evidence: ekos-plugin-rust depends on ekos-observation-sdk (path dependency)
- → hex (`fa785806-31c3-5308-ae4a-2898a2c181ca`) — evidence: ekos-plugin-rust depends on hex 0.4
- → serde_json (`02548eee-6478-5324-851f-3a6329ccfeca`) — evidence: ekos-plugin-rust depends on serde_json 1
- → sha2 (`64d6fba9-eea7-52e6-9b3a-b2e887a6944f`) — evidence: ekos-plugin-rust depends on sha2 0.10
- → thiserror (`bbefe8a3-f76e-509f-910b-b439cd7eeff6`) — evidence: ekos-plugin-rust depends on thiserror 2
- → tokio (`4a4e8d5c-5102-5d1d-addd-d7fb0bc8d7b9`) — evidence: ekos-plugin-rust depends on tokio 1
- → tracing (`4282d266-2850-5d04-a992-0f0a204788aa`) — evidence: ekos-plugin-rust depends on tracing 0.1
- → walkdir (`40b5029d-b6e8-55ee-bc22-14411e3d0fb2`) — evidence: ekos-plugin-rust depends on walkdir 2

## Diagram

```mermaid
graph TD
    n07179babd4865b148c686e743e45b3f6["ekos-plugin-rust"]
    nabd31cd9b31d54c587cd8a4a5b9a30a0["ekos"]
    nabd31cd9b31d54c587cd8a4a5b9a30a0 -->|DependsOn| n07179babd4865b148c686e743e45b3f6
    n7c905290824b57e0a55915ff1499b4d6["async-trait"]
    n07179babd4865b148c686e743e45b3f6 -->|DependsOn| n7c905290824b57e0a55915ff1499b4d6
    n8806bf5463645052b85acef4344e8f19["ekos-artifact"]
    n07179babd4865b148c686e743e45b3f6 -->|DependsOn| n8806bf5463645052b85acef4344e8f19
    ndc169f0a98f15c7c8dd01dbc8504e9c9["ekos-common"]
    n07179babd4865b148c686e743e45b3f6 -->|DependsOn| ndc169f0a98f15c7c8dd01dbc8504e9c9
    n9a955a3a55bc587a942dfb81d6260052["ekos-observation-sdk"]
    n07179babd4865b148c686e743e45b3f6 -->|DependsOn| n9a955a3a55bc587a942dfb81d6260052
    nfa78580631c35308ae4a2898a2c181ca["hex"]
    n07179babd4865b148c686e743e45b3f6 -->|DependsOn| nfa78580631c35308ae4a2898a2c181ca
    n02548eee64785324851f3a6329ccfeca["serde_json"]
    n07179babd4865b148c686e743e45b3f6 -->|DependsOn| n02548eee64785324851f3a6329ccfeca
    n64d6fba9eea752e69b3ab2e887a6944f["sha2"]
    n07179babd4865b148c686e743e45b3f6 -->|DependsOn| n64d6fba9eea752e69b3ab2e887a6944f
    nbbefe8a3f76e509f910bb439cd7eeff6["thiserror"]
    n07179babd4865b148c686e743e45b3f6 -->|DependsOn| nbbefe8a3f76e509f910bb439cd7eeff6
    n4a4e8d5c51025d1dadddd7fb0bc8d7b9["tokio"]
    n07179babd4865b148c686e743e45b3f6 -->|DependsOn| n4a4e8d5c51025d1dadddd7fb0bc8d7b9
    n4282d26628505d04a9920f0a204788aa["tracing"]
    n07179babd4865b148c686e743e45b3f6 -->|DependsOn| n4282d26628505d04a9920f0a204788aa
    n40b5029db6e855eebc2214411e3d0fb2["walkdir"]
    n07179babd4865b148c686e743e45b3f6 -->|DependsOn| n40b5029db6e855eebc2214411e3d0fb2
```

## Evidence

- `02a052b0-59b5-4705-83c6-ce253ecf44a7` — ekos depends on ekos-plugin-rust (path dependency) (confidence: 1.00)
- `033866f0-4fdc-4ddb-b9d6-e9b0827fcb50` — ekos-plugin-rust depends on async-trait 0.1 (confidence: 1.00)
- `b299e116-d2f8-4787-933d-9aa7cc6d30e6` — ekos-plugin-rust depends on ekos-artifact (path dependency) (confidence: 1.00)
- `1abbb2b8-786a-490d-a2b7-1601cd39f37a` — ekos-plugin-rust depends on ekos-common (path dependency) (confidence: 1.00)
- `3045e192-8817-46e3-ab0b-d04348ceefda` — ekos-plugin-rust depends on ekos-observation-sdk (path dependency) (confidence: 1.00)
- `bf926f59-fb01-4747-bb73-5b312bffce4a` — ekos-plugin-rust depends on hex 0.4 (confidence: 1.00)
- `9443b98d-1a93-4fda-92d4-0d1351b22807` — ekos-plugin-rust depends on serde_json 1 (confidence: 1.00)
- `eadf547a-990c-46e3-9e1d-c7e27c31a3d5` — ekos-plugin-rust depends on sha2 0.10 (confidence: 1.00)
- `9f5570a7-1896-4bf4-b7a1-8b7aea27d294` — ekos-plugin-rust depends on thiserror 2 (confidence: 1.00)
- `faff6884-c7e3-4a95-b578-232626d5baba` — ekos-plugin-rust depends on tokio 1 (confidence: 1.00)
- `fb5f7dc5-7134-45a0-8c40-a49dadd81131` — ekos-plugin-rust depends on tracing 0.1 (confidence: 1.00)
- `a57e3a07-a9fb-4da4-b487-b2607b57e778` — ekos-plugin-rust depends on walkdir 2 (confidence: 1.00)
