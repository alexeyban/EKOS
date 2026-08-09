# hash_file (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- ← SegmentStore::seal_active (`f2a44d2b-2547-5a57-b0b3-ec7494028286`)
- ← SegmentStore::verify_sealed (`2ac5591d-1ae5-5ee2-8e32-44c92f6dae0d`)

### Contains

- ← ekos/crates/ledger/src/segment/mod.rs (`80a64e0a-b0ac-51df-b3be-128cddd1cc83`)

## Diagram

```mermaid
graph TD
    na95eaa6439025bdf905ec2436d2c8cc4["hash_file"]
    n80a64e0ab0ac51dfb3be128cddd1cc83["ekos/crates/ledger/src/segment/mod.rs"]
    n80a64e0ab0ac51dfb3be128cddd1cc83 -->|Contains| na95eaa6439025bdf905ec2436d2c8cc4
    nf2a44d2b25475a57b0b3ec7494028286["SegmentStore::seal_active"]
    nf2a44d2b25475a57b0b3ec7494028286 -->|Calls| na95eaa6439025bdf905ec2436d2c8cc4
    n2ac5591d1ae55ee28e3244c92f6dae0d["SegmentStore::verify_sealed"]
    n2ac5591d1ae55ee28e3244c92f6dae0d -->|Calls| na95eaa6439025bdf905ec2436d2c8cc4
```

## Evidence

_No evidence cited._
