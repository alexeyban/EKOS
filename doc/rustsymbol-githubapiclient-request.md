# GitHubApiClient::request (RustSymbol)

## Properties

| Key | Value |
|---|---|
| `kind` | method |

## Relationships

### Calls

- ← GitHubApiClient::list_files (`a59fcb98-efa8-585f-adae-1552e1d6be08`)
- ← GitHubApiClient::list_items (`28d915f2-b5a3-51e1-8f92-8a7e80a23664`)

### Contains

- ← ekos/plugins/github/src/lib.rs (`85954c06-2c57-57ee-8a60-8a92c239ab70`)

## Diagram

```mermaid
graph TD
    na0b8024c32d851218adb250994e1cd0f["GitHubApiClient::request"]
    n85954c062c5757ee8a608a92c239ab70["ekos/plugins/github/src/lib.rs"]
    n85954c062c5757ee8a608a92c239ab70 -->|Contains| na0b8024c32d851218adb250994e1cd0f
    na59fcb98efa8585fadae1552e1d6be08["GitHubApiClient::list_files"]
    na59fcb98efa8585fadae1552e1d6be08 -->|Calls| na0b8024c32d851218adb250994e1cd0f
    n28d915f2b5a351e18f928a7e80a23664["GitHubApiClient::list_items"]
    n28d915f2b5a351e18f928a7e80a23664 -->|Calls| na0b8024c32d851218adb250994e1cd0f
```

## Evidence

_No evidence cited._
