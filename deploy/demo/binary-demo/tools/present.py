#!/usr/bin/env python3
"""Readable views over the real ekos MCP tools, for the demo frames.

Every view is a real JSON-RPC call to `ekos mcp serve` (the same server an AI agent uses); this script only
selects and lays out fields. Nothing here computes a result.

  present.py type-summary <TypeName>
  present.py method <TypeName> <MethodName> [max-lines]
  present.py check <TypeName> <python-file>
"""
import json, os, subprocess, sys

EKOS = os.environ.get("EKOS_BIN", "ekos")


class Mcp:
    def __init__(self):
        self.p = subprocess.Popen([EKOS, "mcp", "serve", "--workspace", "."], stdin=subprocess.PIPE,
                                  stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True)
        self.n = 0
        self.rpc("initialize", {"protocolVersion": "2024-11-05", "capabilities": {}, "clientInfo": {"name": "demo", "version": "1"}})

    def rpc(self, method, params=None):
        self.n += 1
        self.p.stdin.write(json.dumps({"jsonrpc": "2.0", "id": self.n, "method": method, "params": params or {}}) + "\n")
        self.p.stdin.flush()
        return json.loads(self.p.stdout.readline())

    def call(self, tool, args):
        r = self.rpc("tools/call", {"name": tool, "arguments": args})
        if "error" in r:
            sys.exit(f"MCP error: {r['error']}")
        return json.loads("".join(c["text"] for c in r["result"]["content"]))

    def close(self):
        self.p.terminate()


def type_id(name):
    out = subprocess.run([EKOS, "query", "find", name], capture_output=True, text=True).stdout
    for line in out.splitlines():
        parts = line.split(None, 1)
        if len(parts) == 2 and parts[1].strip() == name:
            return parts[0]
    sys.exit(f"type {name} not found in the ledger")


def type_summary(name):
    m = Mcp()
    tid = type_id(name)
    d = m.call("ekos_binary_explain", {"id": tid})
    m.close()
    rec = d["recovered"]
    methods = rec["methods"]
    by = {}
    for x in methods:
        by[x["fidelity"]] = by.get(x["fidelity"], 0) + 1
    print(f"type        {d['target']['name']}   ({d['target']['kind']}, id {tid[:8]}…)")
    print(f"binary      {d['provenance']['binary_path']}   sha256 {d['provenance']['binary_sha256'][:16]}…   locator {d['provenance']['locator']}")
    print(f"extractor   {rec['extractor']}")
    print(f"methods     {len(methods)}   " + "   ".join(f"{k} {v}" for k, v in sorted(by.items())))
    print(f"external IO {len(rec['external_io'])} boundaries (no files, network, database)")
    order = rec["migration_order"]
    print(f"\nmigration order — callees first ({len(order)} methods; port these top to bottom):")
    shown = [o for o in order if not o["name"].startswith(("get_", "set_"))]
    for i, o in enumerate(shown[:14], 1):
        print(f"  {i:>2}. {o['name']:<34} {o['fidelity']}")
    print(f"  … {len(shown) - 14} more (accessors omitted)")


def method(name, meth, maxlines=40):
    m = Mcp()
    d = m.call("ekos_binary_explain", {"id": type_id(name), "method": meth})
    m.close()
    x = d["recovered"]["methods"][0]
    sp = x["spec"]
    print(f"{x['name']}{x['signature']}   token {x['locator']}")
    print(f"fidelity    {x['fidelity']}   readiness: {x['migration_readiness']['verdict']}")
    print(f"why         {x['migration_readiness']['why'][:118]}")
    print(f"calls       {', '.join(c['name'] for c in (sp['calls'] or [])) or '-'}")
    ext = sorted({t for t in (x.get("call_targets") or []) if not t.startswith("MarkdownSharp.")})
    if ext:
        print(f"external    {', '.join(ext)[:118]}")
    print(f"called by   {', '.join(c['name'] for c in (sp['called_by'] or [])) or '-'}")
    if x.get("string_literals"):
        print(f"literals    {json.dumps(x['string_literals'], ensure_ascii=False)[:150]}")
    st = sp["statement_stats"]
    if st:
        print(f"statements  {st['statements']}   ifs {st['ifs']}  loops {st['loops']}  switches {st['switches']}  gotos {st['gotos']}  unknown {st['unknown']}")
    if sp.get("statement_gaps"):
        print(f"GAP         {json.dumps(sp['statement_gaps'])[:220]}")
    print("\npseudocode (every line cites its IL offset):")
    lines = sp["pseudocode"].splitlines()
    for ln in lines[:maxlines]:
        print("  " + (ln if len(ln) <= 150 else ln[:149] + "…"))
    if len(lines) > maxlines:
        print(f"  … {len(lines) - maxlines} more lines")


def check(name, pyfile, maxrows=12):
    m = Mcp()
    d = m.call("ekos_binary_migration_check", {"id": type_id(name), "python_source": open(pyfile, encoding="utf-8").read()})
    m.close()
    s = d["summary"]
    print(f"type {d['type']}   rewrite {os.path.basename(pyfile)}")
    print(f"summary  matches {s['matches']}   differs {s['differs']}   missing {s['missing']}   python_only {s['python_only']}\n")
    print(f"{'verdict':<9} {'original method':<40} first difference")
    rows, seen = [], set()
    for x in d["methods"]:
        key = (x["method"], x["verdict"])
        if x["verdict"] == "matches" or key in seen or x["method"].startswith(("get_", "set_")):
            continue
        seen.add(key)
        rows.append(f"{x['verdict']:<9} {x['method']:<40} {(x.get('differences') or [''])[0][:95]}")
    print("\n".join(rows[:maxrows]))
    if len(rows) > maxrows:
        print(f"…         {len(rows) - maxrows} more rows: constants that moved into _literals.py, format strings written as f-strings, one dead method")
    acc = sum(1 for x in d["methods"] if x["method"].startswith(("get_", "set_")) and x["verdict"] != "matches")
    print(f"missing   {'get_*/set_* (' + str(acc) + ' accessors)':<40} C# properties are plain attributes here")
    print(f"\n{d['note']}")


if __name__ == "__main__":
    cmd = sys.argv[1]
    if cmd == "type-summary":
        type_summary(sys.argv[2])
    elif cmd == "method":
        method(sys.argv[2], sys.argv[3], int(sys.argv[4]) if len(sys.argv) > 4 else 40)
    elif cmd == "check":
        check(sys.argv[2], sys.argv[3], int(sys.argv[4]) if len(sys.argv) > 4 else 12)
    else:
        sys.exit(__doc__)
