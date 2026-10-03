// A line diff (Myers' O((N+M)·D) algorithm) and its side-by-side rendering model, for the LinkML
// editor's "diff vs export" view. Pure, no dependencies: an expert's edits to a schema are a
// handful of lines in thousands, which is exactly where Myers is fast.

export type Op = { kind: "same" | "del" | "add"; text: string };

/** The edit script turning `a` into `b`, line by line. */
export function diffLines(a: string[], b: string[]): Op[] {
  const n = a.length;
  const m = b.length;
  const max = n + m;
  const offset = max;
  let v = new Int32Array(2 * max + 2);
  const trace: Int32Array[] = [];
  let done = false;
  for (let d = 0; d <= max && !done; d++) {
    trace.push(v.slice());
    const next = v.slice();
    for (let k = -d; k <= d; k += 2) {
      let x =
        k === -d || (k !== d && v[offset + k - 1] < v[offset + k + 1])
          ? v[offset + k + 1]
          : v[offset + k - 1] + 1;
      let y = x - k;
      while (x < n && y < m && a[x] === b[y]) {
        x++;
        y++;
      }
      next[offset + k] = x;
      if (x >= n && y >= m) {
        done = true;
        break;
      }
    }
    v = next;
  }
  trace.push(v.slice());

  // Walk the trace back from (n, m) to (0, 0).
  const ops: Op[] = [];
  let x = n;
  let y = m;
  for (let d = trace.length - 2; d >= 0 && (x > 0 || y > 0); d--) {
    const vd = trace[d];
    const k = x - y;
    const prevK =
      k === -d || (k !== d && vd[offset + k - 1] < vd[offset + k + 1]) ? k + 1 : k - 1;
    const prevX = d === 0 ? 0 : vd[offset + prevK];
    const prevY = prevX - prevK;
    while (x > prevX && y > prevY) {
      ops.push({ kind: "same", text: a[x - 1] });
      x--;
      y--;
    }
    if (d === 0) break;
    if (x === prevX) {
      ops.push({ kind: "add", text: b[y - 1] });
      y--;
    } else {
      ops.push({ kind: "del", text: a[x - 1] });
      x--;
    }
  }
  while (x > 0 && y > 0) {
    ops.push({ kind: "same", text: a[x - 1] });
    x--;
    y--;
  }
  return ops.reverse();
}

/** One row of a side-by-side view: a left line, a right line, or both. Numbers are 1-based. */
export interface Row {
  kind: "same" | "change" | "del" | "add" | "gap";
  left?: { n: number; text: string };
  right?: { n: number; text: string };
  /** For a `gap` row: how many unchanged lines it stands for. */
  hidden?: number;
}

/** Side-by-side rows with `context` unchanged lines around each change; longer unchanged runs
 * collapse into one `gap` row. A deletion followed by an addition pairs up as a `change`. */
export function sideBySide(before: string, after: string, context = 3): {
  rows: Row[];
  changes: number;
} {
  const ops = diffLines(before.split("\n"), after.split("\n"));
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
    for (let j = 0; j < Math.max(dels.length, adds.length); j++) {
      const l = j < dels.length ? { n: ln++, text: dels[j] } : undefined;
      const r = j < adds.length ? { n: rn++, text: adds[j] } : undefined;
      full.push({ kind: l && r ? "change" : l ? "del" : "add", left: l, right: r });
    }
  }
  // Keep only changes and their context.
  const keep = full.map(() => false);
  full.forEach((r, i) => {
    if (r.kind !== "same") {
      for (let j = Math.max(0, i - context); j <= Math.min(full.length - 1, i + context); j++) {
        keep[j] = true;
      }
    }
  });
  const rows: Row[] = [];
  let hidden = 0;
  full.forEach((r, i) => {
    if (keep[i]) {
      if (hidden > 0) rows.push({ kind: "gap", hidden });
      hidden = 0;
      rows.push(r);
    } else {
      hidden++;
    }
  });
  if (hidden > 0 && rows.length > 0) rows.push({ kind: "gap", hidden });
  return { rows, changes };
}
