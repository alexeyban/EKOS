import { useQuery } from "@tanstack/react-query";
import { lazy, Suspense, useMemo, useState } from "react";
import { Link, useParams } from "react-router-dom";
import { api } from "../api/client";
import type { GLink, GNode } from "./graph-shared";
import { colorFor } from "./graph-shared";
import { ObjectPanel } from "./ObjectPanel";

const GraphCanvas = lazy(() =>
  import("./GraphCanvas").then((m) => ({ default: m.GraphCanvas })),
);

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

export function Graph() {
  const { id = "" } = useParams();

  const [expandedKind, setExpandedKind] = useState<string | null>(null);
  const [excludedRels, setExcludedRels] = useState<Set<string>>(new Set(DEFAULT_OFF_RELS));
  const [minDegree, setMinDegree] = useState(0);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [focusId, setFocusId] = useState<string | null>(null);
  const [q, setQ] = useState("");

  const params = new URLSearchParams();
  if (expandedKind) {
    params.set("level", "object");
    params.set("kind", expandedKind);
    params.set("max_nodes", "500");
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
    queryFn: () => api<{ matches: { id: string; name: string }[] }>(
      `/workspaces/${id}/search?q=${encodeURIComponent(q)}&limit=15`,
    ),
    enabled: q.trim().length > 1,
  });

  const { nodes, links, relKinds } = useMemo(() => {
    const g = graph.data;
    if (!g) return { nodes: [] as GNode[], links: [] as GLink[], relKinds: [] as string[] };
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
    return { nodes, links, relKinds: g.rel_kind_index };
  }, [graph.data]);

  const gotoObject = (oid: string) => {
    setSelectedId(oid);
    setFocusId(oid);
    if (!nodes.some((n) => n.id === oid)) {
      // not in the current view — expand its kind first (best effort: we don't know the kind
      // without a lookup, so open the panel and let the user expand).
    }
  };

  return (
    <>
      <p className="crumbs">
        <Link to={`/w/${id}`} className="linkish">
          ← dashboard
        </Link>
        {expandedKind && (
          <button className="linkish" onClick={() => setExpandedKind(null)}>
            ↑ back to overview
          </button>
        )}
      </p>

      <div className="graph-layout">
        <aside className="graph-side">
          <strong>{expandedKind ? `${expandedKind} objects` : "Overview — by kind"}</strong>
          {graph.data?.truncated?.nodes && (
            <p className="warn-line">
              showing the {graph.data.truncated.node_limit} most-connected of{" "}
              {graph.data.counts.objects_after_filter ?? "?"}
            </p>
          )}

          {expandedKind && (
            <label className="muted" style={{ display: "block", margin: "0.5rem 0" }}>
              min degree {minDegree}
              <input
                type="range"
                min={0}
                max={20}
                value={minDegree}
                onChange={(e) => setMinDegree(Number(e.target.value))}
                style={{ width: "100%" }}
              />
            </label>
          )}

          <p className="muted" style={{ marginTop: "0.75rem" }}>
            relationship kinds
          </p>
          {relKinds.map((rk) => (
            <label key={rk} className="flt">
              <input
                type="checkbox"
                checked={!excludedRels.has(rk)}
                onChange={(e) =>
                  setExcludedRels((s) => {
                    const n = new Set(s);
                    e.target.checked ? n.delete(rk) : n.add(rk);
                    return n;
                  })
                }
              />{" "}
              {rk}
            </label>
          ))}

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

          {!expandedKind && graph.data && (
            <>
              <p className="muted" style={{ marginTop: "0.75rem" }}>
                click a bubble to expand
              </p>
              <ul className="kinds">
                {[...graph.data.nodes]
                  .sort((a, b) => (b.count ?? 0) - (a.count ?? 0))
                  .map((n) => (
                    <li key={n.id}>
                      <button
                        className="linkish"
                        onClick={() => setExpandedKind(n.n ?? null)}
                        style={{ color: colorFor(n.k ?? 0) }}
                      >
                        {n.n}
                      </button>{" "}
                      <span className="muted">{n.count}</span>
                    </li>
                  ))}
              </ul>
            </>
          )}
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
                onNodeClick={(n) => {
                  if (n.isAggregate) setExpandedKind(n.label);
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
    </>
  );
}
