# Devlog 293 — Docs repository and wiki snapshot (RFC 0221); tokenomics 2026-10-24

**Date:** 2026-10-24
**PRs:** none (direct to `main`: 45c1329 RFC 0221 Phases 1–2, 944b66c + 2c86b90 tokenomics)
**Branch:** `main`

---

## Summary

Part I.8's remaining half. iDempiere's documentation site (Docusaurus, 510 Markdown pages) now joins the code
project as a second `[[sources]]` entry: its YAML front matter becomes document properties instead of 504 junk
sections, each section records the tickets it names, and `ekos commit` links 1,743 docs sections to the tickets
that commits, migrations and code already share — a schema column reaches the page that explains it.
`scripts/wiki-snapshot.py` turns MediaWiki pages into Markdown with provenance; wiki.idempiere.org itself
refuses scripts (Cloudflare challenge), so the snapshot waits for a browser export. Separately, the tokenomics
page and `TOKENOMICS.md` record the founder's 501,000 EKOS burn and the staking-pool top-up.

---

## RFC 0221 — docs repository and wiki snapshot

### Problem / motivation
`localdocs` read the docs repository as 514 documents and 3,016 sections, 504 of them nothing but front matter
(`sidebar_position: 36`, `tags: [technical]`). The front matter is the page's own metadata and the only place a
wiki snapshot's provenance can live. The docs name 1,284 distinct `IDEMPIERE-NNNN` tickets that nothing linked.

### What was built

| Component | What it does |
|---|---|
| `plugins/localdocs/src/front_matter.rs` | `---` … `---`/`...` within 200 lines → top-level scalars and scalar lists (40 keys, 500 chars); unparseable YAML keeps no fields but is still removed |
| `TextParser` (md) | blanks the block's lines (line numbers kept) before sectioning; `front_matter` on the artifact only when present |
| `LocalDocAnalyzerPass` v4 | `front_matter` + promoted `title`/`description`/`source_url`/`source_revision`/`revision_timestamp`/`licence` on the `Document`; provenance subset on every section; `with_ticket_keys` → section `tickets` |
| `semantic::tickets` | `Section` is a citing kind |
| `PIPELINE_LOGIC_VERSION` | 4 → 5 |
| `scripts/wiki-snapshot.py` + `scripts/tests/` | export XML or API → one Markdown file per page with provenance front matter, `SNAPSHOT.json`; pandoc for the conversion; stops on 403/challenge |

### Measured

| | |
|---|---|
| docs repo alone: sections | 3,016 → 2,520; heading-less 504 → 8 (MDX `import` lines) |
| documents with `description` | 499 of 514 |
| sections citing a ticket | 860 |
| code + docs + live DB: commits / tickets | 16,589 / 4,606 |
| `TicketFor` | 1,743 Section, 3,074 SchemaChangeSet, 154 JavaType, 272 JavaMethod |
| first full run | build 407 s, recover 246 s, resolve 7 s, compile 20 s, commit 2,220 s (144,073 objects); model calls 0 |

Chain: `C_Country.ISOCountryCodeAlpha3` → `202412261358_IDEMPIERE-6349` → `IDEMPIERE-6349` → "New Fields in Country"
(v12 new features) and the v12 changelog's 2024-12-27 entry.

### Implementation details worth remembering
- Blanking, not removing, the front-matter lines keeps every section's `line_start` real. Section indices of a
  file with front matter shift by one, so those section ids change once.
- pandoc's GFM writer escapes intraword underscores (`C\_Order`), which breaks every table-name match; the
  snapshot unescapes them (GFM does not emphasise inside a word) and turns `"wikilink"` links into absolute URLs.
- Loading a hyphenated script with `importlib` for tests: register it in `sys.modules` before `exec_module`, or a
  `@dataclass` fails (`sys.modules.get(cls.__module__)` is `None`).

### Decisions
- **No way around bot protection.** wiki.idempiere.org answers every request, API included, with a Cloudflare
  JavaScript challenge. The script stops on it and points at `Special:Export`; nothing retries.
- **No retrieval time in snapshot files**, so the same revision gives byte-identical output.
- **Fresh project for `[[sources]]`**: source names key object ids, so converting `[observe] paths` re-keys
  everything.
- **Licence recorded, not enforced yet.** The docs repository has no licence file; republishing (Part II.2) must
  check `licence` first.

---

## Tokenomics 2026-10-24

From the transactions the founder supplied, read on-chain (public Solana RPC): the deposit moved 500,000 EKOS from
Community Rewards (2M → 1.5M) to the staking pool's reward vault (3M → 3.5M) on 2026-10-23; the burn wallet
`Bd69…dKa` received 3,000 (test) and 500,000 (personal wallet), burned 1,000 + 500,000 = 501,000 on 2026-10-24, and
holds 2,000. The mint's supply is 966,875,284 — ~33,716 lower than the document's figures predicted, burned outside
the project's wallets. Personal wallet: +935,279 bought on the open market, −500,000 → ~2,985,279. ekos.dev/token
deployed; Buyback & Burn marked implemented there at the founder's request.

---

## Knowledge Captured

- **A blocked site looks like HTTP 403 with an HTML "Just a moment…" body** — API endpoints included. A scraper
  must treat a non-JSON answer from `api.php` as a stop, not as data.
- **Never put the user's email in a User-Agent** for a third-party site (done once by mistake on the first probe
  here; the script's UA carries only its name and version).
- **Document figures drift from the chain.** Always read total supply from the mint (`getTokenSupply`) before
  restating it; arithmetic on the previous snapshot was 33,716 EKOS off.
- An incoming SPL transfer is not listed by `getSignaturesForAddress` on the *owner*; query the token account.
- `[skip ci]` also skips the push-email workflow; pushes no longer carry it.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/docs/rfcs/0221-docs-repository-and-wiki-snapshot.md` | new RFC with results |
| `ekos/plugins/localdocs/src/{front_matter,text,lib,docx,email,html,pdf}.rs`, `Cargo.toml` | front matter |
| `ekos/crates/recovery/src/local_docs_analyzer.rs` | v4: promoted properties, section tickets |
| `ekos/crates/semantic/src/tickets.rs` | `Section` citing kind |
| `ekos/crates/common/src/lib.rs` | `PIPELINE_LOGIC_VERSION` 5 |
| `ekos/crates/cli/src/commands/recover.rs` | ticket keys to the docs pass |
| `scripts/wiki-snapshot.py`, `scripts/tests/` | new snapshot script + tests |
| `TOKENOMICS.md` | 2026-10-24 snapshot |
| `README.md`, `TODO.md`, `CLAUDE.md`, `docs/generated/ekos-self-documentation.html` | RFC 0221 |
