# decode_frame (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | function |

## Relationships

### Calls

- ← scan_batches_filtered (`cefece15-35d7-567a-889d-48edf2ee6fe1`)
- → decode_header (`c0caddb8-2278-5f35-b313-f93159e56dbf`)

### Contains

- ← ekos/crates/ledger/src/segment/mod.rs (`80a64e0a-b0ac-51df-b3be-128cddd1cc83`)

## Diagram

```mermaid
graph TD
    nac39fcb49dc0595daf9bbce1eb69f50c["decode_frame"]
    n80a64e0ab0ac51dfb3be128cddd1cc83["ekos/crates/ledger/src/segment/mod.rs"]
    n80a64e0ab0ac51dfb3be128cddd1cc83 -->|Contains| nac39fcb49dc0595daf9bbce1eb69f50c
    ncefece1535d7567a889d48edf2ee6fe1["scan_batches_filtered"]
    ncefece1535d7567a889d48edf2ee6fe1 -->|Calls| nac39fcb49dc0595daf9bbce1eb69f50c
    nc0caddb822785f35b313f93159e56dbf["decode_header"]
    nac39fcb49dc0595daf9bbce1eb69f50c -->|Calls| nc0caddb822785f35b313f93159e56dbf
```

## Evidence

_No evidence cited._
