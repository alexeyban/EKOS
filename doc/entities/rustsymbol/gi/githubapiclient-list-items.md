# GitHubApiClient::list_items (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | method |

## Relationships

### Calls

- → GitHubApiClient::list_files (`a59fcb98-efa8-585f-adae-1552e1d6be08`)
- → GitHubApiClient::request (`a0b8024c-32d8-5121-8adb-250994e1cd0f`)

### Contains

- ← ekos/plugins/github/src/lib.rs (`85954c06-2c57-57ee-8a60-8a92c239ab70`)

## Diagram

```mermaid
graph TD
    n28d915f2b5a351e18f928a7e80a23664["GitHubApiClient::list_items"]
    n85954c062c5757ee8a608a92c239ab70["ekos/plugins/github/src/lib.rs"]
    n85954c062c5757ee8a608a92c239ab70 -->|Contains| n28d915f2b5a351e18f928a7e80a23664
    na59fcb98efa8585fadae1552e1d6be08["GitHubApiClient::list_files"]
    n28d915f2b5a351e18f928a7e80a23664 -->|Calls| na59fcb98efa8585fadae1552e1d6be08
    na0b8024c32d851218adb250994e1cd0f["GitHubApiClient::request"]
    n28d915f2b5a351e18f928a7e80a23664 -->|Calls| na0b8024c32d851218adb250994e1cd0f
```

## Evidence

_No evidence cited._
