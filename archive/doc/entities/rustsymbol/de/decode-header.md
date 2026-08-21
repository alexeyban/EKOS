# decode_header (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- ← scan_batches_filtered (`cefece15-35d7-567a-889d-48edf2ee6fe1`)
- ← scan_headers_slice (`58cd3f37-9374-5ff9-aeea-9fef60615002`)
- ← decode_frame (`ac39fcb4-9dc0-595d-af9b-bce1eb69f50c`)

### Contains

- ← ekos/crates/ledger/src/segment/mod.rs (`80a64e0a-b0ac-51df-b3be-128cddd1cc83`)

## Diagram

```mermaid
graph TD
    nc0caddb822785f35b313f93159e56dbf["decode_header"]
    n80a64e0ab0ac51dfb3be128cddd1cc83["ekos/crates/ledger/src/segment/mod.rs"]
    n80a64e0ab0ac51dfb3be128cddd1cc83 -->|Contains| nc0caddb822785f35b313f93159e56dbf
    ncefece1535d7567a889d48edf2ee6fe1["scan_batches_filtered"]
    ncefece1535d7567a889d48edf2ee6fe1 -->|Calls| nc0caddb822785f35b313f93159e56dbf
    n58cd3f3793745ff9aeea9fef60615002["scan_headers_slice"]
    n58cd3f3793745ff9aeea9fef60615002 -->|Calls| nc0caddb822785f35b313f93159e56dbf
    nac39fcb49dc0595daf9bbce1eb69f50c["decode_frame"]
    nac39fcb49dc0595daf9bbce1eb69f50c -->|Calls| nc0caddb822785f35b313f93159e56dbf
```

## Evidence

_No evidence cited._
