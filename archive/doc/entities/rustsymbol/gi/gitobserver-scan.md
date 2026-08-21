# GitObserver::scan (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | method |

## Relationships

### Calls

- → git_output (`4fea6a2a-9edd-5378-abd5-b37ded47b483`)
- → parse_stat_summary (`2c446572-839f-5362-9f05-99e568697d62`)
- → is_git_repo (`cded5a1e-14f6-59b0-80ab-c13dc8216049`)

### Contains

- ← ekos/plugins/git/src/lib.rs (`8941bcba-6474-5c7b-af9e-97dc4f4f7a13`)

## Diagram

```mermaid
graph TD
    nef656cf7720b5f34ba8aa732bfe51c32["GitObserver::scan"]
    n8941bcba64745c7baf9e97dc4f4f7a13["ekos/plugins/git/src/lib.rs"]
    n8941bcba64745c7baf9e97dc4f4f7a13 -->|Contains| nef656cf7720b5f34ba8aa732bfe51c32
    n4fea6a2a9edd5378abd5b37ded47b483["git_output"]
    nef656cf7720b5f34ba8aa732bfe51c32 -->|Calls| n4fea6a2a9edd5378abd5b37ded47b483
    n2c446572839f53629f0599e568697d62["parse_stat_summary"]
    nef656cf7720b5f34ba8aa732bfe51c32 -->|Calls| n2c446572839f53629f0599e568697d62
    ncded5a1e14f659b080abc13dc8216049["is_git_repo"]
    nef656cf7720b5f34ba8aa732bfe51c32 -->|Calls| ncded5a1e14f659b080abc13dc8216049
```

## Evidence

_No evidence cited._
