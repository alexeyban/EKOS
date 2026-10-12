import { useQuery } from "@tanstack/react-query";
import { lazy, Suspense, useEffect, useMemo, useRef, useState } from "react";
import { useParams, useSearchParams } from "react-router-dom";
import { api, apiPost } from "../api/client";
import type { Conflict } from "../api/types";
import { disputedIds } from "./conflicts-shared";
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

/** Query parameters for `/graph`: an object-level page when a kind is expanded, else the
 * per-kind aggregate overview. */
function graphParams(expanded: boolean, minDegree: number, excludedRels: Set<string>) {
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
  return params;
}

/** The compact `/graph` wire format (index-coded kinds and endpoints) → canvas nodes and links. */
function exportToGraph(g: GraphOut): { nodes: GNode[]; links: GLink[] } {
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
}

/** Pin nodes at server-computed positions; nodes without one stay client-simulated. */
function pinPositions(nodes: GNode[], positions: Record<string, [number, number]>): GNode[] {
  return nodes.map((n) => {
    const p = positions[n.id];
    return p ? { ...n, fx: p[0], fy: p[1] } : n;
  });
}

/** The URL's view parameters for this time-travel position and selected object. */
function withViewParams(prev: URLSearchParams, asOf: string | null, selectedId: string | null) {
  const next = new URLSearchParams(prev);
  if (asOf) next.set("as_of", asOf);
  else next.delete("as_of");
  if (selectedId) next.set("focus", selectedId);
  else next.delete("focus");
  return next;
}

/** The graph on screen: an isolated neighbourhood when one is active, else the exported graph. */
function shownGraph(
  isolated: boolean,
  neighborhood: NeighborhoodOut | undefined,
  exported: GraphOut | undefined,
): { nodes: GNode[]; links: GLink[] } {
  if (isolated) return neighborhood ? neighborhoodToGraph(neighborhood) : { nodes: [], links: [] };
  return exported ? exportToGraph(exported) : { nodes: [], links: [] };
}

/** `excluded` with one relationship kind switched on or off. */
function withRel(excluded: Set<string>, rk: string, on: boolean): Set<string> {
  const next = new Set(excluded);
  if (on) next.delete(rk);
  else next.add(rk);
  return next;
}

interface ToolbarProps {
  isolated: boolean;
  expanded: boolean;
  tracing: boolean;
  linkCopied: boolean;
  fullscreen: boolean;
  onBackToFull: () => void;
  onOverview: () => void;
  onClearImpact: () => void;
  onCopyLink: () => void;
  onPng: () => void;
  onGltf: () => void;
  onToggleFullscreen: () => void;
}

/** The way back up from the current level: out of an isolated neighbourhood, or to the overview. */
function ToolbarBack(p: ToolbarProps) {
  if (p.isolated) {
    return (
      <button className="pill" onClick={p.onBackToFull}>
        ↑ back to full graph
      </button>
    );
  }
  if (p.expanded) {
    return (
      <button className="pill" onClick={p.onOverview}>
        ↑ overview
      </button>
    );
  }
  return <span className="muted">click a bubble to drill in</span>;
}

function GraphToolbar(p: ToolbarProps) {
  return (
    <div className="graph-toolbar">
      <span>
        <ToolbarBack {...p} />
        {p.tracing && (
          <button className="pill" onClick={p.onClearImpact} style={{ marginLeft: "0.4rem" }}>
            ✕ clear impact trace
          </button>
        )}
      </span>
      <span>
        <button
          className="pill"
          onClick={p.onCopyLink}
          title="copy a link to this exact view (time-travel position + selected object)"
        >
          {p.linkCopied ? "✓ copied" : "🔗 copy link"}
        </button>
        <button
          className="pill"
          onClick={p.onPng}
          title="save the current view as a PNG"
          style={{ marginLeft: "0.4rem" }}
        >
          ⬇ PNG
        </button>
        <button
          className="pill"
          onClick={p.onGltf}
          title="save the current view as a glTF model"
          style={{ marginLeft: "0.4rem" }}
        >
          ⬇ glTF
        </button>
        <button
          className="pill"
          onClick={p.onToggleFullscreen}
          style={{ marginLeft: "0.4rem" }}
        >
          {p.fullscreen ? "✕ exit fullscreen" : "⛶ fullscreen"}
        </button>
      </span>
    </div>
  );
}

type Isolate = { id: string; depth: number } | null;
type Impact = { id: string; direction: "dependents" | "dependencies" } | null;

interface SidebarProps {
  isolate: Isolate;
  expanded: boolean;
  focusKind: string | null;
  impact: Impact;
  needsServerLayout: boolean;
  nodeCount: number;
  layoutLoading: boolean;
  graphData: GraphOut | undefined;
  minDegree: number;
  excludedRels: Set<string>;
  q: string;
  matches: { id: string; name: string }[];
  overviewNodes: GraphOut["nodes"];
  kinds: string[];
  setIsolate: (v: Isolate) => void;
  setMinDegree: (v: number) => void;
  toggleRel: (rk: string, on: boolean) => void;
  setQ: (v: string) => void;
  gotoObject: (id: string) => void;
  setFocusKind: (v: string | null) => void;
}

