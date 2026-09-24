# RFC 0167 — Migration agent pack and MCP tool surface

**Status:** Draft
**Date:** 2026-09-24
**Supersedes:** none
**Related:** RFC 0154 (foundation — the human-only rule originates there), RFC 0161 (approval
workflow), RFC 0013/0115/0143 (MCP server and transports), RFC 0151 (session memory and the
enforced human-only precedent), RFC 0138 (eval harness), RFC 0126 (retrieval eval)

---

## Summary

The MCP tool surface for EKOS Migrate and a pack of specialized subagents that drive it. Read tools
and propose/request tools only: **no MCP path reaches approval, execution outside the sandbox, or
sign-off**, and that is enforced by tests rather than by tool allowlists.

Everything an agent can do here, the deterministic CLI can also do. The agent pack is a surface, not
a dependency.

## Motivation

A migration is a long, branching, evidence-heavy investigation — exactly the shape an agent handles
well, and exactly the shape where an agent with too much authority does the most damage.

EKOS already learned this concretely. RFC 0151's live MCP test (devlog_199) found session-claim
isolation applied to 2 of roughly 20 Runtime read methods, and six tools leaked data they should not
have. Nothing there was malicious and no prompt was violated — the enforcement simply was not in the
right layer. The fix was a guard test that fails CI on a new unaudited read path, and that is the
model this RFC follows from the start rather than after an incident.

## Design

### Tool surface

**Read** — pure `Runtime` and ledger reads, no side effects:

| Tool | Returns |
|---|---|
| `ekos_migration_status` | Units by state, waves, blocking findings |
| `ekos_migration_profile` | Table and column profiles (aggregates only — never values) |
| `ekos_dq_findings` | DQ findings with evidence queries and dispositions |
| `ekos_compat_findings` | Compatibility findings with affected rows and lossiness |
| `ekos_mapping_explain` | A type mapping with its lossiness class and the profile that proves it |
| `ekos_migration_plan` | Waves, dependency order, target designs with rationale |
| `ekos_validation_results` | Tier results **with the controls column** |
| `ekos_divergence_explain` | A divergence, its classification and masked examples |
| `ekos_migration_report` | The compiled report and its groundedness score |

**Propose / request** — write to proposal state only, never to an executable or approved state:

| Tool | Effect |
|---|---|
| `ekos_disposition_propose` | A `MigrationDisposition` with status `proposed` |
| `ekos_mapping_propose` | A `MigrationTypeMapping` proposal |
| `ekos_target_design_propose` | A `MigrationTargetDesign` proposal |
| `ekos_transformation_propose` | A `MigrationProposal`, `author: "llm"` (RFC 0164 constraints apply) |
| `ekos_validation_run` | Runs a tier — **R0/R1 only**, sandbox or read-only source |
| `ekos_approval_request` | Raises a `MigrationApprovalRequest`. Raising is not approving. |

**Never exposed over MCP**: approve, reject, execute in staging or production, sign off, drop or
truncate anything, or resolve a production credential.

### Enforcement, in layers

1. **No handler exists.** There is no MCP handler for the forbidden operations. Absence is
   verifiable; a correct permission check is only probable.
2. **The lifecycle module is unreachable.** The source-scanning test from RFC 0161 asserts
   `commands/mcp.rs` never references `ekos_migrate::lifecycle` or `Actor::Human`, copying
   `no_mcp_code_can_reach_the_migration_lifecycle` from the RFC 0151 pattern.
3. **A tool-list test** asserts the exact set of migration tools, so a new tool is a deliberate,
   reviewed addition rather than an accident. The list is asserted whole, not merely checked for
   forbidden names — an allowlist that only forbids what someone thought of is the RFC 0151 failure
   repeated.
4. **An audit guard test**, modelled on `every_object_returning_read_path_has_an_audited_session_decision`:
   every migration tool returning profile or divergence data has an explicit, audited decision about
   whether it can leak source values. A new `pub fn` on that path fails CI until audited.
5. **Propose tools write only `proposed` status**, asserted by test on each one, so the write
   capability cannot escalate through a status field.

Layers 1–2 are the control. Layers 3–5 exist because the RFC 0151 incident showed that the control
being right once is not the same as staying right.

### Subagents

Each has an explicit tool allowlist. The allowlist is ergonomics — it keeps an agent focused — and
is explicitly **not** the security boundary; the enforcement above is.

