#!/usr/bin/env python3
"""Print the source of one function/method from a Python file: snippet.py <file> <name>."""
import ast, sys, textwrap

src = open(sys.argv[1], encoding="utf-8").read()
for node in ast.walk(ast.parse(src)):
    if isinstance(node, (ast.FunctionDef, ast.ClassDef)) and node.name == sys.argv[2]:
        print(textwrap.dedent("\n".join(src.splitlines()[node.lineno - 1 : node.end_lineno])))
        break
else:
    sys.exit(f"{sys.argv[2]} not found")
