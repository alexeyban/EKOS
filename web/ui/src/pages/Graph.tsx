import { useQuery } from "@tanstack/react-query";
import { lazy, Suspense, useMemo, useState } from "react";
import { useParams } from "react-router-dom";
import { api } from "../api/client";
import { colorFor, type GLink, type GNode } from "./graph-shared";
import { ObjectPanel } from "./ObjectPanel";

const GraphCanvas = lazy(() => import("./GraphCanvas").then((m) => ({ default: m.GraphCanvas })));

interface GraphOut {
  level: "aggregate" | "object";
  counts: Record<string, number>;
  truncated: { nodes: boolean; node_limit: number };
  nodes: { id: string; n?: string; k?: number; d?: number; count?: number }[];
  edges: { s: number; t: number; k?: number; w?: number }[];
  kind_index: string[];
  rel_kind_index: string[];
}

const DEFAULT_OFF_RELS = ["CoupledWith", "FeedsInto"];
const ALL_RELS = ["Calls", "Contains", "CoupledWith", "DependsOn", "References", "SameAs"];
const OBJECT_BUDGET = 800;

export function Graph() {
  const { id = "" } = useParams();

  // null = overview (aggregate). Otherwise: object level, focused on this kind (others dimmed).
  const [focusKind, setFocusKind] = useState<string | null>(null);
  const [excludedRels, setExcludedRels] = useState<Set<string>>(new Set(DEFAULT_OFF_RELS));
  const [minDegree, setMinDegree] = useState(2);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [focusId, setFocusId] = useState<string | null>(null);
  const [q, setQ] = useState("");
  const [fullscreen, setFullscreen] = useState(false);

  const expanded = focusKind !== null;

  const params = new URLSearchParams();
  if (expanded) {
    params.set("level", "object");
    params.set("max_nodes", String(OBJECT_BUDGET));
    params.set("min_degree", String(minDegree));
  } else {
    params.set("level", "aggregate");
    params.set("group_by", "kind");
  }
  for (const r of excludedRels) params.append("exclude_rel_kind", r);

  const graph = useQuery({
    queryKey: ["graph", id, params.toString()],
    queryFn: () => api<GraphOut>(`/workspaces/${id}/graph?${params}`),
    enabled: id !== "",
  });

  const search = useQuery({
    queryKey: ["gsearch", id, q],
    queryFn: () =>
      api<{ matches: { id: string; name: string }[] }>(
        `/workspaces/${id}/search?q=${encodeURIComponent(q)}&limit=15`,
      ),
    enabled: q.trim().length > 1,
  });

  const { nodes, links } = useMemo(() => {
    const g = graph.data;
    if (!g) return { nodes: [] as GNode[], links: [] as GLink[] };
    const isAgg = g.level === "aggregate";
    const nodes: GNode[] = g.nodes.map((n) => ({
      id: n.id,
      label: n.n ?? n.id,
      kind: g.kind_index[n.k ?? 0] ?? "?",
      kindIdx: n.k ?? 0,
      degree: n.d ?? 0,
      count: n.count,
      isAggregate: isAgg,
    }));
    const byIdx = g.nodes.map((n) => n.id);
    const links: GLink[] = g.edges.map((e) => ({
      source: byIdx[e.s],
      target: byIdx[e.t],
      relKind: g.rel_kind_index[e.k ?? 0] ?? "?",
      weight: e.w ?? 1,
    }));
    return { nodes, links };
  }, [graph.data]);

  const gotoObject = (oid: string) => {
    setSelectedId(oid);
    setFocusId(oid);
  };

  const toggleRel = (rk: string, on: boolean) =>
    setExcludedRels((s) => {
      const n = new Set(s);
      if (on) n.delete(rk);
      else n.add(rk);
      return n;
    });

  const overviewNodes = graph.data && !expanded ? [...graph.data.nodes] : [];
  overviewNodes.sort((a, b) => (b.count ?? 0) - (a.count ?? 0));

  return (
    <div className={fullscreen ? "graph-page fs" : "graph-page"}>
      <div className="graph-toolbar">
        <span>
          {expanded ? (
            <button className="pill" onClick={() => setFocusKind(null)}>
              ↑ overview
            </button>
          ) : (
            <span className="muted">click a bubble to drill in</span>
          )}
        </span>
        <button className="pill" onClick={() => setFullscreen((f) => !f)}>
          {fullscreen ? "✕ exit fullscreen" : "⛶ fullscreen"}
        </button>
      </div>

      <div className="graph-layout">
        <aside className="graph-side">
          <strong>{expanded ? `Objects · focus: ${focusKind}` : "Overview — by kind"}</strong>
          {graph.data?.truncated?.nodes && (
            <p className="warn-line">
              showing the {graph.data.truncated.node_limit} most-connected of{" "}
              {graph.data.counts.objects_after_filter ?? "?"} — raise min-degree to thin it
            </p>
          )}

          {expanded && (
            <label className="muted" style={{ display: "block", margin: "0.5rem 0" }}>
              min degree: {minDegree}
              <input
                type="range"
                min={0}
                max={15}
                value={minDegree}
                onChange={(e) => setMinDegree(Number(e.target.value))}
                style={{ width: "100%" }}
              />
            </label>
          )}

          <p className="muted" style={{ marginTop: "0.75rem" }}>
            relationship kinds
          </p>
          {(graph.data && graph.data.rel_kind_index.length ? graph.data.rel_kind_index : ALL_RELS).map(
            (rk) => (
              <label key={rk} className="flt">
                <input
                  type="checkbox"
                  checked={!excludedRels.has(rk)}
                  onChange={(e) => toggleRel(rk, e.target.checked)}
                />{" "}
                {rk}
              </label>
            ),
          )}

          <p className="muted" style={{ marginTop: "0.75rem" }}>
            search
          </p>
          <input
            value={q}
            onChange={(e) => setQ(e.target.value)}
            placeholder="2–3 keywords"
            style={{ width: "100%" }}
          />
          <ul className="srch">
            {search.data?.matches.map((m) => (
              <li key={m.id}>
                <button className="linkish" onClick={() => gotoObject(m.id)}>
                  {m.name}
                </button>
              </li>
            ))}
          </ul>

          <p className="muted" style={{ marginTop: "0.75rem" }}>
            {expanded ? "focus a kind" : "click a bubble (or a name) to drill in"}
          </p>
          <ul className="kinds">
            {overviewNodes.map((n) => (
              <li key={n.id}>
                <button
                  className="linkish"
                  onClick={() => setFocusKind(n.n ?? null)}
                  style={{ color: colorFor(n.k ?? 0) }}
                >
                  {n.n}
                </button>{" "}
                <span className="muted">{n.count}</span>
              </li>
            ))}
            {expanded &&
              [...new Set(nodes.map((n) => n.kind))].sort().map((k) => (
                <li key={k}>
                  <button
                    className="linkish"
                    onClick={() => setFocusKind(k)}
                    style={{ fontWeight: k === focusKind ? 700 : 400 }}
                  >
                    {k}
                  </button>
                </li>
              ))}
          </ul>
        </aside>

        <div className="graph-canvas">
          {graph.isError && <p className="err">{String(graph.error)}</p>}
          {graph.isLoading && <p className="muted">loading graph…</p>}
          {graph.data && (
            <Suspense fallback={<p className="muted">loading renderer…</p>}>
              <GraphCanvas
                nodes={nodes}
                links={links}
                focusId={focusId}
                selectedId={selectedId}
                dimKind={expanded ? focusKind : null}
                onNodeClick={(n) => {
                  if (n.isAggregate) setFocusKind(n.label);
                  else {
                    setSelectedId(n.id);
                    setFocusId(n.id);
                  }
                }}
              />
            </Suspense>
          )}
        </div>

        {selectedId && (
          <ObjectPanel
            workspace={id}
            objectId={selectedId}
            onClose={() => setSelectedId(null)}
            onGoto={gotoObject}
          />
        )}
      </div>
    </div>
  );
}
