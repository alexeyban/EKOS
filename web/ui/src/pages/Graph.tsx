import { useQuery } from "@tanstack/react-query";
import { lazy, Suspense, useEffect, useMemo, useRef, useState } from "react";
import { useParams, useSearchParams } from "react-router-dom";
import { api, apiPost } from "../api/client";
import type { GraphCanvasHandle } from "./GraphCanvas";
import { colorFor, SERVER_LAYOUT_THRESHOLD, type GLink, type GNode } from "./graph-shared";
import { impactHopMap, neighborhoodToGraph, type ImpactOut, type NeighborhoodOut } from "./graph-v2";
import { GraphTimeline, type TimelinePoint } from "./GraphTimeline";
import { ObjectPanel } from "./ObjectPanel";

const GraphCanvas = lazy(() => import("./GraphCanvas").then((m) => ({ default: m.GraphCanvas })));

interface GraphOut {
  level: "aggregate" | "object";
  as_of?: string | null;
  counts: Record<string, number>;
  truncated: { nodes: boolean; node_limit: number };
  nodes: { id: string; n?: string; k?: number; d?: number; count?: number; fs?: string }[];
  edges: { s: number; t: number; k?: number; w?: number; fs?: string }[];
  kind_index: string[];
  rel_kind_index: string[];
}

const DEFAULT_OFF_RELS = ["CoupledWith", "FeedsInto"];
const ALL_RELS = ["Calls", "Contains", "CoupledWith", "DependsOn", "References", "SameAs"];
const OBJECT_BUDGET = 800;

