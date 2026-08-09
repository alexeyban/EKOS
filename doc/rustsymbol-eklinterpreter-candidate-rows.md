# EklInterpreter::candidate_rows (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | method |

## Relationships

### Calls

- ← EklInterpreter::execute (`feb95d3d-5916-525d-86e3-ad4cee4ff906`)
- → EklInterpreter::expand_from_anchor (`45d98e4b-c62c-5838-b3ac-7fd7122795a1`)
- → EklInterpreter::resolve_anchor (`822748c3-c82a-526c-993b-53255d03a372`)

### Contains

- ← ekos/crates/ekl/src/interpreter.rs (`9c2cb6e4-ee09-503f-8cf5-ccfaf23ecd79`)

## Diagram

```mermaid
graph TD
    n9f9ddd90331357729e6b1a6c3050ad11["EklInterpreter::candidate_rows"]
    n9c2cb6e4ee09503f8cf5ccfaf23ecd79["ekos/crates/ekl/src/interpreter.rs"]
    n9c2cb6e4ee09503f8cf5ccfaf23ecd79 -->|Contains| n9f9ddd90331357729e6b1a6c3050ad11
    nfeb95d3d5916525d86e3ad4cee4ff906["EklInterpreter::execute"]
    nfeb95d3d5916525d86e3ad4cee4ff906 -->|Calls| n9f9ddd90331357729e6b1a6c3050ad11
    n45d98e4bc62c5838b3ac7fd7122795a1["EklInterpreter::expand_from_anchor"]
    n9f9ddd90331357729e6b1a6c3050ad11 -->|Calls| n45d98e4bc62c5838b3ac7fd7122795a1
    n822748c3c82a526c993b53255d03a372["EklInterpreter::resolve_anchor"]
    n9f9ddd90331357729e6b1a6c3050ad11 -->|Calls| n822748c3c82a526c993b53255d03a372
```

## Evidence

_No evidence cited._
