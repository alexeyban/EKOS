# EklInterpreter::execute (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | method |

## Relationships

### Calls

- → EklInterpreter::candidate_rows (`9f9ddd90-3313-5772-9e6b-1a6c3050ad11`)
- → default_returns (`e8072521-557d-545e-b833-f3d849872856`)
- → project (`990853c7-8608-562b-aea0-df03bfbfaa73`)
- → compare_rows (`91663026-81cb-5c5e-912c-7ffe203d4ed6`)
- → eval_predicate (`db424b5e-3b63-590d-8e63-197b48efa89a`)

### Contains

- ← ekos/crates/ekl/src/interpreter.rs (`9c2cb6e4-ee09-503f-8cf5-ccfaf23ecd79`)

## Diagram

```mermaid
graph TD
    nfeb95d3d5916525d86e3ad4cee4ff906["EklInterpreter::execute"]
    n9c2cb6e4ee09503f8cf5ccfaf23ecd79["ekos/crates/ekl/src/interpreter.rs"]
    n9c2cb6e4ee09503f8cf5ccfaf23ecd79 -->|Contains| nfeb95d3d5916525d86e3ad4cee4ff906
    n9f9ddd90331357729e6b1a6c3050ad11["EklInterpreter::candidate_rows"]
    nfeb95d3d5916525d86e3ad4cee4ff906 -->|Calls| n9f9ddd90331357729e6b1a6c3050ad11
    ne8072521557d545eb833f3d849872856["default_returns"]
    nfeb95d3d5916525d86e3ad4cee4ff906 -->|Calls| ne8072521557d545eb833f3d849872856
    n990853c78608562baea0df03bfbfaa73["project"]
    nfeb95d3d5916525d86e3ad4cee4ff906 -->|Calls| n990853c78608562baea0df03bfbfaa73
    n9166302681cb5c5e912c7ffe203d4ed6["compare_rows"]
    nfeb95d3d5916525d86e3ad4cee4ff906 -->|Calls| n9166302681cb5c5e912c7ffe203d4ed6
    ndb424b5e3b63590d8e63197b48efa89a["eval_predicate"]
    nfeb95d3d5916525d86e3ad4cee4ff906 -->|Calls| ndb424b5e3b63590d8e63197b48efa89a
```

## Evidence

_No evidence cited._
