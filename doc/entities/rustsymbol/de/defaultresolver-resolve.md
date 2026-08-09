# DefaultResolver::resolve (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | method |

## Relationships

### Calls

- → DefaultResolver::score (`babffd19-f871-53c0-80bd-ed8ef0920e33`)
- → UnionFind::new (`05a77ab3-acf0-5cc3-bd01-47e0d0e8d612`)
- → DefaultResolver::threshold_for (`49b695e3-042f-57e0-9d1a-f94a899ecc0b`)
- → UnionFind::union (`a78faae3-7f4c-5699-95bf-ef8c6cf855a5`)
- → UnionFind::find (`023832e4-adcc-5b45-b48d-5c2f435f0546`)

### Contains

- ← ekos/crates/identity/src/lib.rs (`c958282a-6d42-50ab-9cf9-8533976f0820`)

## Diagram

```mermaid
graph TD
    n3372ccc3126352f488b9a8efc6cfa069["DefaultResolver::resolve"]
    nc958282a6d4250ab9cf98533976f0820["ekos/crates/identity/src/lib.rs"]
    nc958282a6d4250ab9cf98533976f0820 -->|Contains| n3372ccc3126352f488b9a8efc6cfa069
    nbabffd19f87153c080bded8ef0920e33["DefaultResolver::score"]
    n3372ccc3126352f488b9a8efc6cfa069 -->|Calls| nbabffd19f87153c080bded8ef0920e33
    n05a77ab3acf05cc3bd0147e0d0e8d612["UnionFind::new"]
    n3372ccc3126352f488b9a8efc6cfa069 -->|Calls| n05a77ab3acf05cc3bd0147e0d0e8d612
    n49b695e3042f57e09d1af94a899ecc0b["DefaultResolver::threshold_for"]
    n3372ccc3126352f488b9a8efc6cfa069 -->|Calls| n49b695e3042f57e09d1af94a899ecc0b
    na78faae37f4c569995bfef8c6cf855a5["UnionFind::union"]
    n3372ccc3126352f488b9a8efc6cfa069 -->|Calls| na78faae37f4c569995bfef8c6cf855a5
    n023832e4adcc5b45b48d5c2f435f0546["UnionFind::find"]
    n3372ccc3126352f488b9a8efc6cfa069 -->|Calls| n023832e4adcc5b45b48d5c2f435f0546
```

## Evidence

_No evidence cited._
