# Devlog 199 — Testing the session-memory MCP surface found six isolation leaks (RFC 0151)

**Date:** 2026-09-22
**PRs:** none (local `main`, `[skip ci]`)
**Branch:** main

---

## Summary
Ran the RFC 0151 agent-session-memory tools against a real `ekos mcp serve` stdio session rather
than through unit tests. The write/commit/recall/brief/staleness loop and the human-only lifecycle
all held. Isolation did not: the filter that hides `Session`/`SessionClaim` was installed on two of
about twenty `Runtime` read methods, so six agent-facing MCP tools returned unconfirmed agent notes
as ordinary compiled objects, and `ekos_dependents` counted an anchored note as a dependent. Fixed
across every read path plus `export_graph`, with a guard test that fails CI on the next unaudited
read method. Also fixed a `CLAUDE.md` example that no EKL parser has ever accepted.

---

## What the live MCP test covered

Driven over stdio JSON-RPC against a scratch workspace built by the real pipeline
(`init → build → recover → resolve → compile → commit`) with a note anchored to a `Table` that
resolves, so anchors, fingerprints and graph edges are all real rather than mocked.

| check | result |
|---|---|
| `ekos_session_note` writes inbox only | pass — `.ekos/session/inbox/*.jsonl`, `0600`, no ledger handle |
| redaction at the write choke point | pass — `aws_secret_access_key=…` → `[REDACTED:generic-assigned-secret]` |
| caps | pass — 200 notes/session, oversize rejected, `100 dropped` surfaced by `session status` |
| validation | pass — empty text and unknown `kind` rejected with usable messages |
| commit → claim | pass — anchored `SessionClaim`, evidence cites the exact inbox line |
| staleness | pass — `fresh` → `changed` after the anchored table gained a column, `change_summary: "changed: columns"` |
| brief framing | pass — imperative preamble above the untrusted envelope, `[CHANGED]` printed only as a departure |
| negative control | pass — explicit `no_relevant_session_memory` |
| human-only lifecycle | pass — `ekos_identity_review` → *"not a SameAs or AuthorizedBy candidate"*; `ekos_architecture_review` → *"not a role Claim"* |
| `ekos_search` / `ekos_query` / `ekos_retrieve` | pass — claims hidden |
| **everything else that returns an object** | **fail — see below** |

---

## The leak

`is_session_memory` was called at exactly two sites in `crates/runtime/src/lib.rs`:
`find_objects` (:273) and `retrieve` (:305). Every other read method passed session memory straight
through, and six MCP tools sit on those methods:

| tool | via | what an agent saw |
|---|---|---|
| `ekos_ekl` | `Runtime::list_objects` | `FIND Object` listed `SessionClaim` / `Session` rows |
| `ekos_neighborhood` | `load_neighborhood` | the note is a graph neighbour of the table it anchors |
| `ekos_dependents` | `trace_impact` | **`dependents_count: 2`** for a table with one real FK |
| `ekos_impact` | `trace_impact` | the note appears in the blast radius |
| `ekos_state` | `reconstruct_state` | full raw note text by id |
| `ekos_graph_export` | `export_graph` (store directly) | notes render as graph nodes — so the web console graph too |

In every case the note arrived with no tier, no staleness verdict and no untrusted envelope: an
unconfirmed `T0` hypothesis presented exactly like a compiled fact.

### What was built

| component | change |
|---|---|
| `kir::custom_kinds` | `SESSION_RELATIONSHIP_KINDS` + shared `is_session_object_kind` / `is_session_relationship_kind` predicates |
| `runtime/src/lib.rs` | filter in `load_object`, `load_neighborhood`, `trace_impact`, `reconstruct_state{,_at}`, `list_{objects,relationships}{,_at}`, `relationships_for`, `{object,relationship}_history`, `build_world`; `dependencies`/`dependents`/`callers`/`related`/`graph_op` inherit it |
| `runtime/src/graph_export.rs` | its own filter — `export_graph` takes `&dyn KnowledgeStore`, so it never inherited `Runtime`'s |
| tests | 6 new: object paths, relationship paths, graph export, the pre-filter negative case, and the audit guard |

### Decisions

