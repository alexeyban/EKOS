# DefaultResolver::score (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | method |

## Relationships

### Calls

- → structural_score (`ca326423-f662-5267-86b4-e5c13edbb31e`)
- ← DefaultResolver::resolve (`3372ccc3-1263-52f4-88b9-a8efc6cfa069`)

### Contains

- ← ekos/crates/identity/src/lib.rs (`c958282a-6d42-50ab-9cf9-8533976f0820`)

## Diagram

```mermaid
graph TD
    nbabffd19f87153c080bded8ef0920e33["DefaultResolver::score"]
    nc958282a6d4250ab9cf98533976f0820["ekos/crates/identity/src/lib.rs"]
    nc958282a6d4250ab9cf98533976f0820 -->|Contains| nbabffd19f87153c080bded8ef0920e33
    nca326423f662526786b4e5c13edbb31e["structural_score"]
    nbabffd19f87153c080bded8ef0920e33 -->|Calls| nca326423f662526786b4e5c13edbb31e
    n3372ccc3126352f488b9a8efc6cfa069["DefaultResolver::resolve"]
    n3372ccc3126352f488b9a8efc6cfa069 -->|Calls| nbabffd19f87153c080bded8ef0920e33
```

## Evidence

_No evidence cited._
