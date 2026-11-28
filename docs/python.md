# EKOS for Python

`ekos` is an embedded knowledge engine for Python: it opens a compiled, evidence-backed EKOS
knowledge ledger and reads it inside your process. There is no server to start and no Rust
toolchain to install.

```bash
pip install ekos
```

Python 3.10 or newer, on Linux (x86_64 and aarch64, glibc 2.28+ or musl), macOS (Apple silicon
and Intel) and Windows x86_64. The wheel has no dependencies.

**Status: provisional (RFC 0269).** The reads below are stable enough to use; names may still
change until the pipeline and remote modes land (phase 2).

The package **reads**. Knowledge enters a ledger by compiling sources with the `ekos` command
(`ekos build`, `recover`, `resolve`, `compile`, `commit`); nothing in this package can add to,
change or delete from a ledger, and there is no review action in it (approving, confirming and
resolving stay with people, in the CLI and the console).

## From nothing to a first answer

```bash
# 1. the compiler (one binary) — see the README for Windows and installers
curl -fsSL https://raw.githubusercontent.com/alexeyban/EKOS/main/install.sh | sh

# 2. compile a repository into a ledger
cd your-repo
ekos init --detect && ekos build && ekos recover && ekos resolve && ekos compile && ekos commit

# 3. read it from Python
pip install ekos
```

```python
import ekos

kb = ekos.open(".")
for hit in kb.search("orders", limit=5):
    state = kb.get(hit.id)
    print(hit.name, state.object.kind, [e.path for e in state.evidence])
```

Runnable scripts are in [`python/examples`](../python/examples): a quickstart, an impact report,
a cited context block for a language model, and a schema overview with asyncio.

To build the package from a checkout instead: `cd python && uvx maturin build --release` (needs
Rust), then install the wheel from `ekos/target/wheels/`.

## Open a ledger

```python
import ekos

kb = ekos.open("~/work/shop")                    # a workspace: the directory with `.ekos/`
kb = ekos.open(project="shop")                   # a project in the EKOS home, as `ekos --project shop`
kb = ekos.open(project="erp", tenant="acme")     # home: `home=`, then EKOS_HOME, then ~/.ekos
```

- An open never creates anything. A place with no compiled ledger raises `ekos.LedgerError`.
- A handle is a **snapshot** of the ledger at the moment it was opened. `kb.refresh()` folds in
  what another process has committed since and returns whether anything changed. Reads never
  refresh on their own, so two reads in a row agree.
- One handle serves one call at a time; a second thread waits. The interpreter lock is released
  while a call runs. Separate handles on the same ledger are independent.
- A ledger kept in SQLite (a workspace from before the fact engine) is refused with
  `ekos.NotSupported`: convert it once with `ekos ledger migrate --v3`. A project that reads from
  a distributed cluster is refused the same way.
- If the search index was written by another EKOS version, the open emits a `UserWarning`;
  search still works, over the older text.

## Read

```python
kb.status()                                   # Status(entries, objects, relationships, ledger_path)

hits = kb.search("payment status", limit=10)  # [Hit(id, name)], best first
state = kb.get(hits[0].id)                    # ObjectState, or None for an unknown id
state.object.kind, state.object.properties
state.relationships                           # [Relationship(kind, from_id, to_id, ...)]
state.evidence                                # [Evidence(path, line, fragment, confidence)]
state.open_conflicts                          # where sources disagree and nobody has decided

kb.facts(id)                                  # [Fact(attribute, value)]
kb.history(id)                                # every version, oldest first
kb.audit_trail(id)                            # every write: run, stage, source artifact

kb.dependents(id)                             # what points at it, one step away
kb.dependencies(id)                           # what it points at
kb.impact(id, direction="dependents", hops=3) # [ImpactHop(hop, id, name, kind, via)]
kb.neighborhood(id, depth=2)                  # Graph(objects, relationships, truncated)

kb.ekl("FIND Object WHERE kind = 'Table' ORDER BY name")   # [dict], one per row
answer = kb.query("which tables reference customers?")     # no model is called
for claim in answer.claims:
    claim.text, claim.value, claim.confidence
    claim.evidence                            # always a list; empty means no evidence record
    claim.disputed, claim.conflicts           # sources disagree about this attribute
kb.retrieve("...")                            # the same answer with its plan and evidence set

kb.diff("2026-10-01", "2026-11-01")           # Diff(changed_total, changed, unchanged)
kb.at("2026-10-01").get(id)                   # the object as it was, or None
kb.graph(kinds=["Table"], max_nodes=500)      # the compact graph export
```

