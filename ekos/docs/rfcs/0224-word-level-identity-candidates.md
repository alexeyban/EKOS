# RFC 0224 — Identity candidates must agree word for word

**Status:** Accepted
**Date:** 2026-10-25
**Amends:** RFC 0007 (same-source identity), RFC 0060/0063 (fuzzy candidates go to review), RFC 0213 (identity
review subject)

## 1. Problem

On the iDempiere testbed the identity review held 25 `SameAs` candidates, and 24 were wrong in the same way: a name
and its child or sibling.

| Pair | Why it scored high |
|---|---|
| `ASP_PROCESS` / `ASP_PROCESS_PARA`, `HR_ListType` / `HR_List`, `AD_Process_DrillRule` / `…_Para` | one name is the other plus a word |
| `HR_ListType` / `HR_ListLine`, `C_TaxType` / `C_TaxBase`, `WS_…FieldInput` / `…FieldOutput` | same stem, one word swapped |
| `react` / `react-dom`, `@docusaurus/core` / `@docusaurus/utils` (5 such) | a shared package scope |

Jaro-Winkler rewards a long common prefix, and these names share most of their letters. RFC 0060 found that no
threshold separates such pairs from real fuzzy matches; RFC 0063 sent every fuzzy match to review instead of merging
it. That keeps them out of the ledger's merges, but a person now has 24 obvious rejections to make before the
identity subject can be signed off (RFC 0213) — on every project of this shape.

Separately, a candidate an earlier compile proposed stays a relationship in the append-only ledger, so the review kept
showing it even after the scoring changed.

## 2. Design

1. **Words.** A name is split into words at every non-alphanumeric character and at camelCase boundaries, lower-cased
   (`HR_ListType` → `hr list type`, `@docusaurus/core` → `docusaurus core`); the affixes `normalize` already drops
   (`table`, `tbl`, `dim`, `fact`) are dropped here too.
2. **Compatible names** have the same words, or the same letters split differently (`anozimada` / `Anozi Mada`), or
   words that pair up as spelling variants (Jaro-Winkler ≥ 0.9: `customer`/`customers`, `cust`/`customer`). A name
   with a word the other lacks, or a word with no close variant, is not a candidate: its name score is 0.
3. **Exceptions.** `Person` — a short and a full name do match (`Robert` / `RobertJoonas`, RFC 0060's known-good
   merges). Objects with identical table rows (structural score ≥ 0.99 from `rows`) — the same PDF table extracted
   twice. Identical **columns** are not an exception: tables of one application share their audit columns, and a
   migration script may state only those (see §3).
4. **Current candidates.** `ekos commit` writes `.ekos/identity/current.json`: the ids of the `SameAs` candidates this
   compile proposed. The review shows an undecided compile candidate only while it is current; a decided one stays (a
   person's decision is kept), and a cross-system candidate from `ekos identity scan` (it carries `name_pattern`) is not
   affected. Without the file (a project not yet re-committed), every candidate shows, as before.
5. **Cache.** `SemanticCompilerPass` runs the resolver and caches its output by input only, so its version string ends
   in `ekos_identity::SCORING_VERSION` (a test checks it).

RFC 0060's documented residual pairs (`Build Private Images GHCR` / `…Public…`, `Tracker CI` / `Tracker script
update`, `… ua inspector readme` / `… ref …`) differ by a whole word and are no longer proposed at all.

## 3. Results

iDempiere testbed (`idemdocs`), compile + commit after each step:

| | identity items |
|---|---|
| before | 25 |
| words compared, identical columns exempt | 12 — eleven pairs of DDL tables from the PostgreSQL migration scripts (`HR_ListType` / `HR_ListLine`, `PP_Order_NodeNext` / `PP_Order_Cost`, …), let through by the identical-content exemption: their column lists scored as identical |
| identical columns no longer exempt | **1** — `anozimada` / `Anozi Mada`, one person's two spellings |

The 13 candidates no longer proposed left the review through `identity/current.json`; they stay in the ledger as
undecided relationships. The pass-version guard (§2.5) is a precaution: in these runs the compile did re-run the
resolver. Scope, PII and write-behaviour sign-offs stayed valid throughout.