function sidebarTitle(p: SidebarProps): string {
  if (p.isolate) return `Isolated neighbourhood (depth ${p.isolate.depth})`;
  if (p.expanded) return `Objects · focus: ${p.focusKind}`;
  return "Overview — by kind";
}

/** What the current view is and is not showing: impact trace, layout source, truncation. */
function SidebarNotes(p: SidebarProps) {
  const truncated = p.isolate ? undefined : p.graphData?.truncated;
  return (
    <>
      {p.impact && (
        <p className="muted">
          impact trace: {p.impact.direction} of <code>{p.impact.id.slice(0, 8)}…</code>
          {!p.expanded && !p.isolate && (
            <> — switch to object-level view (or isolate this object) to see it highlighted</>
          )}
        </p>
      )}
      {p.needsServerLayout && (
        <p className="muted">
          {p.nodeCount} nodes — server-computed layout{" "}
          {p.layoutLoading ? "(computing…)" : "(cached)"}
        </p>
      )}
      {truncated?.nodes && (
        <p className="warn-line">
          showing the {truncated.node_limit} most-connected of{" "}
          {p.graphData?.counts.objects_after_filter ?? "?"} — raise min-degree to thin it
        </p>
      )}
    </>
  );
}

/** The depth slider of an isolated neighbourhood, or the degree and relationship filters. */
function SidebarFilters(p: SidebarProps) {
  if (p.isolate) {
    const isolate = p.isolate;
    return (
      <label className="muted" style={{ display: "block", margin: "0.5rem 0" }}>
        neighbourhood depth: {isolate.depth}
        <input
          type="range"
          min={1}
          max={3}
          value={isolate.depth}
          onChange={(e) => p.setIsolate({ id: isolate.id, depth: Number(e.target.value) })}
          style={{ width: "100%" }}
        />
      </label>
    );
  }
  const relKinds = p.graphData?.rel_kind_index.length ? p.graphData.rel_kind_index : ALL_RELS;
  return (
    <>
      {p.expanded && (
        <label className="muted" style={{ display: "block", margin: "0.5rem 0" }}>
          min degree: {p.minDegree}
          <input
            type="range"
            min={0}
            max={15}
            value={p.minDegree}
            onChange={(e) => p.setMinDegree(Number(e.target.value))}
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
            checked={!p.excludedRels.has(rk)}
            onChange={(e) => p.toggleRel(rk, e.target.checked)}
          />{" "}
          {rk}
        </label>
      ))}
    </>
  );
}

/** The kinds to drill into (overview) or to focus (object level). Not shown when isolated. */
function SidebarKinds(p: SidebarProps) {
  if (p.isolate) return null;
  return (
    <>
      <p className="muted" style={{ marginTop: "0.75rem" }}>
        {p.expanded ? "focus a kind" : "click a bubble (or a name) to drill in"}
      </p>
      <ul className="kinds">
        {p.overviewNodes.map((n) => (
          <li key={n.id}>
            <button
              className="linkish"
              onClick={() => p.setFocusKind(n.n ?? null)}
              style={{ color: colorFor(n.k ?? 0) }}
            >
              {n.n}
            </button>{" "}
            <span className="muted">{n.count}</span>
          </li>
        ))}
        {p.kinds.map((k) => (
          <li key={k}>
            <button
              className="linkish"
              onClick={() => p.setFocusKind(k)}
              style={{ fontWeight: k === p.focusKind ? 700 : 400 }}
            >
              {k}
            </button>
          </li>
        ))}
      </ul>
    </>
  );
}

function GraphSidebar(p: SidebarProps) {
  return (
    <aside className="graph-side">
      <strong>{sidebarTitle(p)}</strong>
      <SidebarNotes {...p} />
      <SidebarFilters {...p} />

      <p className="muted" style={{ marginTop: "0.75rem" }}>
        search
      </p>
      <input
        value={p.q}
        onChange={(e) => p.setQ(e.target.value)}
        placeholder="2–3 keywords"
        style={{ width: "100%" }}
      />
      <ul className="srch">
        {p.matches.map((m) => (
          <li key={m.id}>
            <button className="linkish" onClick={() => p.gotoObject(m.id)}>
              {m.name}
            </button>
          </li>
        ))}
      </ul>

      <SidebarKinds {...p} />
    </aside>
  );
}

