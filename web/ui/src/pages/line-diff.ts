// A line diff (Myers' O((N+M)·D) algorithm) and its side-by-side rendering model, for the LinkML
// editor's "diff vs export" view. Pure, no dependencies: an expert's edits to a schema are a
// handful of lines in thousands, which is exactly where Myers is fast.

export type Op = { kind: "same" | "del" | "add"; text: string };

/** Whether the furthest path on diagonal `k` at depth `d` comes down from `k + 1` (an insertion)
 * rather than across from `k - 1` (a deletion). */
function fromAbove(v: Int32Array, offset: number, k: number, d: number): boolean {
  return k === -d || (k !== d && v[offset + k - 1] < v[offset + k + 1]);
}

/** Myers' forward pass: the furthest-reaching `v` of every depth, up to the one that reaches
 * (n, m). */
function forwardTrace(a: string[], b: string[], offset: number): Int32Array[] {
  const n = a.length;
  const m = b.length;
  const max = n + m;
  let v = new Int32Array(2 * max + 2);
  const trace: Int32Array[] = [];
  let done = false;
  for (let d = 0; d <= max && !done; d++) {
    trace.push(v.slice());
    const next = v.slice();
    done = extendDepth(a, b, v, next, offset, d);
    v = next;
  }
  trace.push(v.slice());
  return trace;
}

/** Extends every diagonal of depth `d` into `next`; true once one of them reaches (n, m). */
function extendDepth(a: string[], b: string[], v: Int32Array, next: Int32Array, offset: number, d: number): boolean {
  for (let k = -d; k <= d; k += 2) {
    let x = fromAbove(v, offset, k, d) ? v[offset + k + 1] : v[offset + k - 1] + 1;
    let y = x - k;
    while (x < a.length && y < b.length && a[x] === b[y]) {
      x++;
      y++;
    }
    next[offset + k] = x;
    if (x >= a.length && y >= b.length) return true;
  }
  return false;
}

/** A position in the edit graph during the walk back, and the script collected so far
 * (in reverse). */
interface Walk {
  a: string[];
  b: string[];
  x: number;
  y: number;
  ops: Op[];
}

/** Moves back along a diagonal (unchanged lines) while both sides are above the bound. */
function snakeBack(w: Walk, toX: number, toY: number): void {
  while (w.x > toX && w.y > toY) {
    w.ops.push({ kind: "same", text: w.a[w.x - 1] });
    w.x--;
    w.y--;
  }
}

/** One depth of the walk back: the snake that ended depth `d`, then the edit before it. */
function stepBack(w: Walk, vd: Int32Array, offset: number, d: number): void {
  const k = w.x - w.y;
  const prevK = fromAbove(vd, offset, k, d) ? k + 1 : k - 1;
  const prevX = d === 0 ? 0 : vd[offset + prevK];
  snakeBack(w, prevX, prevX - prevK);
  if (d === 0) return;
  if (w.x === prevX) {
    w.ops.push({ kind: "add", text: w.b[w.y - 1] });
    w.y--;
  } else {
    w.ops.push({ kind: "del", text: w.a[w.x - 1] });
    w.x--;
  }
}

/** The edit script turning `a` into `b`, line by line. */
export function diffLines(a: string[], b: string[]): Op[] {
  const offset = a.length + b.length;
  const trace = forwardTrace(a, b, offset);

  // Walk the trace back from (n, m) to (0, 0).
  const w: Walk = { a, b, x: a.length, y: b.length, ops: [] };
  for (let d = trace.length - 2; d >= 0 && (w.x > 0 || w.y > 0); d--) stepBack(w, trace[d], offset, d);
  snakeBack(w, 0, 0);
  return w.ops.reverse();
}

/** One row of a side-by-side view: a left line, a right line, or both. Numbers are 1-based. */
export interface Row {
  kind: "same" | "change" | "del" | "add" | "gap";
  left?: { n: number; text: string };
  right?: { n: number; text: string };
  /** For a `gap` row: how many unchanged lines it stands for. */
  hidden?: number;
}

/** Every line as a side-by-side row, and how many separate changes there are. */
function allRows(ops: Op[]): { full: Row[]; changes: number } {
  const full: Row[] = [];
  let ln = 1;
  let rn = 1;
  let changes = 0;
  for (let i = 0; i < ops.length; ) {
    if (ops[i].kind === "same") {
      full.push({ kind: "same", left: { n: ln++, text: ops[i].text }, right: { n: rn++, text: ops[i].text } });
      i++;
      continue;
    }
    changes++;
    const dels: string[] = [];
    const adds: string[] = [];
    while (i < ops.length && ops[i].kind === "del") dels.push(ops[i++].text);
    while (i < ops.length && ops[i].kind === "add") adds.push(ops[i++].text);
    full.push(...pairUp(dels, adds, ln, rn));
    ln += dels.length;
    rn += adds.length;
  }
  return { full, changes };
}

/** One change's deleted and added lines, paired row by row, numbered from `ln` and `rn`. */
function pairUp(dels: string[], adds: string[], ln: number, rn: number): Row[] {
  const rows: Row[] = [];
  for (let j = 0; j < Math.max(dels.length, adds.length); j++) {
    const l = j < dels.length ? { n: ln + j, text: dels[j] } : undefined;
    const r = j < adds.length ? { n: rn + j, text: adds[j] } : undefined;
    rows.push({ kind: rowKind(l, r), left: l, right: r });
  }
  return rows;
}

function rowKind(left: unknown, right: unknown): Row["kind"] {
  if (left && right) return "change";
  return left ? "del" : "add";
}

/** Keeps only changes and `context` rows around each; the rest collapse into `gap` rows. */
function collapse(full: Row[], context: number): Row[] {
  const keep = full.map(() => false);
  full.forEach((r, i) => {
    if (r.kind === "same") return;
    for (let j = Math.max(0, i - context); j <= Math.min(full.length - 1, i + context); j++) keep[j] = true;
  });
  const rows: Row[] = [];
  let hidden = 0;
  full.forEach((r, i) => {
    if (!keep[i]) {
      hidden++;
      return;
    }
    if (hidden > 0) rows.push({ kind: "gap", hidden });
    hidden = 0;
    rows.push(r);
  });
  if (hidden > 0 && rows.length > 0) rows.push({ kind: "gap", hidden });
  return rows;
}

/** Side-by-side rows with `context` unchanged lines around each change; longer unchanged runs
 * collapse into one `gap` row. A deletion followed by an addition pairs up as a `change`. */
export function sideBySide(before: string, after: string, context = 3): {
  rows: Row[];
  changes: number;
} {
  const { full, changes } = allRows(diffLines(before.split("\n"), after.split("\n")));
  return { rows: collapse(full, context), changes };
}
