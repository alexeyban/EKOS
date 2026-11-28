# ekos

An embedded knowledge engine for Python: open a compiled, evidence-backed EKOS knowledge ledger
and query it in-process. No server, no Rust toolchain.

**Status: provisional (RFC 0269, phase 0).** The package is not on PyPI yet and the API will change.

```python
import ekos

kb = ekos.open("path/to/workspace")     # read-only; never creates or changes a ledger
kb.status()
hits = kb.search("payment status", limit=10)
state = kb.get(hits[0].id)              # the object, its relationships and its evidence
```

Build from a checkout: `uvx maturin build --release` in this directory.