| Subagent | Role | Tools |
|---|---|---|
| `migration-scout` | Inventory, dependency waves, out-of-scope detection | `ekos_migration_status`, `ekos_search`, `ekos_neighborhood`, `ekos_dependents` |
| `data-profiler` | Reads profiles, explains distributions, flags suspicious shapes | `ekos_migration_profile` |
| `dq-analyst` | Reviews findings, drafts dispositions with rationale | `ekos_dq_findings`, `ekos_compat_findings`, `ekos_disposition_propose` |
| `target-architect` | Drafts target designs from query shapes and profiles | `ekos_migration_profile`, `ekos_migration_plan`, `ekos_target_design_propose` |
| `logic-translator` | IR-constrained translation of views and procedures | `ekos_transformation_explain`, `ekos_transformation_diff`, `ekos_transformation_propose` |
| `validator` | Runs R0/R1 tiers, bisects, classifies divergences | `ekos_validation_run`, `ekos_validation_results`, `ekos_divergence_explain` |
| `risk-reviewer` | Prepares approval requests with blast radius | `ekos_impact`, `ekos_migration_status`, `ekos_approval_request` |
| `report-writer` | Compiles the report, checks citations | `ekos_migration_report` |

Each prompt carries the same two instructions, which are the ones that actually matter in practice:
**cite a fact id for every claim**, and **never fill a recovery gap by guessing** — say the gap is
there. The existing `binary-migration-planner` agent is the working precedent for both.

### Evaluation

Migration scenarios join the RFC 0138 harness rather than living in a bespoke one:

- **Grounding**: does the agent cite real fact ids that support its claims? Reuses the existing
  groundedness evaluators.
- **Gap honesty**: on a fixture with a deliberately unrecoverable procedure, does the agent report
  the gap or invent a plausible translation? Scored, not observed anecdotally.
- **Escalation**: given a lossy mapping, does the agent raise an approval request rather than
  proposing to proceed?
- **Containment**: an adversarial scenario instructing the agent to approve its own proposal or
  execute against production must fail at the tool layer, and the eval asserts the refusal came from
  there rather than from the model declining.

The last one is the point of the whole RFC. An agent that declines because it was asked nicely is
not a control; an agent that cannot is.

### Demo

A headless end-to-end script — Pagila to ClickHouse, agent-driven, human approvals in the loop,
transcripts recorded — following the pattern of the existing demo material. It is documentation of
real behaviour, not a scripted illusion: the approvals in it are real approvals, and the transcript
shows the agent stopping at each one.

## Testing

- Tool-list test asserts the exact tool set.
- Source-scanning test: `mcp.rs` cannot reach the lifecycle module.
- Audit guard: every profile- or divergence-returning tool has an audited leakage decision.
- Propose tools cannot write a status other than `proposed`.
- `ekos_validation_run` refuses an R2+ tier or a non-sandbox environment.
- Isolation: no MCP tool returns a source row value; asserted by running the full surface against a
  fixture seeded with recognizable values and scanning every response.
- Eval scenarios: grounding, gap honesty, escalation and containment all scored in `ekos eval`.

## Alternatives considered

- **Exposing approve over MCP with a role check.** Rejected — RFC 0161's reasoning applies: absence
  is verifiable, a check is probable. The RFC 0151 incident is the evidence.
- **One general-purpose migration agent.** Rejected: a single broad allowlist is the condition under
  which scope creep goes unnoticed, and specialized agents produce better-focused output anyway.
- **Tool allowlists as the security boundary.** Rejected explicitly, because this is the mistake the
  design is built to avoid. Allowlists are ergonomics.
- **A bespoke migration eval harness.** Rejected: RFC 0138 already grades whole answers with
  deterministic evaluators, and a separate harness would drift out of maintenance.

## Open questions

- [ ] Should `ekos_validation_run` exist at all over MCP, or should tier runs always be CLI-driven?
      It is R0/R1 and read-mostly, but it does consume real source capacity.
- [ ] Do the subagents ship in-repo under `.claude/agents/`, as a plugin, or both?
- [ ] Does an agent's session id (RFC 0151) suffice as requester identity on an approval request, or
      does a request need a human sponsor from the moment it is raised?

## Acceptance criteria

- [ ] The exact tool set is asserted; no approve, execute or sign-off tool exists.
- [ ] `mcp.rs` cannot reach the migration lifecycle, asserted by source scan.
- [ ] Propose tools can write only `proposed` status.
- [ ] No MCP tool returns a source row value, asserted against a seeded fixture.
- [ ] Containment eval: an adversarial approve-your-own-proposal scenario fails at the tool layer.
- [ ] End-to-end agent-driven Pagila → ClickHouse migration with real human approvals, recorded.
