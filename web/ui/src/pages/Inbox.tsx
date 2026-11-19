// RFC 0253 §3.3 — the inbox: files and links people sent, held for the next build.
//
// Anyone who reads the project may send; what someone without the reviewer role sends waits as
// pending until a reviewer accepts it. Purging deletes the original only: the redacted text
// already compiled stays in the ledger, which cannot forget.

import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { type ReactNode, useState } from "react";
import { useOutletContext, useParams } from "react-router-dom";
import { api, apiPost } from "../api/client";
import type { InboxItem } from "../api/types";
import { Empty, ErrorNote, PageHead, QueryView } from "../ui";
import { type Me, mayAdmin, mayReview } from "../WorkspaceShell";
import { ReviewerField } from "./decide-kit";
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
      api<InboxItem[]>(`/workspaces/${id}/inbox${status ? `?status=${status}` : ""}`),
    enabled: id !== "",
  });
  const named = me.mode === "oidc" || reviewer.trim() !== "";
  const done = () => {
    if (me.mode !== "oidc") saveReviewer(reviewer);
    void qc.invalidateQueries({ queryKey: ["inbox", id] });
  };
  const who = me.mode !== "oidc" ? reviewer : null;
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
    <div className="dk-page">
      <PageHead
        title="Inbox"
        sub="Files and links people sent. Accepted material is compiled on the next build; the originals stay outside the ledger."
      />
      <Send
        workspace={id}
        reviewer={who}
        named={named}
        onSent={done}
        nameField={
          me.mode !== "oidc" && (
            <ReviewerField
              who="Your name"
              ariaLabel="reviewer"
              value={reviewer}
              onChange={setReviewer}
            />
          )
        }
      />
      <section className="card dk-list" aria-label="inbox">
        <div className="dk-toolbar">
          <select aria-label="status" value={status} onChange={(e) => setStatus(e.target.value)}>
            <option value="">pending and accepted</option>
            <option value="pending">pending</option>
            <option value="accepted">accepted</option>
          </select>
          <span className="dk-count">{items.length} item(s)</span>
        </div>
        <QueryView
          q={list}
          what="the inbox"
          rows={4}
          isEmpty={(d) => d.length === 0}
          empty={
            <Empty title="The inbox is empty." icon="inbox">
              Send a file or a link above. A reviewer accepts it, and the next build compiles it.
            </Empty>
          }
        >
          {() => (
            <ul className="dk-rows">
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
            </ul>
          )}
        </QueryView>
        {accept.isError && <ErrorNote error={accept.error} />}
        {purge.isError && <ErrorNote error={purge.error} />}
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
    <li className="dk-row dk-row-static">
      <div className="dk-row-head">
        <span className={i.status === "pending" ? "chip warn" : "chip ok"}>{i.status}</span>
        <strong className="dk-row-name">{i.name}</strong>
        <span className="dk-row-actions">
          {i.status === "pending" && canAccept && (
            <button type="button" className="primary" disabled={busy} onClick={onAccept}>
              Accept
            </button>
          )}
          {canPurge && !confirming && (
            <button type="button" className="dk-danger" onClick={() => setConfirming(true)}>
              Purge…
            </button>
          )}
        </span>
      </div>
      <div className="dk-row-sum">
        {sizeText(i.size)} · from {sentBy(i)} · {i.received_at}
        {i.accepted_by ? ` · accepted by ${i.accepted_by}` : ""}
      </div>
      {canPurge && confirming && (
        <div className="dk-actions dk-confirm">
          <input
            aria-label={`type ${i.id} to purge`}
            placeholder={i.id}
            value={typed}
            onChange={(e) => setTyped(e.target.value)}
          />
          <button
            type="button"
            className="dk-danger"
            disabled={busy || typed.trim() !== i.id}
            onClick={onPurge}
          >
            Purge the original
          </button>
          <button type="button" className="linkish" onClick={() => setConfirming(false)}>
            cancel
          </button>
        </div>
      )}
    </li>
  );
}

function Send({
  workspace,
  reviewer,
  named,
  onSent,
  nameField,
}: {
  workspace: string;
  reviewer: string | null;
  named: boolean;
  onSent: () => void;
  /** The name field where nobody is signed in; it also names who accepts and purges. */
  nameField?: ReactNode;
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
      return apiPost<{ name: string; status: string }>(`/workspaces/${workspace}/inbox`, body);
    },
    onSuccess: () => {
      setFile(null);
      setUrl("");
      onSent();
    },
  });
  return (
    <section className="card" aria-label="send material">
      <h2 className="card-label">Send material</h2>
      {nameField}
      <div className="dk-actions dk-send">
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
          type="button"
          className="primary"
          disabled={!named || send.isPending || (!file && !url.trim())}
          onClick={() => send.mutate()}
        >
          Send
        </button>
      </div>
      {send.isError && <ErrorNote error={send.error} />}
      {send.isSuccess && (
        <p className="ok-line">
          Stored {send.data.name} ({send.data.status}).
        </p>
      )}
    </section>
  );
}
