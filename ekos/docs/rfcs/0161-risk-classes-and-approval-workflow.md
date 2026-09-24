# RFC 0161 — Risk classes, approval workflow and evidence snapshots

**Status:** Draft
**Date:** 2026-09-24
**Supersedes:** none
**Related:** RFC 0154 (foundation — risk table and human-only rule originate there), RFC 0160
(statement class, environments), RFC 0158 (lossiness findings and dispositions), RFC 0018
(`ekos_impact` — blast radius), RFC 0151 (human-only lifecycle precedent, enforced by test),
RFC 0131 (console OIDC and role split)

---

## Summary

How a risk class is **computed** rather than guessed, how an approval is pinned to the exact
evidence it was granted on, who may approve, and how a wrong decision is retracted in an append-only
ledger.

## Motivation

Approval workflows fail in three predictable ways, and all three are avoidable mechanically.

1. **Risk is assigned by category, not by situation.** "Schema changes need approval" means the
   trivial rename and the column drop that breaks eleven downstream consumers get the same dialog.
   Approvers learn the dialog means nothing and click through it.
2. **Approval is granted on evidence that then changes.** A lossy mapping is approved when it
   affects 3 rows; by execution it affects 30,000, and the approval still reads as valid.
3. **The agent approves its own proposal.** Not necessarily maliciously — a tool named
   `approve_mapping` in an allowlist is used by a model doing exactly what it was asked to do.

## Design

### Computing risk

```
risk(action) = max over the action's statements of:
    base(statement_class, environment)
  ↑ escalate_if(lossiness == lossy)                      → at least R3
  ↑ escalate_if(blast_radius > policy.blast_threshold)   → at least R3
  ↑ escalate_if(affected_rows > policy.rows_threshold)   → at least R3
  ↑ escalate_if(environment == production)               → R4
```

Base is a table over `(StatementClass, Environment)` from RFC 0160:

| | sandbox | staging | production |
|---|---|---|---|
| `Read` | R0 | R0 | R1 |
| `DdlCreate` | R1 | R2 | R4 |
| `DdlAlter` | R1 | R2 | R4 |
| `DmlInsert` | R1 | R2 | R4 |
| `DmlMutate` | R1 | R2 | R4 |
| `Destructive` | R1 | R3 | R4 |

**Blast radius is real, not a heuristic.** It is `ekos_impact` over the CKM: the views, materialized
views, functions, ETL jobs, application files and documents that reference the object, multi-hop.
A column drop on a table read by one dbt model and a column drop on a table read by forty services
are genuinely different actions, and EKOS is one of the few systems that can tell them apart —
because it compiled the application code alongside the schema.

The computed assessment is a `MigrationRiskAssessment` fact listing every escalation that fired and
why, so an approver sees "R3 because: lossy decimal narrowing (17 rows affected); 23 downstream
consumers" rather than "R3".

### Approval requests and evidence snapshots

A `MigrationApprovalRequest` freezes:

```
action            what would execute (artifact ids and their content hashes)
risk              the MigrationRiskAssessment id
evidence_ids      every fact the assessment rested on
evidence_hash     hash over (evidence_ids, their content hashes) — canonically ordered
requester         agent session id or human OIDC subject
```

At approval time and again at execution time, `evidence_hash` is recomputed and compared. A mismatch
means something the decision rested on has changed, and the request is **dead** — not re-validated,
not warned about. A new request must be raised showing the new numbers. This directly answers
failure mode 2, and it is cheap: the common case is a hash comparison.

The snapshot is ordered canonically so the hash is reproducible across processes, following
RFC 0135 Part C determinism conventions.

### Who may approve

| Class | Gate |
|---|---|
| R0 | none |
| R1 | automatic within policy, logged as a `MigrationApprovalRecord` with actor `policy` |
| R2 | one approver |
| R3 | one approver **plus** evidence review — the request renders the IR diff, affected rows and blast-radius graph, and records that they were displayed |
| R4 | two distinct approvers, and a typed confirmation of the object name (not a checkbox) |

Approvers are OIDC subjects from the console's RFC 0131 role split. Two distinct approvers means two
distinct subjects; a test asserts the same subject cannot satisfy both slots.

### Human-only, enforced in code

