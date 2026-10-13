# RFC 0181 — Read scope: access control inside a project

**Status:** Accepted — implemented 2026-10-13 (devlog_262)
**Author:** EKOS team
**Created:** 2026-10-13
**Builds on:** RFC 0178 (`source` / `sources` on objects), RFC 0180 (roles per tenant and project), RFC 0151 (the session-memory read audit)
**Part of:** production deployment (0178, 0179, 0180, this RFC, then 0182 MCP gateway, 0183 operations)

## Motivation

A project is one ledger over several sources (RFC 0178). RFC 0180 grants roles down to a
project and no further. Two needs are not met:

- a person may see one repository of a project and not another;
- a person may see a project except one directory in it.

One ledger per project means these limits have to be applied when the ledger is read. A limit
that one read path forgets is a leak, and session memory already went through that: six tools
returned what two had been taught to hide.

## Where it is enforced

In a wrapper around the store, `ekos_ledger::scope::ScopedStore`, which implements
`KnowledgeStore` and filters every read. `Runtime`, EKL, graph export and every command take a
`&dyn KnowledgeStore`; given the wrapper they cannot return what it hides. Filtering in each
`Runtime` method would leave out everything that reads the store directly.

An unrestricted scope passes every call through unchanged.

## The scope

```json
{
  "default": "deny",
  "rules": [
    {"effect": "allow", "source": "payments-api"},
    {"effect": "deny",  "source": "payments-api", "path": "secrets/**"},
    {"effect": "allow", "object": "692f6b3a-52d1-58fc-ad6a-ed624ef3d86f"}
  ]
}
```

A rule names any of: a **source**, a **path** pattern inside a source (`*` within one path
component, `**` across components), an **object** id. `default` is `allow` or `deny`; a scope
with `default: allow` and no rule is unrestricted.

### What an object comes from

An object's **origins** are the pairs (source, path): its `sources` (RFC 0178) crossed with the
`location.path` of each of its evidence records. An object with no source has the origin source
"none"; one with no evidence path has the origin path "none". Every object has at least one
origin.

### The decision

An object is visible when it is **not denied** and it is **allowed**.

- **Denied** when a deny rule names its id, or matches any one of its origins. A deny rule
  matches an origin whose source or path is "none": what cannot be placed is treated as inside.
  A reader denied one directory therefore also loses every object the compiler could not
  attribute, such as a commit that lists the files it changed.
- **Allowed** when `default` is `allow`, or an allow rule names its id, or **every** origin is
  matched by some allow rule. An allow rule with a source does not match source "none"; an
  allow rule with a path does not match path "none".

So deny always wins, an object found in two sources needs both, and an object the compiler
could not attribute is hidden from a reader limited by source.

### Everything else follows the object

| Read | Rule |
|---|---|
| Object, any version, any point in time | the decision above, on that version |
| Relationship | both ends visible |
| Evidence record | returned only for an id listed by an object already found visible |
| Event | hidden |
| Search and retrieval | hits are filtered; the store is asked for more than the limit so a filtered page is not short |
| A single fact, an audit trail | follows the object |
| Counts | computed over what is visible |
| Diff | touched ids filtered; entry ids withheld |
| Copy of the ledger (`vacuum_into`), any write | refused |

### Known limits of the decision

- Path rules rely on evidence paths. They are source-relative file paths for everything
  compiled from a file (checked for `File`, `Table`, `Document`, `Section` and code symbols in
  the leak suite). A kind whose evidence does not name its file is matched only by source.
- Objects with no source are hidden from a restricted reader: commit authors (`Person`),
  crates, rollups, business-semantics items, conflicts, risks. The Semantics and Conflicts tabs
  are empty for such a reader.
- Rules hide objects. They do not rewrite the text of a visible document that mentions a hidden
  one.

## Passing a scope

