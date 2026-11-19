import { useQuery } from "@tanstack/react-query";
import { useState } from "react";
import { Link, useOutletContext } from "react-router-dom";
import { Icon } from "../Icon";
import { api } from "../api/client";
import { type Me, seesAll } from "../WorkspaceShell";
import { ObjectNotes } from "./ObjectNotes";
import { ObjectTestimony } from "./ObjectTestimony";

// RFC 0257: the kinds whose recovered Transformation IR graph `ekos transformation graph` draws.
const CHAIN_KINDS = new Set(["View", "Procedure", "TransformNode"]);

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

/** The design's confidence words: high from 0.8, medium from 0.5, else low. */
export function confidenceLabel(c: number | undefined): { text: string; cls: string } | null {
  if (c == null) return null;
  if (c >= 0.8) return { text: "confidence high", cls: "chip ok" };
  if (c >= 0.5) return { text: "confidence medium", cls: "chip warn" };
  return { text: "confidence low", cls: "chip bad" };
}

type Tab = "evidence" | "relationships" | "notes" | "properties";

export function ObjectPanel({
  workspace,
  objectId,
  asOf,
  disputed = false,
  onClose,
  onGoto,
  onIsolate,
  onImpact,
}: {
  workspace: string;
  objectId: string;
  asOf?: string | null; // RFC 0134 — reconstruct the object's state as of this instant
  /** RFC 0174: the object has an open conflict. */
  disputed?: boolean;
  onClose: () => void;
  onGoto: (id: string) => void;
  // RFC 0136 §2/§3 — undefined hides the action entirely (e.g. while already in isolate/impact
  // mode for a different object, `Graph.tsx` still renders a panel but doesn't wire these in).
  onIsolate?: (id: string) => void;
  onImpact?: (id: string, direction: "dependents" | "dependencies") => void;
}) {
  // RFC 0247: notes are for someone who sees the whole project, and describe the entity now.
  const wholeProject = seesAll(useOutletContext<Me | undefined>());
  const [tab, setTab] = useState<Tab>("evidence");
  const state = useQuery({
    queryKey: ["object", workspace, objectId, asOf ?? "live"],
    queryFn: () =>
      api<StateOut>(
        `/workspaces/${workspace}/objects/${objectId}${asOf ? `?as_of=${encodeURIComponent(asOf)}` : ""}`,
      ),
  });

  const o = state.data?.object;
  const rels = state.data?.relationships ?? [];
  const evidence = state.data?.evidence ?? [];
  const notes = wholeProject && !asOf;
  const tabs: [Tab, string][] = [
    ["evidence", `Evidence (${evidence.length})`],
    ["relationships", `Relationships (${rels.length})`],
    ...(notes ? ([["notes", "Notes"]] as [Tab, string][]) : []),
    ["properties", "Properties"],
  ];

  return (
    <aside className="obj-panel" aria-label="object">
      <div className="obj-head">
        <div className="obj-title">
          <strong>{o?.name ?? "…"}</strong>
          <button type="button" className="icon-btn" aria-label="Close the panel" onClick={onClose}>
            <Icon name="close" />
          </button>
        </div>
        {o && (
          <div className="chips">
            <span className="chip accent">{o.kind}</span>
            <code className="obj-id" title={o.id}>
              {o.id}
            </code>
          </div>
        )}
        {asOf && <p className="muted">state as of {asOf.slice(0, 10)}</p>}
      </div>
      {state.isError && <p className="err">{String(state.error)}</p>}
      {o && (
        <>
          {(onIsolate || onImpact) && (
            <div className="obj-actions">
              {onIsolate && (
                <button className="pill" onClick={() => onIsolate(o.id)}>
                  Isolate neighbourhood
                </button>
              )}
              {onImpact && (
                <>
                  <button className="pill" onClick={() => onImpact(o.id, "dependents")}>
                    What depends on this
                  </button>
                  <button className="pill" onClick={() => onImpact(o.id, "dependencies")}>
                    What this depends on
                  </button>
                </>
              )}
            </div>
          )}

          <div className="tabs" role="tablist" aria-label="about this object">
            {tabs.map(([id, label]) => (
              <button
                key={id}
                type="button"
                role="tab"
                aria-selected={tab === id}
                className={tab === id ? "tab active" : "tab"}
                onClick={() => setTab(id)}
              >
                {label}
              </button>
            ))}
          </div>

          {disputed && (
            <div className="obj-banner" role="status">
              <Icon name="conflicts" />
              <span>Sources disagree about this object</span>
              <Link to={`/w/${workspace}/conflicts`}>
                Decide <Icon name="chevronRight" size={12} />
              </Link>
            </div>
          )}

          {tab === "evidence" && (
            <ul className="ev-cards">
              {evidence.map((e, i) => (
                <EvidenceCard key={i} e={e} />
              ))}
              {evidence.length === 0 && <li className="muted">no evidence recorded</li>}
            </ul>
          )}

          {tab === "relationships" && (
            <ul className="rel-list">
              {rels.map((r, i) => {
                const other = r.from === o.id ? r.to : r.from;
                return (
                  <li key={i}>
                    <span className="chip">{r.kind}</span>{" "}
                    {other && (
                      <button className="linkish" onClick={() => onGoto(other)} title={other}>
                        {other.slice(0, 8)}…
                      </button>
                    )}
                  </li>
                );
              })}
              {rels.length === 0 && <li className="muted">no relationships</li>}
            </ul>
          )}

          {tab === "notes" && (
            <>
              <ObjectNotes workspace={workspace} objectId={o.id} enabled={notes} />
              <ObjectTestimony workspace={workspace} objectId={o.id} enabled={notes} />
            </>
          )}

          {tab === "properties" &&
            (o.properties && Object.keys(o.properties).length > 0 ? (
              <pre className="props">{JSON.stringify(o.properties, null, 2)}</pre>
            ) : (
              <p className="muted">no properties</p>
            ))}

          {wholeProject && !asOf && CHAIN_KINDS.has(o.kind) && (
            <p>
              <Link to={`/w/${workspace}/transformations?object=${o.id}`}>Open as a chain of steps</Link>
            </p>
          )}
        </>
      )}
      {state.isLoading && <p className="muted">loading…</p>}
    </aside>
  );
}

function EvidenceCard({ e }: { e: Evidence }) {
  const loc = e.location ?? e.source_location ?? {};
  const where = loc.path ? `${loc.path}${loc.line != null ? `:${loc.line}` : ""}` : null;
  const conf = confidenceLabel(e.confidence);
  const text = e.fragment ?? e.excerpt;
  const [copied, setCopied] = useState(false);
  return (
    <li className="ev-card">
      {text && <pre className="frag">{text}</pre>}
      {where && <code className="ev-where">{where}</code>}
      <div className="ev-foot">
        {e.analyzer && <span className="chip">{e.analyzer}</span>}
        {conf && (
          <span className={conf.cls} title={`${((e.confidence ?? 0) * 100).toFixed(0)}%`}>
            {conf.text}
          </span>
        )}
        <span className="topbar-gap" />
        {where && (
          <button
            type="button"
            className="small-btn"
            onClick={() => {
              // RFC 0261 §2.5: the desktop app opens the editor here (D2); a browser copies.
              void navigator.clipboard?.writeText(where).then(() => setCopied(true));
            }}
          >
            <Icon name="code" size={13} /> {copied ? "Copied" : "Copy location"}
          </button>
        )}
      </div>
    </li>
  );
}
