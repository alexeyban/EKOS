# render_sequence_diagrams (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- → is_feeds_into (`41dc0e3d-c14a-5c12-a09f-6f9c44fbff80`)
- → sequence_participant_line (`6e251f6e-6278-5d8b-bc16-43cd31f3ea0f`)
- → transform_node_origin (`3102e065-0ad9-5ba4-9213-90dfce5c8300`)
- → render_call_sequences_section (`fbb1d6fe-cedd-5ae7-adea-bdbf8ff8247e`)

### Contains

- ← ekos/crates/docs-gen/src/lib.rs (`6ca4fba0-18e5-59b7-9876-7dae72e2ce0e`)

## Diagram

```mermaid
graph TD
    n76f24b817126591abfa9c22e46862167["render_sequence_diagrams"]
    n6ca4fba018e559b798767dae72e2ce0e["ekos/crates/docs-gen/src/lib.rs"]
    n6ca4fba018e559b798767dae72e2ce0e -->|Contains| n76f24b817126591abfa9c22e46862167
    n41dc0e3dc14a5c12a09f6f9c44fbff80["is_feeds_into"]
    n76f24b817126591abfa9c22e46862167 -->|Calls| n41dc0e3dc14a5c12a09f6f9c44fbff80
    n6e251f6e62785d8bbc1643cd31f3ea0f["sequence_participant_line"]
    n76f24b817126591abfa9c22e46862167 -->|Calls| n6e251f6e62785d8bbc1643cd31f3ea0f
    n3102e0650ad95ba4921390dfce5c8300["transform_node_origin"]
    n76f24b817126591abfa9c22e46862167 -->|Calls| n3102e0650ad95ba4921390dfce5c8300
    nfbb1d6fecedd5ae7adeabdbf8ff8247e["render_call_sequences_section"]
    n76f24b817126591abfa9c22e46862167 -->|Calls| nfbb1d6fecedd5ae7adeabdbf8ff8247e
```

## Evidence

_No evidence cited._
