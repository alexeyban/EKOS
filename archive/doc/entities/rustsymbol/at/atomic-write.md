# atomic_write (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- ← save_manifest (`a67a0f9c-e213-50e7-9821-68cfa9ebf4d4`)
- ← write_head (`203436f8-4f5f-5f2b-93a8-b25fd11e5174`)

### Contains

- ← ekos/crates/ledger/src/segment/mod.rs (`80a64e0a-b0ac-51df-b3be-128cddd1cc83`)

## Diagram

```mermaid
graph TD
    n71c4923a8f1e587ca91a37769f53c149["atomic_write"]
    n80a64e0ab0ac51dfb3be128cddd1cc83["ekos/crates/ledger/src/segment/mod.rs"]
    n80a64e0ab0ac51dfb3be128cddd1cc83 -->|Contains| n71c4923a8f1e587ca91a37769f53c149
    na67a0f9ce21350e7982168cfa9ebf4d4["save_manifest"]
    na67a0f9ce21350e7982168cfa9ebf4d4 -->|Calls| n71c4923a8f1e587ca91a37769f53c149
    n203436f84f5f5f2b93a8b25fd11e5174["write_head"]
    n203436f84f5f5f2b93a8b25fd11e5174 -->|Calls| n71c4923a8f1e587ca91a37769f53c149
```

## Evidence

_No evidence cited._
