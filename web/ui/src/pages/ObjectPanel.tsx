import { useQuery } from "@tanstack/react-query";
import { api } from "../api/client";

interface Evidence {
  location?: { path?: string; line?: number };
  source_location?: { path?: string; line?: number };
  fragment?: string;
  excerpt?: string;
  confidence?: number;
  analyzer?: string;
  source?: string;
}
interface Relationship {
  from?: string;
  to?: string;
  kind?: string;
}
interface StateOut {
  object?: {
    id: string;
    name: string;
    kind: string;
    properties?: Record<string, unknown>;
  };
  relationships?: Relationship[];
  evidence?: Evidence[];
}

export function ObjectPanel({
  workspace,
  objectId,
  onClose,
  onGoto,
}: {
  workspace: string;
  objectId: string;
  onClose: () => void;
  onGoto: (id: string) => void;
}) {
  const state = useQuery({
    queryKey: ["object", workspace, objectId],
    queryFn: () => api<StateOut>(`/workspaces/${workspace}/objects/${objectId}`),
  });

  const o = state.data?.object;
  const rels = state.data?.relationships ?? [];
  const evidence = state.data?.evidence ?? [];

  return (
    <aside className="obj-panel">
      <button className="linkish" onClick={onClose} style={{ float: "right" }}>
        close ✕
      </button>
      {state.isError && <p className="err">{String(state.error)}</p>}
      {o && (
        <>
          <strong>{o.name}</strong>
          <p className="muted">
            <span className="chip">{o.kind}</span>{" "}
            <code style={{ fontSize: "0.7rem" }}>{o.id}</code>
          </p>

          {o.properties && Object.keys(o.properties).length > 0 && (
            <details open>
              <summary>Properties</summary>
              <pre className="props">{JSON.stringify(o.properties, null, 2)}</pre>
            </details>
          )}

          <details open>
            <summary>Relationships ({rels.length})</summary>
            <ul className="rel-list">
              {rels.map((r, i) => {
                const other = r.from === o.id ? r.to : r.from;
                return (
                  <li key={i}>
                    <span className="muted">{r.kind}</span>{" "}
                    {other && (
                      <button className="linkish" onClick={() => onGoto(other)}>
                        {other.slice(0, 8)}…
                      </button>
                    )}
                  </li>
                );
              })}
            </ul>
          </details>

          <details open>
            <summary>Evidence ({evidence.length})</summary>
            <ul className="ev-list">
              {evidence.map((e, i) => {
                const loc = e.location ?? e.source_location ?? {};
                return (
                  <li key={i}>
                    <code>
                      {loc.path}
                      {loc.line != null && `:${loc.line}`}
                    </code>
                    {e.analyzer && <span className="muted"> · {e.analyzer}</span>}
                    {e.confidence != null && (
                      <span className="muted"> · {(e.confidence * 100).toFixed(0)}%</span>
                    )}
                    {(e.fragment ?? e.excerpt) && (
                      <pre className="frag">{e.fragment ?? e.excerpt}</pre>
                    )}
                  </li>
                );
              })}
              {evidence.length === 0 && <li className="muted">no evidence recorded</li>}
            </ul>
          </details>
        </>
      )}
      {state.isLoading && <p className="muted">loading…</p>}
    </aside>
  );
}
