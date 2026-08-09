# walk_frames (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- ← scan_batches_filtered (`cefece15-35d7-567a-889d-48edf2ee6fe1`)
- ← scan_headers_slice (`58cd3f37-9374-5ff9-aeea-9fef60615002`)

### Contains

- ← ekos/crates/ledger/src/segment/mod.rs (`80a64e0a-b0ac-51df-b3be-128cddd1cc83`)

## Diagram

```mermaid
graph TD
    n630f07e4a40759c6bdb6e7ed592a703b["walk_frames"]
    n80a64e0ab0ac51dfb3be128cddd1cc83["ekos/crates/ledger/src/segment/mod.rs"]
    n80a64e0ab0ac51dfb3be128cddd1cc83 -->|Contains| n630f07e4a40759c6bdb6e7ed592a703b
    ncefece1535d7567a889d48edf2ee6fe1["scan_batches_filtered"]
    ncefece1535d7567a889d48edf2ee6fe1 -->|Calls| n630f07e4a40759c6bdb6e7ed592a703b
    n58cd3f3793745ff9aeea9fef60615002["scan_headers_slice"]
    n58cd3f3793745ff9aeea9fef60615002 -->|Calls| n630f07e4a40759c6bdb6e7ed592a703b
```

## Evidence

_No evidence cited._