Approval, execution outside the sandbox and sign-off live in `ekos-migrate`'s `lifecycle` module.
`crates/cli/src/commands/mcp.rs` may not reference it, asserted by a source-scanning test copied
directly from `no_mcp_code_can_reach_the_lifecycle_module`
(`crates/cli/src/commands/session.rs:531`):

```rust
#[test]
fn no_mcp_code_can_reach_the_migration_lifecycle() {
    let mcp = include_str!("mcp.rs");
    assert!(!mcp.contains("ekos_migrate::lifecycle"));
    assert!(!mcp.contains("Actor::Human"));
}
```

Crude, and it works — it is the reason RFC 0151's isolation survived a live agent test. The
`Actor` enum has no `Agent` variant, for the same reason `session::lifecycle::Actor` has none: an
absent variant cannot be constructed by a future caller who did not read this RFC.

MCP exposes `ekos_approval_request` (raise a request) and the read tools. Nothing else.

### Retraction: supersede, never delete

The ledger has no delete. A wrong approval, a stale disposition or a request built on bad evidence
is superseded:

1. The object is re-appended with `status: "superseded"` and `superseded_by` pointing at the
   replacement.
2. A `MigrationStatusChanged` event records actor, timestamp and reason.
3. Any unit whose state depended on it returns to the state before that dependency, by an ordinary
   `MigrationTransition` — the state machine has no special case for retraction.

`session::lifecycle` is the working implementation of this shape. Nothing is edited in place and
nothing disappears; `ekos ledger audit` shows the whole sequence.

### Policy

`migrate.policy.toml`, versioned in the repository beside `ekos.toml`:

```toml
[thresholds]
blast_radius     = 10
affected_rows    = 1000
p2_scan_rows     = 50_000_000

[approvers]
r2 = ["group:data-eng"]
r3 = ["group:data-eng", "group:data-owner"]
r4 = ["group:data-owner", "group:cto"]

[environments.production]
require_two_approvers = true
require_typed_confirmation = true
```

The policy file's own content hash is recorded on every approval record, so "the thresholds were
different then" is answerable rather than arguable.

## Testing

- Risk computation: table-driven over `(class, environment, lossiness, blast, rows)`, asserting
  every escalation path and that escalation is monotonic.
- Evidence pinning: approve a request, mutate an underlying fact, assert execution is refused; assert
  a fresh request shows the new numbers.
- Two-approver: the same OIDC subject cannot fill both R4 slots.
- Human-only: the source-scanning test above; plus a test that the MCP tool list contains no approve,
  execute or sign-off tool.
- Supersede: a superseded approval is not executable, the replacement is, and `ekos ledger audit`
  shows both with the reason.
- Negative: no R2-or-above action executes without a matching approval record — asserted per class.

## Alternatives considered

- **Risk as a static per-action-type table.** Rejected — failure mode 1, and it discards the blast
  radius EKOS uniquely has.
- **Re-validating a changed evidence snapshot instead of killing the request.** Rejected: "re-validate"
  means deciding which changes matter, which is the approver's judgement, not the tool's.
- **Exposing approval over MCP with a role check.** Rejected. The check would be correct and the
  control would still be wrong: the point is that no agent path exists at all, and absence is
  verifiable where a correct check is merely probable.
- **Deleting bad approvals.** Impossible — the ledger is append-only — and undesirable: the wrong
  decision and its correction are both part of the audit record.

## Open questions

- [ ] CLI approval outside the console: OIDC device flow, or is console-only acceptable for v1?
      (Inherited from RFC 0154.)
- [ ] Should R1 auto-approval require a policy signature, or is the policy file's content hash on
      the record sufficient?
- [ ] Time-boxing: should an approval expire after N hours even when its evidence has not changed?

## Acceptance criteria

- [ ] Risk is computed from statement class, environment, lossiness, blast radius and affected rows,
      with every escalation recorded on the assessment.
- [ ] An approval whose evidence changed cannot be executed, asserted by test.
- [ ] No MCP tool can approve, execute outside the sandbox, or sign off — asserted by both the
      source-scanning test and a tool-list test.
- [ ] No R2+ action executes without a matching approval, asserted per risk class.
- [ ] Supersede leaves a complete, auditable history.
