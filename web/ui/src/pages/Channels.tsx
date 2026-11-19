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
import { ErrorNote, PageHead, QueryView } from "../ui";

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
    <div className="fm-page">
      <PageHead
        title="Channels"
        sub="Talk to EKOS from Telegram, Slack or Teams: link your chat account to yourself here."
      />
      <section className="card" aria-labelledby="channels-link">
        <h2 className="card-label" id="channels-link">
          Link a chat account
        </h2>
        <p className="fm-note">
          Write <code>/ekos link</code> to the EKOS bot in Telegram, Slack or
          Teams, then enter the code it sends you. Afterwards the bot answers
          you with your own access, and what you tell it is recorded under your
          name.
        </p>
        <form
          className="fm-inline"
          onSubmit={(e) => {
            e.preventDefault();
            link.mutate();
          }}
        >
          <input
            aria-label="code"
            placeholder="code"
            className="fm-mono"
            value={code}
            onChange={(e) => setCode(e.target.value.toUpperCase())}
          />
          <button
            type="submit"
            disabled={link.isPending || code.trim().length < 4}
          >
            Link
          </button>
        </form>
        {link.isError && <ErrorNote error={link.error} what="Linking" />}
        {link.isSuccess && (
          <p className="fm-ok">
            Linked your {link.data.channel} account as {link.data.label}.
          </p>
        )}
        <div className="fm-sub">
          <h3 className="fm-sub-title">Your linked accounts</h3>
          <QueryView
            q={mine}
            what="your linked accounts"
            rows={2}
            isEmpty={(d) => d.length === 0}
            empty={<p className="fm-hint">None yet. Link one with a code above.</p>}
          >
            {(accounts) => (
              <ul className="fm-list">
                {accounts.map((a) => (
                  <li key={a.id}>
                    <span className="fm-item">
                      {a.channel} · {a.account}
                      {a.default_workspace
                        ? ` · direct messages use ${a.default_workspace}`
                        : ""}
                    </span>
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
            )}
          </QueryView>
        </div>
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
    <section className="card" aria-labelledby="channels-owner">
      <h2 className="card-label" id="channels-owner">
        Channels (owner)
      </h2>
      {overview.isError && (
        <ErrorNote error={overview.error} what="Reading the channels" />
      )}
      <p className="fm-note">
        Running: {running.length ? running.join(", ") : "none"} — a channel runs
        when its secrets are set on the Settings page (Channels).
        {o?.public && " This is a public deployment: no questions are sent."}
      </p>

      <div className="fm-sub">
        <h3 className="fm-sub-title">Bindings</h3>
        {o && !o.bindings?.length && (
          <p className="fm-hint">No channel is bound to a project yet.</p>
        )}
        <ul className="fm-list">
          {(o?.bindings ?? []).map((b) => (
            <li key={b.id}>
              <span className="fm-item">
                {b.channel} {b.conversation} → {b.workspace_id}
              </span>
              <button
                className="linkish danger"
                aria-label={`unbind ${b.id}`}
                onClick={() => unbind.mutate(b.id)}
              >
                remove
              </button>
            </li>
          ))}
        </ul>
        <form
          onSubmit={(e) => {
            e.preventDefault();
            bind.mutate();
          }}
        >
          <div className="fm-grid">
            <label className="fm-field">
              <span className="fm-label">Where</span>
              <select
                aria-label="channel"
                value={channel}
                onChange={(e) => setChannel(e.target.value)}
              >
                <option value="slack">Slack channel</option>
                <option value="teams">Teams channel</option>
                <option value="telegram">Telegram group</option>
              </select>
            </label>
            <label className="fm-field">
              <span className="fm-label">Channel or group id</span>
              <input
                aria-label="conversation"
                placeholder="channel or group id"
                value={conversation}
                onChange={(e) => setConversation(e.target.value)}
              />
            </label>
            <label className="fm-field">
              <span className="fm-label">Project</span>
              <input
                aria-label="project"
                placeholder="project id"
                value={workspace}
                onChange={(e) => setWorkspace(e.target.value)}
              />
            </label>
          </div>
          <div className="fm-actions">
            <button
              type="submit"
              disabled={bind.isPending || !conversation.trim() || !workspace.trim()}
            >
              Bind
            </button>
          </div>
          {bind.isError && <ErrorNote error={bind.error} what="Binding" />}
        </form>
      </div>

      <div className="fm-sub">
        <h3 className="fm-sub-title">Linked accounts</h3>
        {o && !o.accounts?.length && (
          <p className="fm-hint">Nobody has linked a chat account yet.</p>
        )}
        <ul className="fm-list">
          {(o?.accounts ?? []).map((a) => (
            <li key={a.id}>
              {a.channel} · {a.account} → {a.label}
            </li>
          ))}
        </ul>
      </div>

      <div className="fm-sub">
        <h3 className="fm-sub-title">Questions digest</h3>
        <div className="fm-actions">
          <button disabled={digest.isPending} onClick={() => digest.mutate()}>
            Send the questions digest now
          </button>
          {digest.isSuccess && (
            <span className="fm-ok">
              {digest.data.skipped ?? `${digest.data.sent} question(s) sent`}
            </span>
          )}
        </div>
        {digest.isError && (
          <ErrorNote error={digest.error} what="Sending the digest" />
        )}
      </div>
    </section>
  );
}
