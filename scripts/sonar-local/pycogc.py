"""Cognitive complexity of Python functions, counted the way SonarCloud's S3776 counts it.

`complexipy` scores lower than Sonar on this repository (`parse_activity`: 10 against Sonar's 20),
mostly because Sonar counts every boolean-operator sequence, including `x or ""` defaults outside
conditions. This follows SonarSource's specification ("Cognitive Complexity", G. Ann Campbell):

* +1, plus the nesting level: `if`, ternary, `for`, `while`, `except`, `match`;
* +1 without nesting: `elif`, `else`, each sequence of like boolean operators, a `break`/`continue`
  is free, recursion is not counted;
* nesting grows inside those structures and inside a nested function or lambda.

Usage: `pycogc.py <limit> <base>` → JSON lines {file, line, end, name, complexity} for every
function over the limit in the Python files changed since <base> that SonarCloud analyses as main
code. The files come from git, never from the command line. Standard library only.
"""

from __future__ import annotations

import ast
import json
import re
import sys

sys.path.insert(0, str(__import__("pathlib").Path(__file__).resolve().parent))
from changed_lines import changed, git_diff, repo_root

# What Sonar analyses as main code: not docs, demos or fixtures (`sonar.exclusions`), not test code
# (`sonar.tests`: web/api/tests, deploy/e2e; scripts/tests by name).
NOT_MAIN = re.compile(
    r"^(docs/|web/api/tests/|deploy/e2e/|deploy/fixtures/|scripts/tests/)|/demo/|/tests/fixtures/"
)


class Counter(ast.NodeVisitor):
    def __init__(self) -> None:
        self.score = 0
        self.nesting = 0

    def nested(self, nodes: list[ast.AST] | ast.AST) -> None:
        self.nesting += 1
        for n in nodes if isinstance(nodes, list) else [nodes]:
            self.visit(n)
        self.nesting -= 1

    def visit_If(self, node: ast.If, elif_: bool = False) -> None:
        self.score += 1 if elif_ else 1 + self.nesting
        self.visit(node.test)
        self.nested(node.body)
        orelse = node.orelse
        if len(orelse) == 1 and isinstance(orelse[0], ast.If):
            self.visit_If(orelse[0], elif_=True)
        elif orelse:
            self.score += 1
            self.nested(orelse)

    def visit_IfExp(self, node: ast.IfExp) -> None:
        self.score += 1 + self.nesting
        self.visit(node.test)
        self.nested([node.body, node.orelse])

    def _loop(self, node: ast.For | ast.AsyncFor | ast.While) -> None:
        self.score += 1 + self.nesting
        if isinstance(node, ast.While):
            self.visit(node.test)
        else:
            self.visit(node.iter)
        self.nested(node.body)
        if node.orelse:
            self.score += 1
            self.nested(node.orelse)

    visit_For = visit_AsyncFor = visit_While = _loop

    def visit_Try(self, node: ast.Try) -> None:
        for n in node.body:
            self.visit(n)
        for h in node.handlers:
            self.score += 1 + self.nesting
            self.nested(h.body)
        if node.orelse:
            self.score += 1
            self.nested(node.orelse)
        for n in node.finalbody:
            self.visit(n)

    visit_TryStar = visit_Try

    def visit_Match(self, node: ast.Match) -> None:
        self.score += 1 + self.nesting
        self.visit(node.subject)
        for c in node.cases:
            self.nested(c.body)

    def visit_BoolOp(self, node: ast.BoolOp) -> None:
        self.score += 1
        for v in node.values:
            # `a or (b or c)` is still one sequence of `or`.
            if isinstance(v, ast.BoolOp) and type(v.op) is type(node.op):
                for w in v.values:
                    self.visit(w)
            else:
                self.visit(v)

    def visit_comprehension(self, node: ast.comprehension) -> None:
        # Sonar counts a comprehension's `if` clauses like conditions: +1 each, no nesting.
        self.visit(node.iter)
        for cond in node.ifs:
            self.score += 1
            self.visit(cond)

    def _function(self, node: ast.AST) -> None:
        self.nested(node.body if isinstance(node.body, list) else [node.body])

    visit_FunctionDef = visit_AsyncFunctionDef = visit_Lambda = _function


def functions(tree: ast.AST):
    """Every function, with its qualified name; nested functions count inside their parent."""

    def walk(node: ast.AST, prefix: str):
        for child in ast.iter_child_nodes(node):
            if isinstance(child, (ast.FunctionDef, ast.AsyncFunctionDef)):
                yield f"{prefix}{child.name}", child
            elif isinstance(child, ast.ClassDef):
                yield from walk(child, f"{prefix}{child.name}::")

    yield from walk(tree, "")


def score(fn: ast.FunctionDef | ast.AsyncFunctionDef) -> int:
    c = Counter()
    for stmt in fn.body:
        c.visit(stmt)
    return c.score


def main() -> int:
    limit = int(sys.argv[1])
    root = repo_root()
    for rel in sorted(changed(git_diff(sys.argv[2]))):
        path = (root / rel).resolve()
        if (
            not rel.endswith(".py")
            or NOT_MAIN.search(rel)
            or not path.is_relative_to(root)
        ):
            continue
        if not path.is_file():
            continue
        tree = ast.parse(path.read_text(encoding="utf-8"), filename=rel)
        for name, fn in functions(tree):
            n = score(fn)
            if n > limit:
                print(
                    json.dumps(
                        {
                            "file": rel,
                            "line": fn.lineno,
                            "end": fn.end_lineno,
                            "name": name,
                            "complexity": n,
                        }
                    )
                )
    return 0


if __name__ == "__main__":
    sys.exit(main())
