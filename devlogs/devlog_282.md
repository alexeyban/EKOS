# Devlog 282 — `docx-rs` dropped for the zip advisories; ekos.dev SEO pass

**Date:** 2026-10-19
**PRs:** none (local `main`, pushed)
**Branch:** EKOS main — this commit; ekos-webpage 0a45007 (deployed)

---

## Summary

The Audit workflow failed on RUSTSEC-2026-0367/0368/0369 (`zip` < 9: `extract()` follows symlinks
out of the destination and keeps SUID/SGID bits). Two `zip` versions were in the tree, both through
the `localdocs` plugin: its own `zip = "2"` and `docx-rs` 0.4.22's `zip` 8.6.0. `docx-rs` is at its
latest release and pins `zip ^8.6`, so it could not move. The DOCX reader now parses the main
document part with `roxmltree` over `zip` 9, and `docx-rs` is gone from all lockfiles. Separately,
ekos.dev got an SEO pass (titles, descriptions, sitemap dates, server compression, `www` redirect).

---

## localdocs: DOCX reader without `docx-rs`

### Problem / motivation
`scripts/audit.sh` (and `audit.yml`) fail on any vulnerability. EKOS never calls `extract()`, so
the advisories were not reachable, but ignoring them by ID would also hide a future reachable use
anywhere in the tree. The extraction EKOS needs — body paragraph text and table cells — is a small
part of what `docx-rs` does (it is mostly a DOCX *writer*).

### What was built
| Piece | Change |
|---|---|
| `plugins/localdocs/src/docx.rs` | Main part found through `_rels/.rels` (`officeDocument` relationship, default `word/document.xml`), parsed with `roxmltree`; Transitional and Strict WordprocessingML namespaces; decompressed XML part capped at 256 MiB (zip bomb) |
| `plugins/localdocs/Cargo.toml` | `docx-rs` removed; `zip` 2 → 9; `roxmltree` (already a workspace dependency) added |
| Tests | Fixture `.docx` built with the `zip` writer instead of `docx-rs`; new: main part via relationship, hyperlink/insertion/`w:delText` text not extracted, non-package bytes rejected |
| Lockfiles | `ekos/` and `tests/integration/` now hold only `zip` 9.0.5 (`benchmark/` has no `zip`) |

### Implementation details worth remembering
- The reader keeps **exactly** what the old `docx-rs` traversal kept: `w:t` inside a paragraph's
  *direct* `w:r` children, and a table cell's direct paragraphs. Text inside `w:hyperlink`, `w:ins`
  and `w:sdt` was never extracted; including it now would change artifact contents for every DOCX
  already indexed, so that is a separate decision (a test pins the current behaviour).
- Verified against 40 real `.docx` files on this machine: text, tables, sections and image hashes
  byte-identical to the `docx-rs` output; the run was about 3× faster (1.93 s → 0.57 s for all 40).
- `zip` 9 changed `ZipFile::name()` to return `Result<Cow<str>>`; an undecodable entry name is
  skipped, like any other unreadable entry in `extract_media_images`.
- `roxmltree` rejects DTDs by default, so no XXE surface.

### Decisions
- **Replace, not ignore.** An `audit.toml` ignore is per advisory ID, tree-wide. Replacing removed a
  dependency and the vulnerable versions outright.
- No pipeline logic-version bump: the output is identical, so cached artifacts stay valid.

---

## ekos.dev SEO pass (ekos-webpage 0a45007; nginx on the web host)

Google barely ranks ekos.dev: "ekos" belongs to a dozen other organisations and the GitHub repo
outranks the site. Changes:
- `tools/build_docs.py`: optional `@seo-title` / `@seo-desc` lines after a page header (`<title>` and
  meta only; H1 and lead unchanged). 85 docs pages got keyword-bearing titles (≤60 chars) and
  110–160 char descriptions; the 13 Learn pages are retitled as interview guides (were 69–107 chars).
- BreadcrumbList JSON-LD on docs pages; sitemap `lastmod` is now the date a page's HTML last changed
  (`tools/lastmod.json`, seeded from git) instead of the build date on every URL.
- Homepage art: a 2.3 MB PNG fetched eagerly at high priority → 217 KB WebP, lazy.
- nginx: `www.ekos.dev` (a 200 duplicate of the site) → 301 to `ekos.dev`; HSTS; gzip for
  CSS/JS/JSON/SVG/XML (nginx gzips only `text/html` by default — `search.json` 149 → 54 KB);
  30-day cache for images.
- GitHub: EKOS homepage → `https://ekos.dev` (was the old Pages site) and ekos-lakehouse →
  `/lakehouse/`, plus topics on both.

---

## Knowledge Captured

- **`docx-rs` pins `zip ^8.6`** (0.4.22, latest as of 2026-10-19). Any crate depending on it carries
  RUSTSEC-2026-0367/0368/0369. ekos-lakehouse follows EKOS `main` by git: a `cargo update -p ekos`
  there drops `docx-rs`, but it also has its own `zip` 2.6.1 from another dependency.
- **To check a parser swap, diff it against the old one on real files**, not only on fixtures:
  dump the parser output (text, tables, sections, image hashes) before and after with a temporary
  `#[ignore]` test and `cmp` the results.
- **nginx's `gzip on` without `gzip_types` compresses only `text/html`.** Check
  `Content-Encoding` on a `.css` / `.json` response, not on the page.
- **A sitemap whose `lastmod` is always "today" gets ignored by Google.** Derive it from content.

---

## Files Changed
| File | Change summary |
|---|---|
| `ekos/plugins/localdocs/src/docx.rs` | DOCX reader on `roxmltree` + `zip` 9; tests rebuilt without `docx-rs` |
| `ekos/plugins/localdocs/Cargo.toml` | `docx-rs` removed, `zip` 9, `roxmltree` |
| `ekos/Cargo.lock`, `tests/integration/Cargo.lock` | `docx-rs` and `zip` 2/8 gone, `zip` 9.0.5 |
| `devlogs/devlog_282.md` | this file |