Every result is a frozen dataclass with `to_dict()`. Times are RFC 3339 strings, a `datetime`
(naive means UTC) or a bare date (midnight UTC).

**Evidence is part of every answer.** `ObjectState.evidence` and `Claim.evidence` are always
lists. An empty list is information — the ledger holds no evidence for that item — and the
types make it hard to overlook.

### Vector and hybrid search

```python
kb.search("customers who churned", mode="hybrid", embedding=vector)
```

`mode="vector"` and `"hybrid"` need the query as a vector, from the same model the ledger's
vector index was built with (`[embeddings]` in `ekos.toml`). The package makes no network call,
so it does not compute one for you. Without an index the vector arm does not run and the
lexical one answers.

### Read scope

```python
kb = ekos.open(path, read_scope={"default": "deny", "rules": [{"effect": "allow", "source": "app"}]})
```

The scope is the `--read-scope` format (RFC 0181). Every read is filtered by it. `status()` and
`audit_trail()` cannot be filtered and raise `ekos.NotSupported` for a restricted reader.

### asyncio

```python
import ekos.aio

kb = await ekos.aio.open("~/work/shop")
status, hits = await asyncio.gather(kb.status(), kb.search("orders"))
```

Each call runs on a worker thread, so the event loop is not blocked. `kb.sync` is the blocking
handle underneath.

## Recipes

**What breaks if I change this?**

```python
target = kb.search("customers")[0]
for step in kb.impact(target.id, hops=3):
    print(step.hop, step.kind, step.name, "via", step.via)
```

**Ground a model's prompt in cited facts.** `kb.query` calls no model; it returns claims with the
evidence each rests on. Number them and let the model cite the numbers:

```python
answer = kb.query("which tables reference customers?")
facts = [c for c in answer.claims if not c.weak]
context = "\n".join(
    f"[{n}] {c.text} (source: {c.evidence[0].location if c.evidence else 'none'})"
    + (" [DISPUTED]" if c.disputed else "")
    for n, c in enumerate(facts, 1)
)
```

**Every table with its columns.**

```python
for row in kb.ekl("FIND Object WHERE kind = 'Table' ORDER BY name"):
    table = kb.get(row["id"])
    print(table.object.name, [c["name"] for c in table.object.properties.get("columns", [])])
```

**What changed since last week, and what an object looked like before.**

```python
diff = kb.diff("2026-11-21")
print(diff.changed_total, [c.name for c in diff.changed if c.entity == "Object"][:10])
before = kb.at("2026-11-21").get(object_id)      # None if it did not exist yet
```

**A long-running service.** Open once, keep the handle, and refresh when you want new commits:

```python
kb = ekos.open(project="shop")
...
if kb.refresh():           # another process ran `ekos commit`
    log.info("ledger updated: %s objects", kb.status().objects)
```

## Errors

| Exception | Meaning |
|---|---|
| `ekos.EkosError` | the base of all of them |
| `ekos.LedgerError` | the path, its `ekos.toml` or its store cannot be opened or read |
| `ekos.QueryError` | a wrong argument: a malformed id, an EKL query that does not parse, a bound exceeded |
| `ekos.NotFound` (a `QueryError`) | a well-formed id that names nothing — `get` returns `None` instead |
| `ekos.NotSupported` | this kind of store, or this reader's scope, cannot serve the call |

## The same answers as MCP

Each read is the same Rust function the MCP server runs for the tool of the same name
(`kb.search` ↔ `ekos_search`, `kb.get` ↔ `ekos_state`, `kb.impact` ↔ `ekos_impact`, …), in the
crate `ekos-read`. A test compares the two on one ledger, so a program and an agent see the same
thing.

## Not yet

- Running the pipeline from Python (`ekos.Project`) and connecting to a remote EKOS
  (`ekos.connect`): RFC 0269 phase 2.
- Contributing facts from an application: its own RFC. It will be a source that is compiled, not
  a direct append.
