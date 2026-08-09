# SegmentStore::verify_sealed (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | method |

## Relationships

### Calls

- → segment_path (`a07b97cc-69e4-5c35-bedf-11119542af72`)
- → hash_file (`a95eaa64-3902-5bdf-905e-c2436d2c8cc4`)

### Contains

- ← ekos/crates/ledger/src/segment/mod.rs (`80a64e0a-b0ac-51df-b3be-128cddd1cc83`)

## Diagram

```mermaid
graph TD
    n2ac5591d1ae55ee28e3244c92f6dae0d["SegmentStore::verify_sealed"]
    n80a64e0ab0ac51dfb3be128cddd1cc83["ekos/crates/ledger/src/segment/mod.rs"]
    n80a64e0ab0ac51dfb3be128cddd1cc83 -->|Contains| n2ac5591d1ae55ee28e3244c92f6dae0d
    na07b97cc69e45c35bedf11119542af72["segment_path"]
    n2ac5591d1ae55ee28e3244c92f6dae0d -->|Calls| na07b97cc69e45c35bedf11119542af72
    na95eaa6439025bdf905ec2436d2c8cc4["hash_file"]
    n2ac5591d1ae55ee28e3244c92f6dae0d -->|Calls| na95eaa6439025bdf905ec2436d2c8cc4
```

## Evidence

_No evidence cited._
