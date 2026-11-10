// RFC 0253 §3.2 — channels: link your Telegram, Slack or Teams account to yourself, and (for an
// owner) bind a channel or group to a project and send the questions digest now.
//
// A bot gives an unlinked account a one-time code (`/ekos link`); entering it here while signed
// in links that account to you. Until then the bot does nothing for it.

import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { useOutletContext } from "react-router-dom";
import { api, apiDelete, apiPost } from "../api/client";
import type { ChannelAccount, ChannelsOverview } from "../api/types";

interface Identity {
  owner?: boolean;
  mode?: string;
}

export function Channels() {
  const me = useOutletContext<Identity>();
  const qc = useQueryClient();
  const mine = useQuery({
    queryKey: ["channels-me"],
    queryFn: () => api<ChannelAccount[]>("/channels/me"),
  });
  const [code, setCode] = useState("");
  const link = useMutation({
    mutationFn: () =>
      apiPost<{ channel: string; label: string }>("/channels/link", { code }),
    onSuccess: () => {
      setCode("");
      void qc.invalidateQueries({ queryKey: ["channels-me"] });
    },
  });
  const unlink = useMutation({
    mutationFn: (accountId: string) =>
      apiDelete(`/channels/me/${encodeURIComponent(accountId)}`),
    onSuccess: () => void qc.invalidateQueries({ queryKey: ["channels-me"] }),
  });

  return (
    <div className="sem-page">
      <section className="card">
        <strong>Link a chat account</strong>
        <p className="muted">
          Write <code>/ekos link</code> to the EKOS bot in Telegram, Slack or
          Teams, then enter the code it sends you. Afterwards the bot answers
          you with your own access, and what you tell it is recorded under your
          name.
        </p>
        <div className="btnrow">
          <input
            aria-label="code"
            placeholder="code"
            value={code}
            onChange={(e) => setCode(e.target.value.toUpperCase())}
          />
          <button
            disabled={link.isPending || code.trim().length < 4}
            onClick={() => link.mutate()}
          >
            Link
          </button>
        </div>
        {link.isError && <p className="err">{String(link.error)}</p>}
        {link.isSuccess && (
          <p className="ok-line">
            Linked your {link.data.channel} account as {link.data.label}.
          </p>
        )}
        <strong className="sem-sub">Your linked accounts</strong>
        {mine.data?.length === 0 && <p className="muted">None yet.</p>}
        <ul className="rel-list">
          {(mine.data ?? []).map((a) => (
            <li key={a.id}>
              {a.channel} · {a.account}
              {a.default_workspace
                ? ` · direct messages use ${a.default_workspace}`
                : ""}{" "}
              <button
                className="linkish"
                aria-label={`unlink ${a.id}`}
                onClick={() => unlink.mutate(a.id)}
              >
                unlink
              </button>
            </li>
          ))}
        </ul>
      </section>
      {me?.owner === true && <OwnerPanel />}
    </div>
  );
}

function OwnerPanel() {
  const qc = useQueryClient();
  const overview = useQuery({
    queryKey: ["channels"],
    queryFn: () => api<ChannelsOverview>("/channels"),
  });
  const [channel, setChannel] = useState("slack");
  const [conversation, setConversation] = useState("");
  const [workspace, setWorkspace] = useState("");
  const refresh = () => void qc.invalidateQueries({ queryKey: ["channels"] });
  const bind = useMutation({
    mutationFn: () =>
      apiPost("/channels/bindings", {
        channel,
        conversation,
        workspace_id: workspace,
      }),
    onSuccess: () => {
      setConversation("");
      refresh();
    },
  });
  const unbind = useMutation({
    mutationFn: (id: string) =>
      apiDelete(`/channels/bindings/${encodeURIComponent(id)}`),
    onSuccess: refresh,
  });
  const digest = useMutation({
    mutationFn: () =>
      apiPost<{ sent: number; skipped?: string }>("/channels/digest"),
  });
  const o = overview.data;
  const running = Object.entries(o?.configured ?? {})
    .filter(([, on]) => on)
    .map(([name]) => name);

  return (
    <section className="card">
      <strong>Channels (owner)</strong>
      {overview.isError && <p className="err">{String(overview.error)}</p>}
      <p className="muted">
        Running: {running.length ? running.join(", ") : "none"} — a channel runs
        when its secrets are set on the Settings page (Channels).
        {o?.public && " This is a public deployment: no questions are sent."}
      </p>
      <strong className="sem-sub">Bindings</strong>
      <ul className="rel-list">
        {(o?.bindings ?? []).map((b) => (
          <li key={b.id}>
            {b.channel} {b.conversation} → {b.workspace_id}{" "}
            <button
              className="linkish"
              aria-label={`unbind ${b.id}`}
              onClick={() => unbind.mutate(b.id)}
            >
              remove
            </button>
          </li>
        ))}
      </ul>
      <div className="btnrow">
        <select
          aria-label="channel"
          value={channel}
          onChange={(e) => setChannel(e.target.value)}
        >
          <option value="slack">Slack channel</option>
          <option value="teams">Teams channel</option>
          <option value="telegram">Telegram group</option>
        </select>
        <input
          aria-label="conversation"
          placeholder="channel or group id"
          value={conversation}
          onChange={(e) => setConversation(e.target.value)}
        />
        <input
          aria-label="project"
          placeholder="project id"
          value={workspace}
          onChange={(e) => setWorkspace(e.target.value)}
        />
        <button
          disabled={bind.isPending || !conversation.trim() || !workspace.trim()}
          onClick={() => bind.mutate()}
        >
          Bind
        </button>
      </div>
      {bind.isError && <p className="err">{String(bind.error)}</p>}
      <strong className="sem-sub">Linked accounts</strong>
      <ul className="rel-list">
        {(o?.accounts ?? []).map((a) => (
          <li key={a.id}>
            {a.channel} · {a.account} → {a.label}
          </li>
        ))}
      </ul>
      <div className="btnrow">
        <button disabled={digest.isPending} onClick={() => digest.mutate()}>
          Send the questions digest now
        </button>
        {digest.isSuccess && (
          <span className="ok-line">
            {digest.data.skipped ?? `${digest.data.sent} question(s) sent`}
          </span>
        )}
        {digest.isError && <span className="err">{String(digest.error)}</span>}
      </div>
    </section>
  );
}
