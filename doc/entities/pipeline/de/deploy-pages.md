# Deploy Pages (Pipeline)

## Properties

| Key | Value |
|---|---|
| `jobs` | [{"name":"deploy","steps":["actions/checkout@v7","actions/upload-pages-artifact@v3","actions/deploy-pages@v4"]}] |
| `path` | .github/workflows/pages.yml |
| `triggers` | ["push","workflow_dispatch"] |

## Relationships

_No compiled relationships touch this object._

## Diagram

_No relationships to diagram._

## Evidence

- `8b3ef68d-34c7-462b-a038-cf9273323524` — workflow definition at .github/workflows/pages.yml (confidence: 1.00)
