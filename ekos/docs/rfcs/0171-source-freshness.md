# RFC 0171 — Source freshness: does the ledger still match the source?

| | |
|---|---|
| **Status** | Implemented (2026-10-09) |
| **Depends on** | RFC 0135 Part A (build fingerprints), RFC 0097 (MCP store cache) |

## Problem

The ledger is compiled from a source tree at one moment. The code keeps moving afterwards. Nothing tells a reader when the ledger is behind it.

- `ekos build` keeps a per-observe-path fingerprint, but only as a cache. It skips an unchanged rescan and says nothing. It is per path, not per file, and nobody reads it at query time.
- The MCP server notices when the **ledger** changes (RFC 0097), not when the **source** does.
- `ekos architecture drift`, migrate drift and session-memory staleness each compare ledger against ledger, or repository against a live database. None compares the ledger with the source it came from.

So an agent asking over MCP gets answers from a ledger that may be days behind the code, with no hint that they might be stale.

## Design

**Record what the ledger was compiled from.**
- `ekos build` already walks every observed file to compute its fingerprint. The same walk now also records a per-file manifest: workspace-relative path, size, mtime in nanoseconds, plus `git rev-parse HEAD` when the workspace is a repository. The walk happens once; the fingerprint is computed from the same entries, and its hash is unchanged. The manifest is written to `.ekos/source-manifest.json`.
- `ekos commit`, once it succeeds, copies that file to `.ekos/committed-manifest.json` with `committed_at`. That file is the definition of "what the ledger reflects". It is a derived sidecar, like `semantics/current.json`, so the ledger stays append-only.

**Compare at compile time.** `ekos compile` and `ekos commit` re-walk the metadata. If it differs from `source-manifest.json`, they warn `FRESH001 N file(s) changed since ekos build — run ekos build first`. This is a warning, never a failure.

**Compare at query time.** `freshness::check` re-walks the metadata and compares it with `committed-manifest.json`. The result has:
- a status: `fresh`, `source_changed`, or `unknown` (nothing committed yet, or the workspace predates this RFC);
- the changed, added and removed files, with counts and the first paths;
- the git HEAD then and now.

It is reported in these places:

| Surface | What it shows |
|---|---|
| `ekos status` (text and `--json`) | One line, or a `freshness` object: as of `<committed_at>` (git `abc1234`); N changed, M added, K removed since |
| `ekos doctor` | A "Source freshness" check. It is always `ok`, with the drift in the detail, because a stale ledger is not a broken environment |
| `ekos freshness [--json] [--limit N]` | Every changed, added and removed file, and for each changed or removed file the ledger objects whose evidence cites it ("these facts may be stale") |
| MCP `ekos_status` | A `freshness` block |
| MCP read tools | When the status is `source_changed`, one extra text item after the result: "the ledger may be behind the source: N file(s) changed since the last commit". The result itself is unchanged |

The MCP server memoises the check for `[freshness] ttl-seconds` (default 30), so a busy server does not walk the tree on every call.

**What "changed" means.** Size or mtime differs. No content hash is stored, so a file that was only `touch`ed counts as changed. The CLI output states this rule. Hashing every file on every check would cost more than the check is worth, and an mtime-only false positive is cheap: it only suggests a rebuild.

**Raw-content safety (RFC 0043).** Only file metadata is read, never contents. This adds no raw-content entry point, and nothing needs redacting.

## Configuration

```toml
[freshness]
enabled = true        # default; false turns off every check, the FRESH001 warning and the MCP note
ttl-seconds = 30      # MCP memo
```

## Not in scope

- Remote sources (GitHub issues, Confluence, ClickHouse): only files under `[observe] paths` are compared.
- Auto-rebuild: EKOS reports drift; it never recompiles on its own.
- Per-object staleness inside `ekos_state` and other tool results. `ekos freshness` maps files to objects on request; doing it on every call would mean scanning all evidence.

## Tests

- **Manifest diff:** changed, added and removed, with ordering and caps.
- **Fingerprint:** the hash is unchanged by the refactor.
- **Committed manifest:** it is promoted only by a successful commit.
- **Status:** `unknown` before the first commit.
- **MCP:** the note appears only when the status is `source_changed`, and `ekos_status` carries the block.
- **Config:** the defaults.
