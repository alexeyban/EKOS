# Devlog 10 — Phase 14: Enterprise Scale Connectors (scaffold pass)

**Date:** 2026-07-11
**PRs:** —
**Branch:** main

---

## Summary

Started Phase 14 — Enterprise Scale Connectors. Every connector's validation criterion in TODO.md
requires a live vendor sandbox (SAP sandbox, Salesforce developer org, Oracle XE container,
Fabric/Snowflake trial accounts) that isn't available in this environment, so per RFC 0012 this
session scaffolds five connector plugins (`sap`, `salesforce`, `oracle`, `fabric`, `snowflake`)
against a mockable client-trait boundary — real, reviewable `Observer` mapping logic backed by unit
tests, with the live network/driver wiring either written-but-untested (SAP, Salesforce, Fabric,
Snowflake — all REST) or explicitly stubbed (Oracle — no native ODPI-C dependency added). The
Kubernetes connector and "additional connectors on demand" backlog item were left untouched; TODO.md
marks all four scaffolded items `[~]` (in progress), not `[x]`, since the actual integration-test
validation criteria are unmet.

---

## RFC 0012 — Phase 14 connector scaffolding

### Problem / motivation

Phase 14 can't be fully implemented-and-verified here the way Phases 0–13 were — there's no SAP
sandbox, Salesforce dev org, Oracle XE container, or Fabric/Snowflake trial account reachable from
this environment. Shipping nothing until credentials exist would waste the part of this phase that
doesn't depend on them: the trait boundaries, request/response shapes, and `Observer` mapping logic
are real design and implementation work regardless of whether a live sandbox is available.

### What was built

| Component | Change |
|---|---|
| `docs/rfcs/0012-enterprise-connectors-scaffold.md` | RFC 0012 (accepted) — shared shape, per-connector decisions, explicit non-goals |
| `plugins/salesforce/` | `SalesforceClient` trait, `SalesforceApiClient` (real, untested), `MockSalesforceClient`, `SalesforceObserver`, 4 tests |
| `plugins/sap/` | `SapClient` trait, `SapODataClient` (real, untested), `MockSapClient`, `SapObserver`, 4 tests |
| `plugins/oracle/` | `OracleClient` trait, `OracleDbClient` (documented stub — `NotImplemented`), `MockOracleClient`, `OracleObserver`, 4 tests |
| `plugins/fabric/` | `FabricClient` trait, `FabricApiClient` (real, untested), `MockFabricClient`, `FabricObserver`, 3 tests |
| `plugins/snowflake/` | `SnowflakeClient` trait, `SnowflakeApiClient` (real, untested), `MockSnowflakeClient`, `SnowflakeObserver`, 3 tests |
| `Cargo.toml` | 5 new workspace members |
| `TODO.md` | Marked all 4 connector items `[~]` with a `Status (RFC 0012)` note each |

### Implementation details worth remembering

**Every connector follows the same shape**, modeled directly on `LlmProvider`/`MockLlmProvider`
(RFC 0008, `crates/recovery/src/llm.rs`): a `#[async_trait]` client trait, one real implementation,
one mock, and an `Observer` that holds `Arc<dyn Client>` injected at construction — never built from
`ScanContext`. This mirrors how `recover.rs` assembles an `AnthropicProvider` or falls back to a
mock itself, rather than `SqlAnalyzerPass` doing that internally; credential assembly is a CLI-layer
concern, not the observer's.

**SAP is OData-only, not RFC/`nwrfc`.** The original TODO phrasing offered either. `nwrfc` needs the
proprietary SAP NetWeaver RFC SDK — native libraries requiring `bindgen` against vendor headers that
aren't installable here, and adding that dependency would risk breaking `cargo build --workspace`
for anyone without the SDK. OData is plain REST, so `SapClient` gets the same `reqwest`-based
real-client treatment as Salesforce/Fabric/Snowflake.

