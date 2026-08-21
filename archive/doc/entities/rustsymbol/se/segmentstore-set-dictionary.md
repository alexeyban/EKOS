# SegmentStore::set_dictionary (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | method |

## Relationships

### Calls

- → save_manifest (`a67a0f9c-e213-50e7-9821-68cfa9ebf4d4`)
- → build_dict (`2479e892-73dc-5eb6-91fc-6f345d564fdf`)

### Contains

- ← ekos/crates/ledger/src/segment/mod.rs (`80a64e0a-b0ac-51df-b3be-128cddd1cc83`)

## Diagram

```mermaid
graph TD
    n2b91f5c637e454ff9f4b735785f13c04["SegmentStore::set_dictionary"]
    n80a64e0ab0ac51dfb3be128cddd1cc83["ekos/crates/ledger/src/segment/mod.rs"]
    n80a64e0ab0ac51dfb3be128cddd1cc83 -->|Contains| n2b91f5c637e454ff9f4b735785f13c04
    na67a0f9ce21350e7982168cfa9ebf4d4["save_manifest"]
    n2b91f5c637e454ff9f4b735785f13c04 -->|Calls| na67a0f9ce21350e7982168cfa9ebf4d4
    n2479e89273dc5eb691fc6f345d564fdf["build_dict"]
    n2b91f5c637e454ff9f4b735785f13c04 -->|Calls| n2479e89273dc5eb691fc6f345d564fdf
```

## Evidence

_No evidence cited._
