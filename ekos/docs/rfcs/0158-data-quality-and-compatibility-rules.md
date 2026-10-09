# RFC 0158 — Data-quality rules, target compatibility, and the source-coverage completeness check

**Status:** Draft
**Date:** 2026-09-24
**Supersedes:** none
**Related:** RFC 0154 (foundation — this RFC carries its coverage requirement), RFC 0157 (profiles
and catalog facts), RFC 0159 (consumes lossiness findings), RFC 0161 (dispositions and approval),
RFC 0018 (`ekos_impact` — seeds inferred relationships from real code)

---

## Summary

Three things: the data-quality rule families, the target-compatibility rules that measure how many
rows a migration would actually damage, and the **completeness check** that enforces RFC 0154's
promise that no PostgreSQL feature is a non-goal.

A finding is never a warning in a log. It is a fact, with the query that produced it, the measured
value, and a required `MigrationDisposition` before its unit can advance.

## Motivation

Type-level compatibility checking ("`numeric` may not fit in Spark's `DECIMAL(38)`") is easy and
nearly useless: it flags every `numeric` column in the database and buries the three that actually
overflow. What a migration needs is the row count — *seventeen rows in one table exceed precision
38, here are their keys*. That turns a blocking unknown into a five-minute decision.

The second half of this RFC exists because "supported" has to be provable. RFC 0154 states that
every source object is recovered, classified and dispositioned, and that a migration reaching
`signed_off` with an unclassified object is a defect. That sentence is worth nothing without a
mechanical check, which is defined here.

## Design

### Rule shape

Every rule, DQ or compatibility, is a row in a declarative catalog:

```
id            e.g. "DQ.UNIQ.001", "COMPAT.CH.NUMERIC_PRECISION"
applies_to    predicate over catalog facts + profile facts
sql_template  per source dialect; must be a single SELECT (RFC 0160 classifier)
measure       what the query returns: affected row count, plus up to k example keys
threshold     what makes it a finding
severity      info / warn / blocking
target        null for DQ rules, a target engine for compatibility rules
lossiness     exact / widening / narrowing-safe / lossy   (compatibility rules only)
```

Rules are data, so adding one is a catalog entry plus a fixture — not a code change — and the
fixture requirement is enforced (see *Testing*).

### Data-quality rule families

| Family | Examples |
|---|---|
| **Completeness** | Null rate against declared or documented expectation; empty string used as NULL; sentinel dates (`1900-01-01`, `9999-12-31`) standing in for NULL |
| **Uniqueness** | Duplicates on PK candidates and business keys; UNIQUE constraints that are partial, deferrable or `NOT VALID` and therefore not actually guaranteed |
| **Referential integrity** | Orphans on declared FKs — possible with `NOT VALID` constraints or disabled triggers — **and on inferred FKs** |
| **Validity / domain** | Enum-like text columns with outliers; range violations; format violations against the profiled pattern class; malformed JSON in text columns |
| **Consistency** | Documentation contradicts data (see *Doc-vs-data conflicts*) |
| **Timeliness** | Tables with no writes in N days — candidates for "archive, do not migrate" |

**Inferred relationships are the EKOS-specific part.** A declared FK is easy. The interesting case is
the FK that was never declared, and EKOS can find it because it has already compiled the application
code: join predicates recovered from views, ETL and app code (`ekos_impact`, the Transformation IR)
propose candidate `(child.col → parent.col)` pairs, and an inclusion check measures how true they
actually are. A pair at 99.97% inclusion is a real relationship with 340 orphans — which is both a
DQ finding and a hard input to wave ordering. Candidates are proposals with confidence and evidence,
never silently promoted, following the RFC 0029/0063 precedent for unconfirmed matches.

### Doc-vs-data conflicts

There is no `ConflictingEvidence` path in EKOS today; identity's `ConflictKind::SameNameDifferentKind`
is a different problem. This RFC introduces the first one, narrowly scoped: a `MigrationDqFinding`
with `rule = "DQ.CONSIST.DOC"`, carrying both sides as evidence — the documentation claim (a
Confluence page, a README section, a column comment recovered by RFC 0146 Phase 2) and the measured
value.

**Delivered as RFC 0172 Phase 3 (2026-10-09):** `ekos migrate assess` writes the `DQ.CONSIST.DOC` finding, and a contradicted claim also becomes a `ConflictingEvidence` (`doc_vs_data`) item. See that RFC for the claim grammar and the live verification.

Kept deliberately small: it fires only where documentation makes a *checkable* claim (never null,
unique, one of a fixed set, within a range) about a column that was profiled. Free-text mismatch
detection is not attempted.

### Target compatibility

Each rule measures affected rows, not just types. The matrix below is the ClickHouse column; the
Delta/Spark column is RFC 0165's, sharing this rule shape.