- **Filter in `Runtime`, not in `mcp.rs`.** Stripping the kinds in each MCP tool would have been a
  smaller diff, but the next `Runtime` consumer — `docs-gen`, the web-console API, the eval
  harnesses — would leak again. `export_graph` is exactly that case already: it bypasses `Runtime`
  and so needed its own filter, which is the argument for the chokepoint, not against it.
- **Hide from `ekos_state` too, rather than tag.** A tagged object still reaches the model as a
  normal tool result. The claim's own tool (`ekos_session_recall`) already returns it with tier,
  verdict and envelope, so hiding costs no reachability.
- **An edge is session memory only when an endpoint is.** `AnchoredTo`/`ObservedIn` are emitted by
  nothing else, which makes the kind a sound pre-filter that avoids an object load per edge — but
  the endpoint's real kind decides, so an `AnchoredTo` between two ordinary objects survives.
- **Drop session objects before the export totals**, not just before rendering. A `total_objects`
  that counts notes is a count of something that is not in the graph.

---

## Knowledge Captured

- **A filter installed at "the retrieval path" is not installed at the read surface.** Two of ~20
  `Runtime` methods were filtered, and the RFC, CLAUDE.md and the devlog all described that as
  isolation from "every default answer path". The claim was written about the two methods someone
  had in mind, not about the surface as it exists. When an invariant says *every*, enumerate the
  methods in a test — prose cannot hold a surface that grows.
- **Unit tests confirmed the isolation that existed; only the live MCP session found the isolation
  that didn't.** `session_memory_is_invisible_to_default_retrieval` passed the whole time. It tested
  `find_objects` and `retrieve` — the two places the filter was. A test written against the
  implementation cannot find the paths the implementation forgot; drive the real agent surface.
- **The most dangerous leak was a count, not a text.** `ekos_dependents` returning
  `dependents_count: 2` for a table with one foreign key is wrong in a way that survives being read
  carefully — the note is visibly a note, but the number is just a number, and it feeds impact
  analysis and any "is it safe to remove this" answer.
- **`export_graph` takes `&dyn KnowledgeStore`, not `&Runtime`.** Anything enforced as a `Runtime`
  method is not enforced for it. Worth checking for every future invariant placed on `Runtime`.
- **`AnchoredTo`/`ObservedIn` are session-only kinds**, which is what makes a cheap edge pre-filter
  possible at all. If anything else ever emits them, `is_session_relationship`'s endpoint check is
  what keeps it correct — the kind check is only there to avoid an object load per edge.
- **A guard test must be tested against its negative case.** A throwaway `pub fn
  zzz_probe_new_read_method` was added to confirm the audit guard actually fails, then removed. A
  guard that has only ever passed is not known to guard anything (same lesson as the `headless.sh`
  act-filter bug).
- **`CLAUDE.md:47` documented `ekl "FIND Table WHERE ..."`**, which the parser has never accepted —
  `parse_entity` takes only `Object` and `Relationship`. Every session that trusted the example got
  `unknown entity 'Table'`. The working form is `FIND Object WHERE kind = 'Table'`.
- **Email is deliberately not redacted** (RFC 0043 §100): PII scope is regex-shaped
  secrets/tokens/credentials, because git commit author name/email is extracted on purpose. Session
  notes are free text and so likelier to carry one than source code is — worth knowing before
  someone reports it as a redaction bug.

---

## Files Changed
| File | Change summary |
|---|---|
| `ekos/crates/kir/src/custom_kinds.rs` | `SESSION_RELATIONSHIP_KINDS` + the two shared predicates |
| `ekos/crates/runtime/src/lib.rs` | session filter on every object/relationship-returning read path; `is_session_object`/`is_session_relationship` helpers; 4 new tests incl. the audit guard |
| `ekos/crates/runtime/src/graph_export.rs` | drops session objects and their edges before the totals; 2 new tests |
| `ekos/docs/rfcs/0151-agent-session-memory.md` | new *Isolation scope (revised 2026-09-22)* section; principle 5, the alternatives row and the risk table corrected |
| `CLAUDE.md` | session crate-map row states the real scope; `FIND Table` example fixed |
| `docs/session-memory.md` | new safety-property bullet for the widened guarantee |
