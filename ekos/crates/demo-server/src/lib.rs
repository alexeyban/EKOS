//! RFC 0045 — hosted demo server. A small, read-only surface over a fixed,
//! two-repo catalog: pre-rendered static curated docs, plus one evidence-cited
//! `/ask` endpoint. See `ekos/docs/rfcs/0045-hosted-demo-server.md` for the
//! full design and its explicit non-goals (no self-serve ingestion, no auth,
//! no write path, no ledger-only/no-LLM answer mode).

pub mod ask;
pub mod catalog;
pub mod page;
