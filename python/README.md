# ekos

An embedded knowledge engine for Python: open a compiled, evidence-backed
[EKOS](https://github.com/alexeyban/EKOS) knowledge ledger and query it in-process. No server,
no Rust toolchain.

```bash
pip install ekos
```

Python 3.10+, no dependencies. **Status: provisional (RFC 0269)** — names may still change.

```python
import ekos

kb = ekos.open("path/to/workspace")          # read-only; never creates or changes a ledger
hits = kb.search("payment status", limit=10)
state = kb.get(hits[0].id)                   # the object, its relationships and its evidence
for e in state.evidence:
    print(e.path, e.line, e.fragment)

kb.impact(hits[0].id, hops=3)                # what a change to it would reach
answer = kb.query("which tables reference customers?")   # claims with evidence, no model call
```

The package reads. Knowledge enters a ledger by compiling sources with the `ekos` command.

Full guide: [docs/python.md](https://github.com/alexeyban/EKOS/blob/main/docs/python.md).
Examples: [python/examples](https://github.com/alexeyban/EKOS/tree/main/python/examples).
Build from a checkout: `uvx maturin build --release` in this directory.
