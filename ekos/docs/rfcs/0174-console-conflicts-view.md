# RFC 0174 — Console conflicts view

| | |
|---|---|
| **Status** | Implemented (2026-10-11) |
| **Depends on** | RFC 0172 (ConflictingEvidence), RFC 0127/0131 (web console, role split), RFC 0133 (graph view) |

## Problem

A conflict can only be settled on the command line: `ekos conflicts resolve <id> --pick <n> | --both-valid`. The person who knows which source is right is often a reviewer who uses the web console, where business-semantics hypotheses are already reviewed. The console shows nothing about conflicts, and the graph draws a disputed table like any other.

## Design

**A Conflicts tab**, per workspace, beside Semantics.

- A list of the current conflicts, open by default, filterable by status. Each row shows the type, the name (`subject.attribute`), the status and the claimed values side by side.
- Selecting one shows what it is about, what EKOS kept, the measured row count for a documentation-vs-data conflict, and every claim with its value, `path:line` and source.
- A write-role user decides: "Claim *n* is right" (`--pick=n`) or "Both are valid" (`--both-valid`, needs a note). A read-role user sees everything and no decision controls.

**A ring in the graph.** At object level, a node whose object is the subject of an open conflict gets an amber ring, and its tooltip says that sources disagree. The graph page reads the open conflicts once; if that read fails it draws no rings.

## API

| Route | Role | Runs |
|---|---|---|
| `GET /workspaces/{id}/conflicts?status=&type=` | read | `ekos conflicts list --json [--status s] [--type t]`, added to the read-only allowlist |
| `POST /workspaces/{id}/conflicts/{conflict_id}/resolve` | write | `ekos conflicts resolve <id> --as=<who> --pick=<n> \| --both-valid [--note=…]` |

The decision route follows the Semantics tab's rules (RFC 0170 Phase 4):
- it runs the human-only CLI command as an argv list, never a shell and never MCP;
- the conflict is named by UUID only, and each value rides on its flag with `=`, so no text can become an option;
- the reviewer is the OIDC identity, or `token:<typed name>` in token mode;
- a held ledger lock answers 409, a CLI refusal 400.

## Not in this RFC

- Rings on aggregate (kind-level) nodes.
- A link from a conflict to its subject in the graph, and from `ObjectPanel` to the conflict.

## Tests

- API: only the list is on the read allowlist; the resolve argv for both decisions and each rejected shape; the list needs read and a decision needs write and a reviewer; a CLI refusal is 400 and a held lock 409.
- UI: value and location formatting; open conflicts sort first and only their subjects are ringed; the list call and the claims shown; a reader gets no decision controls; a pick and a both-valid decision send the right body.
