# RFC 0249 — Public demo: safety, bans and limits

**Status:** Accepted (2026-11-07, on the maintainer's instruction to implement all proposed RFCs)
**Date:** 2026-11-07
**Depends on:** RFC 0248, RFC 0180 (console access control), RFC 0182 (MCP gateway, audit), RFC 0187 (chat)
**Plan:** `ekos-demo-public/docs/PLAN.md`, "Safety"; public text: `ekos-demo-public/docs/RULES.md`

---

## 1. Motivation

A shared public demo with one database, visible requests and a model behind an owner-paid key invites abuse:
prompt injection, attempts to damage others' marts, runaway cost. The defences must be mostly deterministic, with
the model only helping to recognise intent, and the rules must be published before sign-in.

## 2. Design

### 2.1 Triage is mostly deterministic

Term resolution (RFC 0248) already decides possible / impossible. `unsafe` comes from (a) deterministic signals and
(b) the model's intent judgement. Users never write SQL, so there is no direct SQL injection; the remaining
surface is text: questions, comments, instructions, synonyms.

### 2.2 Unsafe (published at sign-in)

Bypass attempts (ignore-the-rules, hidden text, extracting the key or system prompt); data outside the allowed
schemas; mass deletion or breaking existing marts; resource abuse (huge joins, mass repeats); abusive or illegal
content.

### 2.3 Bans

- **Automatic ban** on deterministic signals: known injection patterns, access to forbidden schemas, N unsafe
  refusals in a row. Account ban plus a temporary IP block.
- **Model-only** judgement: refusal, a temporary block (24 h) and a review-log entry; a repeat leads to a ban.
- The user sees the reason. The owner sees the log and has an unban action. Every decision is an audit record
  (RFC 0182 style).

### 2.4 Limits

Per user per day: questions, model calls, additive changes; **one modifying change per day** after warning and
consent; a cap on dashboards; rate limit per IP; modifying changes only after N days since first sign-in; inactive
dashboards archived after N days. Counters are in PostgreSQL (EKOS already logs model calls, RFC 0220). Numbers
are set before launch, not in this RFC.

### 2.5 Data sent to the model

The question, candidate terms and schema names. **No row values.** Demo questions are cached: no model call. The
key lives in Key Vault and is used only by the console backend.

### 2.6 Sign-in

Platform authentication (Container Apps built-in) with Google, Microsoft and GitHub; no own sign-in code. The
principal reaches the console as an OIDC identity (RFC 0180); a token principal is never allowed `review`.

## 3. Pre-launch checks (blocking)

1. OpenCode Zen: does the `[llm]` config reach its endpoint, or does it need a provider? Do its terms allow serving
   third parties through the owner's key? **If not, the demo does not launch with that key.**
2. A red-team pass over the injection set before sign-in opens.
3. Data protection: what is stored per user (identity, requests, consents), retention, deletion on request. This
   depends on the erasure design for an append-only ledger (open TODO) and must be decided first.

## 4. Tests

A set of injection strings is refused and counts toward the automatic ban; a forbidden schema is refused; a
model-only `unsafe` blocks for 24 h and a repeat bans; limits are enforced per user and reset daily; the model
never receives a row value (asserted on the request body); a banned user cannot read or write.

## 5. Open questions

1. IP blocking behind a proxy: which address is authoritative? 2. Appeals: any process, or owner-only unban?