**Oracle's real client is an honest stub, not a silent no-op.** The `oracle` crate wraps ODPI-C,
same native-dependency problem as SAP's `nwrfc`, but Oracle has no REST alternative for schema
introspection. `OracleDbClient::list_tables/list_constraints/list_views` all return
`OracleClientError::NotImplemented(...)` — visible in the type system and covered by a test
(`stub_real_client_returns_not_implemented`) asserting it actually returns that error rather than
panicking or silently returning empty data. The trait, metadata types
(`TableMetadata`/`ConstraintMetadata`/`ViewMetadata`), and `OracleObserver`'s mapping logic
(attaching each table's constraints to its artifact) are fully real and tested via
`MockOracleClient`.

**Salesforce's relationship signal:** `SObjectField.reference_to` (populated from the `describe`
endpoint's `referenceTo` array) is surfaced in each artifact's `data.reference_fields` — this is the
raw fact a future Phase 6-style recovery pass would use to infer a `KirRelationship`, but emitting
that inference is out of scope for an `Observer` (same boundary `FileObserver` already respects: the
Observation Layer collects facts, never interprets them).

**Content-addressing still holds across all five** — same `MockXClient` input always produces the
same `ObservationArtifact` id, verified with a `same_*_same_artifact_id`-style test per connector,
matching the existing `FileObserver`/`GitObserver` convention.

### Decisions

**No live integration tests this session** — explicitly deferred, not silently skipped. RFC 0012's
Acceptance Criteria and this devlog both name it. TODO.md's `[~]` (in progress, not done) reflects
that the phase's actual validation criterion — a passing `cargo test -p plugin-<name> --features
integration` against real credentials — is unmet.

**Kubernetes connector deferred to its own pass**, not attempted alongside these four. It's the one
Phase 14 connector that's fully testable locally (via `kind`, no vendor credentials needed), so it
deserves a real implementation-and-verification pass of its own rather than another mock-only
scaffold bundled in with the four that genuinely can't be verified here.

---

## Knowledge Captured

- **Native-dependency connectors (SAP `nwrfc`, Oracle ODPI-C) are a different risk class from
  REST-based ones.** Adding either as a hard `Cargo.toml` dependency would require vendor SDK
  headers/libraries present at *build* time, not just at run time — breaking `cargo build
  --workspace` for every contributor without them installed, not just failing the one connector's
  tests. REST-based connectors (Salesforce, Fabric, Snowflake, SAP-via-OData) don't have this
  problem; that asymmetry is why SAP ended up OData-only and Oracle ended up a stub rather than
  both getting a "best-effort" native binding.
- **The `LlmProvider`/`MockLlmProvider` pattern from RFC 0008 generalizes cleanly to any
  "real-network-call vs. in-process-fixture" trait boundary** — every Phase 14 connector this
  session reused it verbatim (trait + real impl + mock impl + constructor-injected into the
  consumer). Worth reaching for first the next time a new external-system boundary needs both a
  production path and a testable-without-network path.
- **Phase 14's own TODO context blurb** ("Phases 0–13 built and proved the compiler with Git,
  Postgres, SQL Server, and file system sources") **overstates what actually exists** — there is no
  live Postgres/SQL Server connector in this codebase; SQL analysis happens over static DDL text
  via `SqlAnalyzerPass`, not a live database connection. Worth knowing before assuming a "same
  surface as the Postgres connector" comparison (as Oracle's TODO item asks for) has a real
  precedent to compare against — it doesn't; `OracleObserver`'s shape was designed from first
  principles instead.

---

## Files Changed

| File | Change summary |
|---|---|
| `docs/rfcs/0012-enterprise-connectors-scaffold.md` | New RFC (accepted) |
| `ekos/plugins/salesforce/{Cargo.toml,src/lib.rs}` | New crate — 4 tests |
| `ekos/plugins/sap/{Cargo.toml,src/lib.rs}` | New crate — 4 tests |
| `ekos/plugins/oracle/{Cargo.toml,src/lib.rs}` | New crate — 4 tests |
| `ekos/plugins/fabric/{Cargo.toml,src/lib.rs}` | New crate — 3 tests |
| `ekos/plugins/snowflake/{Cargo.toml,src/lib.rs}` | New crate — 3 tests |
| `ekos/Cargo.toml` | 5 new workspace members |
| `TODO.md` | 4 Phase 14 connector items marked `[~]` with scaffolding-status notes |