export function Graph() {
  const { id = "" } = useParams();
  // RFC 0136 §7 (deep-linking) — `?as_of=&focus=` seed the initial view once on mount (a lazy
  // initializer, so this reads the URL exactly once, not on every render) and are kept in sync as
  // the corresponding state changes below. One-directional (state -> URL) rather than a full
  // two-way binding: simple enough to satisfy "paste this link, see the same view" without also
  // having to guard against browser back/forward re-driving state mid-session.
  const [searchParams, setSearchParams] = useSearchParams();

  // null = overview (aggregate). Otherwise: object level, focused on this kind (others dimmed).
  const [focusKind, setFocusKind] = useState<string | null>(null);
  const [excludedRels, setExcludedRels] = useState<Set<string>>(new Set(DEFAULT_OFF_RELS));
  const [minDegree, setMinDegree] = useState(2);
  const [selectedId, setSelectedId] = useState<string | null>(() => searchParams.get("focus"));
  const [focusId, setFocusId] = useState<string | null>(() => searchParams.get("focus"));
  const [q, setQ] = useState("");
  const [fullscreen, setFullscreen] = useState(false);
  const [linkCopied, setLinkCopied] = useState(false);
  // RFC 0134 — null = live. Otherwise an RFC 3339 instant the graph is shown "as of".
  const [asOf, setAsOf] = useState<string | null>(() => searchParams.get("as_of"));

  // Deep-link a selected object into an expanded (object-level) view — a bare `?focus=<id>` with
  // no other state would otherwise land on the aggregate overview, where the id can't resolve to
  // a visible node. `""` (not a real kind name) is a deliberate "expanded, no kind filter" value:
  // `focusKind` is only ever sent to the backend as an `expanded` boolean (`level=object` vs
  // `aggregate`), never as a `kind=` filter param, and `dimKind={... ? focusKind : null}` treats
  // an empty string as falsy, so nothing gets dimmed — this reaches the same "show me the object
  // among every kind" state a real click-driven expansion never actually produces on its own.
  useEffect(() => {
    if (searchParams.get("focus") && focusKind === null) setFocusKind("");
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    setSearchParams(
      (prev) => {
        const next = new URLSearchParams(prev);
        if (asOf) next.set("as_of", asOf);
        else next.delete("as_of");
        if (selectedId) next.set("focus", selectedId);
        else next.delete("focus");
        return next;
      },
      { replace: true },
    );
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [asOf, selectedId]);
  // RFC 0136 §2 — set replaces the normal graph fetch with just this object's BFS neighbourhood.
  const [isolate, setIsolate] = useState<{ id: string; depth: number } | null>(null);
  // RFC 0136 §3 — set overlays a hop-distance trace on whatever graph is currently on screen.
  const [impact, setImpact] = useState<{
    id: string;
    direction: "dependents" | "dependencies";
  } | null>(null);
  const canvasRef = useRef<GraphCanvasHandle>(null);

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
  // Always fetch the union (latest) graph with first-seen stamps — the slider filters it
  // client-side, so scrubbing never refetches (RFC 0134 §3.1/§3.3).
  params.set("include_first_seen", "1");

  const graph = useQuery({
    queryKey: ["graph", id, params.toString()],
    queryFn: () => api<GraphOut>(`/workspaces/${id}/graph?${params}`),
    enabled: id !== "",
  });

  const timeline = useQuery({
    queryKey: ["graph-timeline", id],
    queryFn: () =>
      api<{ points: TimelinePoint[] }>(`/workspaces/${id}/stats/timeline?bucket=day`),
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

  // RFC 0136 §2 — a real sub-graph (objects + relationships), fetched instead of `/graph` when
  // isolate mode is active. Depth is part of the key so dragging the slider refetches.
  const neighborhood = useQuery({
    queryKey: ["neighborhood", id, isolate?.id, isolate?.depth],
    queryFn: () =>
      api<NeighborhoodOut>(
        `/workspaces/${id}/neighborhood/${isolate?.id}?depth=${isolate?.depth}`,
      ),
    enabled: isolate !== null,
  });

  // RFC 0136 §3 — a hop-distance node list, overlaid on whatever graph is already on screen.
  const impactQuery = useQuery({
    queryKey: ["impact", id, impact?.id, impact?.direction],
    queryFn: () =>
      api<ImpactOut>(
        `/workspaces/${id}/impact/${impact?.id}?direction=${impact?.direction}&max_hops=5`,
      ),
    enabled: impact !== null,
  });

  const { nodes, links } = useMemo(() => {
    if (isolate) {
      return neighborhood.data
        ? neighborhoodToGraph(neighborhood.data)
        : { nodes: [] as GNode[], links: [] as GLink[] };
    }
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
      firstSeen: n.fs,
    }));
    const byIdx = g.nodes.map((n) => n.id);
    const links: GLink[] = g.edges.map((e) => ({
      source: byIdx[e.s],
      target: byIdx[e.t],
      relKind: g.rel_kind_index[e.k ?? 0] ?? "?",
      weight: e.w ?? 1,
      firstSeen: e.fs,
    }));
    return { nodes, links };
  }, [graph.data, isolate, neighborhood.data]);

  // RFC 0136 §4 — above the client-side simulation threshold, ask the console to precompute
  // ForceAtlas2 positions server-side rather than let the browser's own force simulation choke.
  const needsServerLayout = nodes.length > SERVER_LAYOUT_THRESHOLD;
  const layout = useQuery({
    queryKey: ["graph-layout", id, params.toString(), isolate?.id, isolate?.depth],
    queryFn: () =>
      apiPost<{ positions: Record<string, [number, number]> }>(
        `/workspaces/${id}/graph/layout`,
        {
          nodes: nodes.map((n) => n.id),
          edges: links.map((l) => [
            typeof l.source === "string" ? l.source : (l.source as GNode).id,
            typeof l.target === "string" ? l.target : (l.target as GNode).id,
          ]),
        },
      ),
    enabled: needsServerLayout,
  });

  // Pins nodes at their server-computed position (the same `fx`/`fy` fields the client-side
  // simulation's own `onEngineStop` uses to freeze itself) — `GraphCanvas`/d3-force then skips
  // simulating those nodes entirely, matching `react-force-graph`'s documented pre-positioned
  // behavior. Falls back to the plain `nodes` (client-simulated) below the threshold or before
  // the layout call resolves.
  const positionedNodes = useMemo(() => {
    if (!needsServerLayout || !layout.data) return nodes;
    const positions = layout.data.positions;
    return nodes.map((n) => {
      const p = positions[n.id];
      return p ? { ...n, fx: p[0], fy: p[1] } : n;
    });
  }, [nodes, needsServerLayout, layout.data]);

  // RFC 0136 §3 — id -> hop distance, or null when impact mode isn't active.
  const impactHops = impact && impactQuery.data ? impactHopMap(impactQuery.data) : null;

  // Node count visible at the current `asOf` — for the "viewing as of" banner (monotonic graph,
  // so this is just a filter, never a refetch).
  const visibleCount = useMemo(
    () => (asOf ? nodes.filter((n) => !n.firstSeen || n.firstSeen <= asOf).length : nodes.length),
    [nodes, asOf],
  );

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
          {isolate ? (
            <button className="pill" onClick={() => setIsolate(null)}>
              ↑ back to full graph
            </button>
          ) : expanded ? (
            <button className="pill" onClick={() => setFocusKind(null)}>
              ↑ overview
            </button>
          ) : (
            <span className="muted">click a bubble to drill in</span>
          )}
          {impact && (
            <button className="pill" onClick={() => setImpact(null)} style={{ marginLeft: "0.4rem" }}>
              ✕ clear impact trace
            </button>
          )}
        </span>
        <span>
          <button
            className="pill"
            onClick={() => {
              navigator.clipboard?.writeText(window.location.href).then(() => {
                setLinkCopied(true);
                window.setTimeout(() => setLinkCopied(false), 1500);
              });
            }}
            title="copy a link to this exact view (time-travel position + selected object)"
          >
            {linkCopied ? "✓ copied" : "🔗 copy link"}
          </button>
          <button
            className="pill"
            onClick={() => canvasRef.current?.exportPng()}
            title="save the current view as a PNG"
            style={{ marginLeft: "0.4rem" }}
          >
            ⬇ PNG
          </button>
          <button
            className="pill"
            onClick={() => canvasRef.current?.exportGltf()}
            title="save the current view as a glTF model"
            style={{ marginLeft: "0.4rem" }}
          >
            ⬇ glTF
          </button>
          <button
            className="pill"
            onClick={() => setFullscreen((f) => !f)}
            style={{ marginLeft: "0.4rem" }}
          >
            {fullscreen ? "✕ exit fullscreen" : "⛶ fullscreen"}
          </button>
        </span>
      </div>

      <div className="graph-layout">
        <aside className="graph-side">
          <strong>
            {isolate
              ? `Isolated neighbourhood (depth ${isolate.depth})`
              : expanded
                ? `Objects · focus: ${focusKind}`
                : "Overview — by kind"}
          </strong>
          {impact && (
            <p className="muted">
              impact trace: {impact.direction} of <code>{impact.id.slice(0, 8)}…</code>
              {!expanded && !isolate && (
                <> — switch to object-level view (or isolate this object) to see it highlighted</>
              )}
            </p>
          )}
          {needsServerLayout && (
            <p className="muted">
              {nodes.length} nodes — server-computed layout{" "}
              {layout.isLoading ? "(computing…)" : "(cached)"}
            </p>
          )}
          {graph.data?.truncated?.nodes && !isolate && (
            <p className="warn-line">
              showing the {graph.data.truncated.node_limit} most-connected of{" "}
              {graph.data.counts.objects_after_filter ?? "?"} — raise min-degree to thin it
            </p>
          )}

          {isolate && (
            <label className="muted" style={{ display: "block", margin: "0.5rem 0" }}>
              neighbourhood depth: {isolate.depth}
              <input
                type="range"
                min={1}
                max={3}
                value={isolate.depth}
                onChange={(e) => setIsolate({ id: isolate.id, depth: Number(e.target.value) })}
                style={{ width: "100%" }}
              />
            </label>
          )}

          {expanded && !isolate && (
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

          {!isolate && (
            <>
              <p className="muted" style={{ marginTop: "0.75rem" }}>
                relationship kinds
              </p>
              {(graph.data && graph.data.rel_kind_index.length
                ? graph.data.rel_kind_index
                : ALL_RELS
              ).map((rk) => (
                <label key={rk} className="flt">
                  <input
                    type="checkbox"
                    checked={!excludedRels.has(rk)}
                    onChange={(e) => toggleRel(rk, e.target.checked)}
                  />{" "}
                  {rk}
                </label>
              ))}
            </>
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

          {!isolate && (
            <>
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
            </>
          )}
        </aside>

        <div className="graph-canvas">
          {(isolate ? neighborhood.isError : graph.isError) && (
            <p className="err">{String(isolate ? neighborhood.error : graph.error)}</p>
          )}
          {(isolate ? neighborhood.isLoading : graph.isLoading) && (
            <p className="muted">{isolate ? "loading neighbourhood…" : "loading graph…"}</p>
          )}
          {impact?.id && impactQuery.isError && (
            <p className="err">impact trace failed: {String(impactQuery.error)}</p>
          )}
          {nodes.length > 0 && (
            <Suspense fallback={<p className="muted">loading renderer…</p>}>
              <GraphCanvas
                ref={canvasRef}
                nodes={positionedNodes}
                links={links}
                focusId={focusId}
                selectedId={selectedId}
                dimKind={!isolate && expanded ? focusKind : null}
                asOf={isolate ? null : asOf}
                impactHops={impactHops}
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

          {asOf && !isolate && (
            <div className="graph-asof-banner">
              viewing as of <strong>{asOf.slice(0, 10)}</strong> · {visibleCount} of {nodes.length}{" "}
              nodes
              <button className="linkish" onClick={() => setAsOf(null)}>
                back to live
              </button>
            </div>
          )}

          {!isolate && graph.data && timeline.data && (
            <GraphTimeline points={timeline.data.points} value={asOf} onChange={setAsOf} />
          )}
        </div>

        {selectedId && (
          <ObjectPanel
            workspace={id}
            objectId={selectedId}
            asOf={isolate ? null : asOf}
            onClose={() => setSelectedId(null)}
            onGoto={gotoObject}
            onIsolate={(oid) => {
              setImpact(null);
              setIsolate({ id: oid, depth: 1 });
            }}
            onImpact={(oid, direction) => setImpact({ id: oid, direction })}
          />
        )}
      </div>
    </div>
  );
}