- **CLI:** `--read-scope <file>` (JSON, or `EKOS_READ_SCOPE`) on `ekos mcp serve`, `ekos ekl`
  and `ekos query`. Any other command refuses the flag: those commands also read files beside
  the ledger, which no store wrapper filters.
- **MCP, per call:** `params._meta["ekos/readScope"]` on `tools/call`. It combines with the
  server's own scope: an object must be visible under both. A scope can therefore only remove.
  A scope that does not parse fails the call; it is never ignored.

A locally run `ekos` with no scope is unrestricted: that user already has the files.

### MCP tools under a restricted scope

Each tool is classified. A tool that reads only through the store is **scoped**. Every other
tool is **refused** for a restricted caller: it reads something the wrapper does not cover (the
conflict counts file, the freshness report with its file paths, session memory, the query log,
a live database). A test fails when a tool is in neither list, so a new tool is refused until
someone decides.

Scoped: `ekos_search`, `ekos_query`, `ekos_retrieve`, `ekos_ekl`, `ekos_neighborhood`,
`ekos_state`, `ekos_dependents`, `ekos_impact`, `ekos_diff`, `ekos_graph_export`.

For a restricted caller the freshness note on read results is dropped: it counts changed files
across every source. The open conflicts on `ekos_state` and the `disputed` flag on a claim are
read through the store, so they show only conflicts the reader can see, which today is none.

The server's result cache is keyed by scope as well as by tool and arguments.

## The console

A role binding (RFC 0180) may now carry a rule:

```
POST /api/tenants/{t}/bindings
{"email": "...", "role": "viewer", "workspace_id": "shop",
 "effect": "allow" | "deny", "source": "...", "path": "...", "object": "..."}
```

- A binding with a rule is for one workspace and the role `viewer`.
- `effect: deny` takes something away from a person who otherwise sees the workspace.
- A person whose only access to a workspace is rule-carrying allow bindings is a **restricted
  viewer**: scope `default: deny` plus their rules. A person with a plain role and some deny
  bindings is restricted too: `default: allow` plus the denies.
- `operator`, `admin` and platform owners are never restricted, and rules on them are ignored:
  running the pipeline and reading its logs shows every source. The API says so when such a
  binding is created.
- API tokens can carry rules like people.

For a restricted principal the console:

- sends the scope with every MCP call for that workspace;
- serves only the routes marked as scoped (graph, object, search, neighbourhood, impact, kinds);
  every other workspace route answers 403. Those routes run CLI commands or read files that
  show more than the ledger: status and its freshness report, run history and logs, the config,
  eval reports, semantics and conflicts;
- refuses review, since the items under review carry no source.

The route walk of RFC 0180 gains a check: a workspace route is scoped only where it is marked.

## Not built

- `source` on derived objects (rollups, semantics, conflicts), which would let a restricted
  reader see the ones derived only from what they can see.
- A restricted reader's dashboard, run history, semantics and conflicts.
- `--read-scope` on `ekos ask`, `docs generate` and the other commands.
- Rules by object kind.
- The MCP gateway that turns an API token into a scope for an MCP client (RFC 0182). Until
  then only the console's own calls carry one.

## Verification

- The decision as a table: every rule shape, deny over allow, several sources, no source, no
  path, the path forms.
- `ScopedStore`: each trait method against a small ledger with a hidden and a visible part; an
  unrestricted scope is byte-identical to the inner store; a test reads the trait's source and
  fails when a method has no decision in the wrapper.
- **Leak suite**: a project of two real sources with marker words in a secret directory and a
  secret source, compiled by the real pipeline. Under a scope that hides them every scoped MCP
  tool is called with every argument shape that could reach them, and no marker may appear
  anywhere in any response. Every other tool must refuse. The same calls without a scope must
  show the markers, or the suite proves nothing.
- Console: scopes computed from bindings as a table; a restricted viewer over HTTP reaches the
  scoped routes with the scope attached and gets 403 on the rest; the route walk.
- Every existing test passes unchanged: no scope, no change.
