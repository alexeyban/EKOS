// RFC 0253 §3.3 — the inbox: files and links people sent, held for the next build.
//
// Anyone who reads the project may send; what someone without the reviewer role sends waits as
// pending until a reviewer accepts it. Purging deletes the original only: the redacted text
// already compiled stays in the ledger, which cannot forget.

import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { useOutletContext, useParams } from "react-router-dom";
import { api, apiPost } from "../api/client";
import type { InboxItem } from "../api/types";
import { type Me, mayAdmin, mayReview } from "../WorkspaceShell";
import { fileToBase64, inboxOrder, sentBy, sizeText } from "./people-shared";
import { loadReviewer, saveReviewer } from "./semantics-shared";

export function Inbox() {
  const me = useOutletContext<Me>();
  const { id = "" } = useParams();
  const qc = useQueryClient();
  const [status, setStatus] = useState("");
  const [reviewer, setReviewer] = useState(loadReviewer());
  const list = useQuery({
    queryKey: ["inbox", id, status],
    queryFn: () =>
      api<InboxItem[]>(
        `/workspaces/${id}/inbox${status ? `?status=${status}` : ""}`,
      ),
    enabled: id !== "",
  });
  const named = me.mode !== "token" || reviewer.trim() !== "";
  const done = () => {
    if (me.mode === "token") saveReviewer(reviewer);
    void qc.invalidateQueries({ queryKey: ["inbox", id] });
  };
  const who = me.mode === "token" ? reviewer : null;
  const accept = useMutation({
    mutationFn: (item: string) =>
      apiPost(`/workspaces/${id}/inbox/${item}/accept`, { reviewer: who }),
    onSuccess: done,
  });
  const purge = useMutation({
    mutationFn: (item: string) =>
      apiPost(`/workspaces/${id}/inbox/${item}/purge`, {
        reviewer: who,
        confirm: item,
      }),
    onSuccess: done,
  });
  const items = inboxOrder(list.data ?? []);

  return (
    <div className="sem-page">
      <div className="sem-views">
        <select
          aria-label="status"
          value={status}
          onChange={(e) => setStatus(e.target.value)}
        >
          <option value="">pending and accepted</option>
          <option value="pending">pending</option>
          <option value="accepted">accepted</option>
        </select>
        <span className="muted sem-note">
          What people sent: files and links, compiled on the next build once
          accepted. Originals stay outside the ledger.
        </span>
      </div>
      {me.mode === "token" && (
        <label className="sem-field">
          <span>
            Your name (token mode has no identity; recorded as
            token:&lt;name&gt;)
          </span>
          <input
            aria-label="reviewer"
            value={reviewer}
            onChange={(e) => setReviewer(e.target.value)}
          />
        </label>
      )}
      <Send workspace={id} reviewer={who} named={named} onSent={done} />
      <section className="card">
        {list.isLoading && <p className="muted">loading…</p>}
        {list.isError && <p className="err">{String(list.error)}</p>}
        {list.data?.length === 0 && (
          <p className="muted">The inbox is empty.</p>
        )}
        <table className="lm-table">
          <tbody>
            {items.map((i) => (
              <InboxRow
                key={i.id}
                item={i}
                canAccept={mayReview(me) && named}
                canPurge={mayAdmin(me) && named}
                busy={accept.isPending || purge.isPending}
                onAccept={() => accept.mutate(i.id)}
                onPurge={() => purge.mutate(i.id)}
              />
            ))}
          </tbody>
        </table>
        {accept.isError && <p className="err">{String(accept.error)}</p>}
        {purge.isError && <p className="err">{String(purge.error)}</p>}
      </section>
    </div>
  );
}

function InboxRow({
  item: i,
  canAccept,
  canPurge,
  busy,
  onAccept,
  onPurge,
}: {
  item: InboxItem;
  canAccept: boolean;
  canPurge: boolean;
  busy: boolean;
  onAccept: () => void;
  onPurge: () => void;
}) {
  const [confirming, setConfirming] = useState(false);
  const [typed, setTyped] = useState("");
  return (
    <tr>
      <td>
        <span className={i.status === "pending" ? "chip warn" : "chip ok"}>
          {i.status}
        </span>
      </td>
      <td>
        <strong>{i.name}</strong>
        <div className="muted small">
          {sizeText(i.size)} · from {sentBy(i)} · {i.received_at}
          {i.accepted_by ? ` · accepted by ${i.accepted_by}` : ""}
        </div>
      </td>
      <td className="btnrow">
        {i.status === "pending" && canAccept && (
          <button disabled={busy} onClick={onAccept}>
            Accept
          </button>
        )}
        {canPurge && !confirming && (
          <button className="danger" onClick={() => setConfirming(true)}>
            Purge…
          </button>
        )}
        {canPurge && confirming && (
          <span>
            <input
              aria-label={`type ${i.id} to purge`}
              placeholder={i.id}
              value={typed}
              onChange={(e) => setTyped(e.target.value)}
            />
            <button
              className="danger"
              disabled={busy || typed.trim() !== i.id}
              onClick={onPurge}
            >
              Purge the original
            </button>
            <button className="linkish" onClick={() => setConfirming(false)}>
              cancel
            </button>
          </span>
        )}
      </td>
    </tr>
  );
}

function Send({
  workspace,
  reviewer,
  named,
  onSent,
}: {
  workspace: string;
  reviewer: string | null;
  named: boolean;
  onSent: () => void;
}) {
  const [file, setFile] = useState<File | null>(null);
  const [url, setUrl] = useState("");
  const send = useMutation({
    mutationFn: async () => {
      const body = file
        ? {
            name: file.name,
            content_base64: await fileToBase64(file),
            reviewer,
          }
        : { url: url.trim(), reviewer };
      return apiPost<{ name: string; status: string }>(
        `/workspaces/${workspace}/inbox`,
        body,
      );
    },
    onSuccess: () => {
      setFile(null);
      setUrl("");
      onSent();
    },
  });
  return (
    <section className="card">
      <strong>Send material</strong>
      <div className="btnrow">
        <input
          type="file"
          aria-label="file"
          onChange={(e) => setFile(e.target.files?.[0] ?? null)}
        />
        <span className="muted">or</span>
        <input
          aria-label="link"
          placeholder="https://…"
          value={url}
          onChange={(e) => setUrl(e.target.value)}
        />
        <button
          disabled={!named || send.isPending || (!file && !url.trim())}
          onClick={() => send.mutate()}
        >
          Send
        </button>
      </div>
      {send.isError && <p className="err">{String(send.error)}</p>}
      {send.isSuccess && (
        <p className="ok-line">
          Stored {send.data.name} ({send.data.status}).
        </p>
      )}
    </section>
  );
}
