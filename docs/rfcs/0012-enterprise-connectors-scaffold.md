# RFC 0012 — Phase 14 Connector Scaffolding (SAP, Salesforce, Oracle, Fabric, Snowflake)

| Field | Value |
|-------|-------|
| **Status** | Accepted |
| **Author** | EKOS team |
| **Created** | 2026-07-11 |
| **Gating** | Phase 14 |

---

## Motivation

Phase 14's TODO validates each connector against a live vendor sandbox (SAP sandbox, Salesforce
developer org, Oracle XE container, Fabric/Snowflake trial accounts). None of those are available
in this environment. Rather than block the phase entirely, this RFC scopes a version that ships
real, reviewable connector code — trait boundary, request/response shapes, `Observer` mapping logic,
all exercised by unit tests against an in-process mock client — while being explicit about what
still needs a real sandbox before it's production-ready: the live HTTP/RFC wiring is written to the
vendor's documented API shape but has never been run against a real account. This mirrors the
project's established pattern of narrowing scope honestly (RFC 0009/0010/0011 all did this) rather
than shipping something that claims more certainty than it has earned.

---

## Design

### Shared shape across all five connectors

Every connector plugin follows the same structure, modeled on `LlmProvider`/`MockLlmProvider` from
RFC 0008 (`crates/recovery/src/llm.rs`):

```rust
#[async_trait]
pub trait XClient: Send + Sync {
    async fn list_objects(&self) -> Result<Vec<XObjectMetadata>, XClientError>;
    // + one or two more methods specific to what that platform exposes
}

pub struct XObserver {
    client: Arc<dyn XClient>,
}

impl XObserver {
    pub fn new(client: Arc<dyn XClient>) -> Self { ... }
}

#[async_trait]
impl Observer for XObserver {
    fn name(&self) -> &str { "x" }
    async fn scan(&self, ctx: &ScanContext) -> Result<ObservationPackage, ObserveError> {
        // client.list_objects().await, map each into one ObservationArtifact
    }
}

pub struct MockXClient { pub objects: Vec<XObjectMetadata> }
// returns pub struct XApiClient (real reqwest-based implementation) for platforms with a plain
// REST surface; see per-connector notes below for what's real vs. deferred.
```

The client is **constructor-injected**, not built from `ScanContext` — credentials and connection
details are the CLI's job to assemble (mirroring how `recover.rs` builds an `AnthropicProvider` or
falls back to a mock, not something `SqlAnalyzerPass` does internally). `ScanContext` still carries
per-scan config (e.g. which sObjects to describe) via `ConnectorConfig`.

### Per-connector decisions

**Salesforce** (`plugins/salesforce`) — REST `sobjects/<Name>/describe` is a plain authenticated
HTTP GET returning JSON; `SalesforceApiClient` is a real `reqwest`-based implementation (bearer
token + instance URL supplied at construction). One `ObservationArtifact` per sObject with its full
field list; reference-type fields (`referenceTo`) become the relationship signal Phase 6 identity
resolution will use later, but this phase writes an `ObservationArtifact` per emitting a real trace
in `data`, not a `KirRelationship` — that's a downstream compiler pass's job, same boundary
`FileObserver` already respects.

**Microsoft Fabric / Snowflake** (`plugins/fabric`, `plugins/snowflake`) — both are documented REST
APIs (Fabric: `workspaces`/`items` endpoints; Snowflake: SQL REST API for warehouse/schema
metadata). Same treatment as Salesforce: real `reqwest`-based clients, written to the documented
request/response shape, never run against a live trial account.

**SAP** (`plugins/sap`) — the original TODO wording offers "SAP OData APIs *or* RFC (Remote
Function Call) via `nwrfc`." This RFC picks **OData only**. The `nwrfc` binding requires the
proprietary SAP NetWeaver RFC SDK — native libraries that aren't installable in this environment
and that most Rust crates wrapping it (e.g. `sap-nwrfc`) require at build time via `bindgen` against
vendor headers. Adding that dependency risks breaking `cargo build --workspace` for anyone without
the SDK installed, which is a much bigger blast radius than "one connector's tests are only
mock-based." OData is REST, so `SapClient` follows the same `reqwest`-based real-client pattern as
Salesforce/Fabric/Snowflake. If RFC-based access is genuinely needed later, it should be its own
follow-up RFC that explicitly accepts the native-dependency tradeoff.

**Oracle** (`plugins/oracle`) — same native-dependency problem as SAP: the `oracle` crate wraps
ODPI-C, which needs Oracle Instant Client libraries present on the build machine. Unlike the other
four, there's no REST alternative for Oracle table/constraint/view introspection that ships in this
phase. `OracleClient`'s real implementation (`OracleDbClient`) is therefore a **documented stub**
that returns `OracleClientError::NotImplemented` — the trait, the metadata types
(`TableMetadata`/`ConstraintMetadata`/`ViewMetadata`/`ProcedureMetadata`), and the `Observer` mapping
logic are all real and unit-tested against `MockOracleClient`; only the live-database wiring is
deferred. This is called out explicitly rather than silently shipped as if it worked, per this
project's "don't claim more than is true" convention (see RFC 0009's hard-fail-on-missing-API-key
decision for the same instinct applied elsewhere).

### What's NOT in scope for this pass

- Live integration tests against real vendor sandboxes/credentials (explicitly deferred — TODO.md's
  `--features integration` validation criterion is not met by this work).
- SAP RFC/BAPI access via `nwrfc` (OData only, see above).
- Oracle's real database wiring (stub only, see above).
- Kubernetes connector (not touched this pass — it's the one Phase 14 connector that's fully
  testable locally via `kind` without vendor credentials, and is better scoped as its own follow-up
  once there's time to stand up a local cluster and verify against it for real, rather than another
  mock-only scaffold).
- The "Additional connectors on demand" backlog item.

---

## Alternatives Considered

- **Skip Phase 14 entirely until credentials exist** — rejected; the trait boundaries, request
  shapes, and `Observer` mapping logic are real, reviewable work independent of whether a live
  sandbox is available, and having them in place means the *only* remaining work once credentials
  exist is wiring + an integration test, not a redesign.
- **Add the `oracle`/ODPI-C and SAP `nwrfc` native dependencies now, guarded by a Cargo feature flag
  disabled by default** — rejected for this pass. It's a reasonable path later, but adds real
  complexity (bindgen, vendored headers, CI matrix implications) for code that can't be exercised or
  verified here anyway; better to land it alongside the actual integration-test follow-up.

---

## Open Questions

None — this RFC is deliberately scoped to avoid open questions: everything it commits to (trait
shapes, mock-based tests, which connectors get real HTTP clients vs. stubs) is fully specified above.

---

## Acceptance Criteria

- [x] Design is consistent with the Observation SDK contract (`Observer::scan` never mutates the
      workspace; identical remote state → identical artifact IDs, honored by construction since
      artifacts are content-addressed from the mapped metadata).
- [x] Every connector ships with unit tests against its mock client — parsing/mapping logic is real
      and verified, not just declared.
- [x] Every deferred piece (live integration tests, SAP RFC access, Oracle live driver) is named
      explicitly, not silently absent.