| Issue | ClickHouse behaviour | Lossiness |
|---|---|---|
| `numeric` precision > 76 | no representation | lossy |
| Unconstrained `numeric` | needs P,S chosen from the profiled scale | narrowing-safe if profile proves the bound |
| `NaN` / `±Infinity` in numeric | `Float` yes, `Decimal` no | lossy if mapped to Decimal |
| `±infinity` timestamps and dates | no equivalent | lossy |
| Dates outside 1970–2149 | `Date` fails; `Date32` covers 1900–2299 | exact with `Date32` |
| Dates before 1900 | outside `Date32`/`DateTime64` | lossy |
| `timestamptz` | `DateTime64(6, 'UTC')` | exact |
| `timestamp` without zone | `DateTime64(6)` | exact |
| NULLs | non-nullable by default; `Nullable(T)` has real cost | exact, with a design note |
| `jsonb` | `JSON` type (version-dependent) or `String` | widening to String |
| Arrays, nested arrays | `Array(T)` | exact |
| Enums, domains | `Enum8`/`Enum16` or `LowCardinality(String)` | exact / widening |
| `uuid` | `UUID` | exact |
| `inet` / `cidr` | `IPv4`/`IPv6`, split by family | exact, needs a split |
| `interval` | no direct type; months/days/microseconds split | exact, needs a split |
| `bytea` | `String` | exact |
| `money` | `Decimal` — locale-dependent input | lossy without an explicit locale decision |
| Identity / `serial` | no auto-increment | **behavioural**, needs a design decision |
| PK / UNIQUE / FK enforcement | not enforced; `ReplacingMergeTree` dedup is eventual | **behavioural**, R3 |
| Collation-dependent ordering | binary | behavioural |
| `char(n)` trailing spaces | padding semantics differ | see RFC 0155 |
| Case-sensitive identifiers | case-sensitive | exact |

The last rows matter most and are the ones type-mapping tools skip: they are not type problems, they
are **semantic** problems. Losing FK enforcement does not corrupt a single row on load day; it
corrupts the database six months later. Each is a blocking finding requiring an R3 disposition.

### The completeness check

The check that makes RFC 0154's coverage requirement real:

```
denominator = every object RFC 0157's introspector recovered from the source catalog
numerator   = those with (a) a translation decision with evidence, or
                          (b) a MigrationDisposition by a named human
assert numerator == denominator  before any unit may reach signed_off
```

It runs as a query over the ledger, reported per object kind, so the gap is always specific: "9
triggers and 2 extensions are unclassified" rather than "94% complete". Unsupported-in-target
features are *classified*, not excluded — PostGIS geometry, custom C functions, RLS policies and
triggers each produce a blocking finding with the exact objects, and a human dispositions them
(re-implement downstream, drop deliberately, keep in PostgreSQL, out of scope for this wave).

Explicitly: **"no rule matched this object" is a failure of the check, not a pass.** That asymmetry
is the whole point — a silent gap must cost something.

## Testing

- **Synthetic fixture per compatibility rule**, containing exactly the offending values and nothing
  else, with the expected affected-row count committed as a literal. Recall must be 100% and the
  count exact — approximate is worse than absent here, because it gets budgeted against.
- **A catalog test asserts every rule has a fixture.** A rule without one fails CI, which is what
  keeps "rules are data" from becoming "rules are untested data".
- Inferred FK: a fixture where code joins on an undeclared pair with known orphans; confidence and
  orphan count both asserted.
- Doc-vs-data: a fixture where a column comment says `NOT NULL` and 3% of rows are null.
- Completeness: a fixture database containing one object of every kind; the check fails while any
  is unclassified and passes only when all are dispositioned. A deliberately added trigger makes it
  fail again.

## Alternatives considered

- **Type-level compatibility only.** Rejected — it is the behaviour every existing tool has, and it
  produces a report nobody acts on.
- **Sampling for affected-row counts.** Rejected for blocking rules: the decision is "are there any,
  and which", and a sample cannot answer it. Non-blocking informational rules may sample, labelled.
- **Treating unsupported features as out-of-scope silently.** Rejected; it is the precise failure
  RFC 0154's coverage requirement exists to prevent.
- **A general documentation-contradiction detector via LLM.** Rejected for v1: unbounded false
  positives, and every one costs human review time. Checkable claims only.

## Open questions

- [ ] Inclusion-check threshold for promoting an inferred FK from candidate to reportable — and is
      it one threshold or per-corpus?
- [ ] Do timeliness findings ("stale table") belong here or in planning (RFC 0159)?
- [ ] Should the completeness check run per wave as well as per project? Per wave allows partial
      sign-off; per project is the honest total.

## Acceptance criteria

- [ ] 100% recall on every compatibility fixture, with exact affected-row counts.
- [ ] Every rule in the catalog has a fixture, enforced by test.
- [ ] The completeness check fails on an unclassified object of every catalog kind, including
      triggers and extensions, and passes only when each is dispositioned.
- [ ] Findings are visible through the CLI and MCP with their evidence query attached.