/** Load and error lines for whichever fetch feeds the canvas, and for the impact trace. */
function GraphStatus(p: {
  isolated: boolean;
  feed: { isError: boolean; isLoading: boolean; error: unknown };
  impactError: unknown;
}) {
  return (
    <>
      {p.feed.isError && <p className="err">{String(p.feed.error)}</p>}
      {p.feed.isLoading && (
        <p className="muted">{p.isolated ? "loading neighbourhood…" : "loading graph…"}</p>
      )}
      {p.impactError !== null && (
        <p className="err">impact trace failed: {String(p.impactError)}</p>
      )}
    </>
  );
}

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
      (prev) => withViewParams(prev, asOf, selectedId),
      { replace: true },
    );
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [asOf, selectedId]);
  // RFC 0136 §2 — set replaces the normal graph fetch with just this object's BFS neighbourhood.
  const [isolate, setIsolate] = useState<Isolate>(null);
  // RFC 0136 §3 — set overlays a hop-distance trace on whatever graph is currently on screen.
  const [impact, setImpact] = useState<Impact>(null);
  const canvasRef = useRef<GraphCanvasHandle>(null);

  const expanded = focusKind !== null;

  const params = graphParams(expanded, minDegree, excludedRels);

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

  // RFC 0174 — objects with an open conflict get a ring. A failed read just draws no rings.
  const openConflicts = useQuery({
    queryKey: ["conflicts", id, "open"],
    queryFn: () => api<Conflict[]>(`/workspaces/${id}/conflicts?status=open`),
    enabled: id !== "",
    retry: false,
  });
  const disputed = useMemo(() => disputedIds(openConflicts.data), [openConflicts.data]);

  // RFC 0136 §3 — a hop-distance node list, overlaid on whatever graph is already on screen.
  const impactQuery = useQuery({
    queryKey: ["impact", id, impact?.id, impact?.direction],
    queryFn: () =>
      api<ImpactOut>(
        `/workspaces/${id}/impact/${impact?.id}?direction=${impact?.direction}&max_hops=5`,
      ),
    enabled: impact !== null,
  });

  const { nodes, links } = useMemo(
    () => shownGraph(isolate !== null, neighborhood.data, graph.data),
    [graph.data, isolate, neighborhood.data],
  );

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
  const positionedNodes = useMemo(
    () => (needsServerLayout && layout.data ? pinPositions(nodes, layout.data.positions) : nodes),
    [nodes, needsServerLayout, layout.data],
  );

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

  const toggleRel = (rk: string, on: boolean) => setExcludedRels((s) => withRel(s, rk, on));

  const copyLink = () => {
    navigator.clipboard
      ?.writeText(window.location.href)
      .then(() => {
        setLinkCopied(true);
        window.setTimeout(() => setLinkCopied(false), 1500);
      })
      // Clipboard access can be denied (permissions, insecure context); the link is
      // still in the address bar, so there is nothing to recover — just don't claim it.
      .catch(() => setLinkCopied(false));
  };

  const kinds = useMemo(
    () => (expanded ? [...new Set(nodes.map((n) => n.kind))].sort((a, b) => a.localeCompare(b)) : []),
    [expanded, nodes],
  );

  const overviewNodes = graph.data && !expanded ? [...graph.data.nodes] : [];
  overviewNodes.sort((a, b) => (b.count ?? 0) - (a.count ?? 0));

  return (
    <div className={fullscreen ? "graph-page fs" : "graph-page"}>
      <GraphToolbar
        isolated={isolate !== null}
        expanded={expanded}
        tracing={impact !== null}
        linkCopied={linkCopied}
        fullscreen={fullscreen}
        onBackToFull={() => setIsolate(null)}
        onOverview={() => setFocusKind(null)}
        onClearImpact={() => setImpact(null)}
        onCopyLink={copyLink}
        onPng={() => canvasRef.current?.exportPng()}
        onGltf={() => canvasRef.current?.exportGltf()}
        onToggleFullscreen={() => setFullscreen((f) => !f)}
      />

      <div className="graph-layout">
        <GraphSidebar
          isolate={isolate}
          expanded={expanded}
          focusKind={focusKind}
          impact={impact}
          needsServerLayout={needsServerLayout}
          nodeCount={nodes.length}
          layoutLoading={layout.isLoading}
          graphData={graph.data}
          minDegree={minDegree}
          excludedRels={excludedRels}
          q={q}
          matches={search.data?.matches ?? []}
          overviewNodes={overviewNodes}
          kinds={kinds}
          setIsolate={setIsolate}
          setMinDegree={setMinDegree}
          toggleRel={toggleRel}
          setQ={setQ}
          gotoObject={gotoObject}
          setFocusKind={setFocusKind}
        />

        <div className="graph-canvas">
          <GraphStatus
            isolated={isolate !== null}
            feed={isolate ? neighborhood : graph}
            impactError={impact?.id && impactQuery.isError ? impactQuery.error : null}
          />
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
                disputed={disputed}
                onNodeClick={(n) => (n.isAggregate ? setFocusKind(n.label) : gotoObject(n.id))}
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
