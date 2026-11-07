# Devlog 325 — Cheap fixes, release signing, glibc install check, public demo scaffold on Azure

**Date:** 2026-11-07
**PRs:** none (direct commits to main)
**Branch:** main

---

## Summary
Reviewed TODO.md (237 open of ~990), fixed the cheap correctness bugs, added signed build provenance and a
glibc-floor install check to the release workflow, then scaffolded the public demo from
`public-demo-plan.md` into a private repo and applied phase 1 to Azure. Three RFCs (0247–0249) open the plan's M0.

---

## Code fixes (47bbc53, bcb9496)
- `sql_transform_analyzer` split statements with `split(';')`, cutting `$$` bodies, string literals and comments
  in half; now `split_sql_statements` (quote, dollar-quote and comment aware, split into helpers for S3776).
- `search.rs::query_scored_marked` added the memory-path boost twice; RFC 0101 describes one clause.
- `ollama.rs::likely_truncated` was true on every call when `max-tokens` equalled the window; now it asks whether
  the prompt alone fills 95% of it.
- The RFC 0139 "fabrication regression OPEN" entries were stale (closed by `43f352c`, devlog_172); ticked.

## Release (ca1fbfc, bda748f, 3dbe375)
- `actions/attest-build-provenance` on every archive and `SHA256SUMS` (first-party, so no third-party action in the
  job with `contents: write`). Verify: `gh attestation verify <file> --repo alexeyban/EKOS`.
- `verify-glibc` job: installs the published tag on `rockylinux:8` (must fall back to musl), `rockylinux:9`,
  `ubuntu:22.04` (must keep gnu). Neither job has run on a real tag yet.

## Public demo (ekos-demo-public, EKOS 82f6df7f/864fca9c)
- Terraform reuses `modules/azure` (container compute). The module's `allowed_cidrs` was VM-only, so a "closed by
  default" variable would have been a lie on Container Apps; the ingress now gets IP Allow rules (tested).
- Applied to "Azure subscription 1": 35 resources, console healthy. Phase 2 (Polaris/Trino/Superset) not deployed.

---

## Knowledge Captured
- **`git add <dir>` stages untracked files too.** `git add deploy` pushed an unrelated untracked `deploy/demo/` tree
  (binary-demo, an embedded git clone, a venv with `.so` files) in 82f6df7f; unstaged in b10eb39a but it stays in
  history (the maintainer chose not to rewrite). Add explicit paths.
- **A subscription can refuse a whole region**: `RequestDisallowedByAzure ... not accepting new customers`
  (`aka.ms/locationineligible`) for West Europe on this subscription. Probe regions with a free vnet in a temporary
  group (vnet names under 2 characters are invalid, which makes a probe look "blocked"). North/Sweden Central/
  Germany West Central/France Central worked.
- **Debian 11 mirrors 404** (EOL): do not use it for CI container installs. The v1.3.0 gnu binary needs at most
  GLIBC_2.34 although it is built on 22.04 (2.35), so Rocky 9 keeps the gnu build and Rocky 8 (2.28) falls back.
- A sonar.yml run is cancelled by the next push to main; wait for the last one, and push less often.
- Terraform local module paths cannot reach a sibling module (`../common`); test with a `git::file://...//path`
  source instead.

## Files Changed
| File | Change summary |
|---|---|
| `ekos/crates/recovery/src/sql_transform_analyzer.rs` | quote-aware statement splitter |
| `ekos/crates/ledger/src/search.rs`, `recovery/src/ollama.rs` | duplicate boost removed; truncation predicate |
| `.github/workflows/release.yml` | provenance attestation, `verify-glibc` job, HTTPS-only curl |
| `deploy/terraform/modules/azure/*` | ingress `allowed_cidrs`, new outputs, tests |
| `ekos/docs/rfcs/0247-0249` | annotations; questions → dashboards; public demo safety |
| `README.md`, `TODO.md`, `CLAUDE.md` | provenance docs, roadmap, highest RFC |
