# CI (Pipeline)

## Properties

| Key | Value |
|---|---|
| `jobs` | [{"name":"build-and-test","steps":["actions/checkout@v7","Install Rust stable","Cache cargo registry","Build (all crates)","Test (unit + integration)","Clippy (no warnings)","Format check"]},{"name":"benchmark","steps":["actions/checkout@v7","Install Rust stable","Cache cargo registry","Run benchmarks","Upload benchmark report"]}] |
| `path` | .github/workflows/ci.yml |
| `triggers` | ["push","pull_request"] |

## Relationships

_No compiled relationships touch this object._

## Diagram

_No relationships to diagram._

## Evidence

- `fef3971d-b69d-4c10-bebc-5559454402b3` — workflow definition at .github/workflows/ci.yml (confidence: 1.00)
