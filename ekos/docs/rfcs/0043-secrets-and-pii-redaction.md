# RFC 0043 — Global Secrets/PII Redaction

**Status:** Draft
**Author:** EKOS team
**Created:** 2026-08-09

---

## Motivation

EKOS observes raw file content from every connected system and compiles it into an append-only
ledger — a ledger that, by the project's own core invariant, is never modified in place (CLAUDE.md
"Key invariants"). That combination is dangerous by default: if a workspace happens to contain a
`.env` file, an AWS key pasted into a script, or a private key checked in by mistake, today's
pipeline copies that content into `ObservationArtifact`s, `KirObject` properties, and ultimately
the ledger — permanently, by design, with no delete path (confirmed: `grep`ing the whole
`crates/`/`plugins/` tree for `tombstone`/object-level `delete` finds nothing; only
`prune_snapshots`/`prune_empty_dirs`, which clean derived caches, not ledger content). The user
asked for a **global limitation**: EKOS must never scan and save secrets, API tokens, passwords,
personal data, or other confidential information, on by default, not something a workspace can
turn off entirely.

Investigated directly before writing this RFC:

- `ekos/crates/observation-sdk/src/lib.rs:159-208` — `Observer::scan` returns an
  `ObservationPackage` of `ObservationArtifact`s; there is no filtering hook anywhere between an
  observer producing content and it reaching the store.
- `ekos/crates/cli/src/commands/build.rs:158-171` — every observer's artifacts funnel through
  **one** loop (`for artifact in &package.artifacts { ... artifact_store.write(...) }`) before
  persistence — the single most centralized interception point across all ~15 connector plugins,
  present and future, without touching each one individually.
- `ekos/crates/cli/src/commands/build.rs:184-214` — a **second** leak point in the same file:
  `artifact.content.data["excerpt"]`/`["symbols"]` get copied again, verbatim, into `KirObject`
  properties that land directly in the ledger.
- `ekos/crates/cli/src/commands/recover.rs:89,178,353,408` — four separate blocks (SQL dialect
  scan, RFC 0019 dependency-scan, this session's `crate_topology_analyzer`/`cicd_analyzer` file
  collection) call `std::fs::read_to_string` directly, **bypassing the artifact store entirely** —
  a second raw-content entry point that a build.rs-only fix would miss.
- `ekos/crates/recovery/src/crypto_analyzer.rs` — despite the name, this is unrelated (RFC 0017
  DeFi/cryptocurrency export data mapping, not secret detection); no existing secret-scanning code
  exists anywhere to reuse. `dependency_analyzer.rs`'s pattern-table shape (`const PATTERNS: &[(&str,
  &str)]`, case-insensitive substring match, RFC 0019) is the closest precedent and the template
  this RFC's regex-pattern table follows.
- `ekos/crates/kir/src/lib.rs:264-270` — `KirEvidence.fragment: String`. Checked `sql_analyzer.rs`
  and `dependency_analyzer.rs`: both build derived/descriptive fragment strings, never a verbatim
  source copy. No analyzer pass constructs a `fragment` from raw source text today — the two real
  leak points are the `file` observer's `excerpt`/`symbols` (into `KirObject` properties, not
  `KirEvidence`) and `recover.rs`'s four direct-read blocks.
- `ekos/crates/compiler-core/src/config.rs:6-21` — `EkosConfig` is `#[serde(deny_unknown_fields)]`
  with one field per `ekos.toml` table (`[observe]`, `[llm]`, `[marketing]`, …); a new `[security]`
  section follows the exact same opt-in-table pattern `[document-semantics]` already uses.
- No `regex` crate is in the workspace yet (`glob = "0.3"` already is, used by
  `dependency_analyzer.rs` and others) — added here for pattern matching.

Two scoping decisions confirmed with the user before design:

1. **Both redact-in-place and drop-entirely, not one or the other.** Most files get their matched
   secret spans replaced with a labeled placeholder, keeping the rest of the file's structural
   value (a config file with one leaked token shouldn't lose all its other, safe keys). A
   configurable glob list (with a sensible built-in baseline — `.env`, `*.pem`, `id_rsa`, …) is
   instead **fully excluded** — never read into an artifact at all — because a file matching one
   of those patterns is near-100% secret material with no useful non-secret structure worth
   keeping.
2. **On by default, cannot be fully disabled.** `ekos.toml`'s new `[security]` section can only
   *extend* the pattern/exclusion lists (`extra-patterns`, `extra-excluded-globs`); there is no
   `enabled = false` escape hatch. This matches "global limitation," not an opt-in feature.

## Scope

A new `ekos-common::redaction` module (baseline secret-pattern regex table + baseline
fully-excluded-file glob table + `redact()`/`redact_json()`/`is_excluded_path()` functions),
wired into the two real content-entry points found above (`build.rs`'s artifact loop,
`recover.rs`'s four direct-read blocks), plus a new `[security]` `ekos.toml` config section that
can only add to the baseline.

## Non-goals / Known limitations

- **Not a DLP/entropy-based scanner.** Like `dependency_analyzer.rs`'s own pattern table, this is
  a fixed, transparent list of well-known secret *shapes* (AWS/GitHub/Slack/Google/Stripe token
  prefixes, PEM private-key blocks, JWTs, and a generic `key/secret/password/token = value`
  assignment pattern) — not a statistical high-entropy-string detector. It will miss secrets that
  don't match a known shape; it is a baseline floor, not a guarantee of zero leakage. Documented
  explicitly, the same honesty standard RFC 0019 already set ("not exhaustive... answers 'what
  obviously depends on X'").
- **The content-addressed `ArtifactId` is still computed from pre-redaction bytes.** Each
  connector plugin builds its `ObservationArtifact::new(...)` (which hashes `content` into the
  `id`, `crates/artifact/src/lib.rs:145-160`) *before* `build.rs`'s central redaction pass ever
  runs, since that pass operates on the already-constructed `package.artifacts`. The **data** is
  fully redacted before it's ever written to the store or copied into a `KirObject`; only the
  opaque id hash technically derives from the original bytes. A cryptographic hash doesn't
  reversibly leak its input, so this is low-severity (an attacker who already possesses the exact
  secret string could confirm-by-hash that it was once present — the same property git blob SHAs
  already have) — noted here rather than silently accepted. Redacting before every individual
  plugin's own `data` construction (~15 call sites) would close this too, but is out of scope for
  v1; the central choke point was chosen specifically because it protects every current *and
  future* connector automatically, without relying on each plugin author remembering to call a
  helper.
  _Tracked as backlog (security-relevant, not routine cleanup): see `TODO.md` → "Promoted from
  RFC Non-Goals" → "Security"._
- **PII scope is regex-shaped secrets/tokens/credentials, not free-text personal information in
  prose** (e.g. a name mentioned in a document). Structured, intentionally-modeled personal data
  that connectors already extract on purpose — git commit author name/email
  (`git_analyzer.rs`, RFC 0007's `Person` object) — is explicitly **not** redacted; that's a
  deliberate, labeled `KirObject` property, not incidental leakage of raw text, and redacting it
  would break a real, intentional feature (contributor attribution). This RFC only touches raw
  free-text content fields (`excerpt`, `symbols`, SQL/manifest/workflow file bodies), never
  already-structured connector metadata.

## Design

### `ekos-common::redaction` (new module)

```rust
pub struct RedactionConfig {
    pub extra_patterns: Vec<(String, String)>,   // (label, regex) — additive
    pub extra_excluded_globs: Vec<String>,        // additive
}

pub fn is_excluded_path(rel_path: &str, config: &RedactionConfig) -> bool;
pub fn redact(content: &str, config: &RedactionConfig) -> String;
pub fn redact_json(value: &mut serde_json::Value, config: &RedactionConfig);
```

- Built-in secret patterns (regex, compiled once via `OnceLock`): AWS access key id (`AKIA...`),
  GitHub token (`gh[pousr]_...`), Slack token (`xox[baprs]-...`), Google API key (`AIza...`),
  Stripe key (`sk_(live|test)_...`), PEM private-key block (`-----BEGIN ... PRIVATE
  KEY-----...-----END ... PRIVATE KEY-----`), JWT (`eyJ...\....\....`), and a generic
  case-insensitive `(api[_-]?key|secret|password|passwd|access[_-]?key|auth[_-]?token)\s*[:=]\s*
  ...` assignment pattern. Each match is replaced with `[REDACTED:<label>]` — the placeholder
  names *what kind* of secret was found without ever retaining it.
- Built-in excluded-file globs (matched against the file's base name via `glob::Pattern`, the
  crate already in the workspace): `.env`, `.env.*`, `*.pem`, `*.key`, `*.pfx`, `*.p12`, `id_rsa`,
  `id_rsa.*`, `id_ed25519`, `id_ed25519.*`, `*.ppk`, `credentials`, `credentials.json`, `.npmrc`,
  `.netrc`, `.pgpass`, `*.jks`, `*.keystore`.
- `redact_json` walks a `serde_json::Value` recursively (strings redacted in place; arrays/objects
  recursed into) — needed because `ObservationArtifact.content.data` and `excerpt`/`symbols` are
  JSON, not a single string.

### `[security]` config (`ekos/crates/compiler-core/src/config.rs`)

```toml
[security]
extra-patterns = [{ label = "internal-token", regex = "ITKN-[0-9a-f]{32}" }]
extra-excluded-globs = ["secrets/*.yaml"]
```

`SecurityConfig { extra_patterns: Vec<SecretPatternConfig { label, regex }>, extra_excluded_globs:
Vec<String> }`, added to `EkosConfig` as a new field (same opt-in-table pattern as
`DocumentSemanticsConfig`). No `enabled` flag — the baseline always runs; this section only adds.

### Wiring

- **`build.rs`** (the primary "global" interception point, RFC's central architectural choice):
  right after `let mut package = observer.scan(&ctx).await?;`, before either the artifact-write
  loop or the `KirObject`-property loop reads `package.artifacts`: `package.artifacts.retain(|a|
  !is_excluded_path(&a.content.target, &redaction_config))`, then `for artifact in &mut
  package.artifacts { redact_json(&mut artifact.content.data, &redaction_config); }`. One pass,
  covers every observer, including any connector written after this RFC ships.
- **`recover.rs`**: each of the four direct-`std::fs::read_to_string` blocks (SQL dialect scan,
  dependency-scan, `crate_topology_analyzer`, `cicd_analyzer` file collection) gains an
  `is_excluded_path` check before reading (skip the file, `continue`) and a `redact()` call
  immediately after reading, before the content is handed to the relevant `*AnalyzerPass`.

## Alternatives Considered

- **Per-plugin redaction (each of the ~15 `Observer` implementations calls the helper itself)** —
  rejected as the primary mechanism (though not incompatible with adding later): relies on every
  plugin author remembering to call it, including future ones; the central `build.rs` choke point
  can't be forgotten. Documented as the RFC's one known gap (pre-redaction `ArtifactId` hash) in
  Non-goals rather than solved by this alternative, given the low severity.
- **Full entropy-based/statistical secret scanner** — rejected for v1 as scope creep matching
  `dependency_analyzer.rs`'s own precedent of a fixed, transparent, "not exhaustive" pattern table
  over a heavier statistical approach; revisit if the fixed-pattern baseline proves insufficient
  in practice.
- **`enabled = false` escape hatch** — rejected per explicit user direction: "on by default,
  cannot be fully disabled."

## Testing

- `ekos-common::redaction` unit tests: each built-in pattern redacts a realistic fake secret
  (fake `AKIA...` key, fake `ghp_...` token, a PEM block, a JWT, a `password = "..."` assignment)
  and leaves surrounding safe text untouched; `is_excluded_path` matches `.env`/`*.pem`/etc. and
  does not match ordinary source files; `extra_patterns`/`extra_excluded_globs` from config are
  additive (baseline still fires with an empty config).
- `build.rs` integration test: a fixture file containing a fake AWS key is observed; the persisted
  `ObservationArtifact` and the resulting `KirObject`'s `excerpt` property both contain
  `[REDACTED:aws-access-key-id]`, never the fake key string. A second fixture named `.env` is
  observed; zero artifacts/objects are written for it.
- `recover.rs` integration test: a `Cargo.toml`/SQL/workflow fixture containing a fake secret is
  fed through the relevant analyzer pass; the resulting `KnowledgeArtifact`'s KIR contains the
  redaction placeholder, never the fake secret.

## Acceptance Criteria

- [ ] `cargo build --workspace && cargo test --workspace && cargo clippy --workspace -- -D
      warnings && cargo fmt --check` clean from `ekos/`.
- [ ] A workspace containing a fake AWS key in a tracked file, and a `.env` file, run through
      `ekos build && ekos recover && ekos resolve && ekos compile && ekos commit`: no committed
      ledger object or evidence fragment contains the fake key string or any `.env` content.
- [ ] `[security]` config can add patterns/exclusions; no config combination disables the
      built-in baseline.

## Files Changed (planned)

| File | Change |
|---|---|
| `ekos/crates/common/src/redaction.rs` | New: `RedactionConfig`, `redact`, `redact_json`, `is_excluded_path`, built-in pattern/glob tables |
| `ekos/crates/common/src/lib.rs` | `+pub mod redaction;` |
| `ekos/crates/common/Cargo.toml`, `ekos/Cargo.toml` | `+regex` dependency |
| `ekos/crates/compiler-core/src/config.rs` | `+SecurityConfig`, `+SecretPatternConfig`, `+EkosConfig.security` field |
| `ekos/crates/cli/src/commands/build.rs` | Central redaction pass right after `observer.scan()` |
| `ekos/crates/cli/src/commands/recover.rs` | Exclusion check + redaction at all four direct-read blocks |
