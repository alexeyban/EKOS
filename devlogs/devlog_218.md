# Devlog 218 — RFC 0161: risk that is computed, and two controls that were decoration

**Date:** 2026-09-26
**PRs:** none — committed to main, local gates green, `[skip ci]`
**Branch:** main

---

## Summary

`ekos-migrate-approval` and `ekos migrate review | approve | reject`. Risk computed from the
situation rather than assigned by category, approval requests pinned to an evidence snapshot, and the
human-only decision path. This closes the gap devlog_217 flagged: `authorize` was being passed `None`
and only sandbox writes could ever succeed.

Live, end to end:

```
$ ekos migrate load --unit ekos_fk.orders --env staging --dry-run
Error: ekos_fk.orders:ddl is R2 and has no matching approval.
  R2 because: affected rows were not measured, so no row-count escalation could be evaluated

$ ekos migrate review --unit ekos_fk.orders --env staging
  covers   : 2 artifact(s) — the DDL and every chunk, so one decision covers the load
  evidence : 3 fact(s) frozen
  requester: cli:legion — whoever approves must be someone else

$ ekos migrate approve REQ:ekos_fk.orders:staging --as legion
Error: an approver may not be the requester. human:legion raised this request, so somebody else
       has to approve it.

$ ekos migrate approve REQ:ekos_fk.orders:staging --as sam
Approved REQ:ekos_fk.orders:staging (R2)
```

**Running it found two of my own controls that were decoration and one determinism bug.** Each looked
correct, passed its unit tests, and did nothing.

---

## Control 1: self-approval was allowed

`approve` refuses an approver who is the requester. The unit test passed. The first real run
approved my own request.

The requester is recorded as `cli:legion` and an approver as `human:legion`. Those are different
strings, so the comparison never matched — and the unit test passed throughout because it happened to
use `agent:session-7` and `human:alex`, two identities that genuinely differ.

The check now compares **identity**, stripping the leading scheme, with a test over four
requester/approver label pairs that are the same human spelled differently. The general shape: a
guard comparing two identifiers needs a test where they are the *same subject in different
notations*, because that is the case it exists for and the easiest one to miss.

## Control 2: an escalation at an already-reached class was silently dropped

`assess` raised the class and appended the reason in one step, so the second reason at the same class
never got recorded. An approver saw:

> `R3 because: the mapping is lossy`

and never learned that 40 consumers depend on it and 17,000 rows are affected. The entire point of
computing risk rather than assigning it is that the approver reads *reasons*, and the implementation
was throwing most of them away. Now every condition that fires is recorded and the class is the
maximum of the base and everything recorded — two separate steps.

The same function also escalated a **read** in production to R4, because the production rule applied
unconditionally. A read in production is R1: real load on somebody's live system, worth logging, not
a two-approver event.

## The determinism bug: hashing a `HashMap`

The evidence snapshot reported *every* fact as changed immediately after being frozen. `KirObject::properties`
is a `HashMap`, and serializing one gives a different key order on each read — so the "content hash"
was not a content hash at all. Sorted into a `BTreeMap` before hashing.

This is RFC 0135's determinism concern arriving somewhere new. Anywhere a hash is taken over a map,
the map has to be ordered first, and the failure is invisible until two reads are compared.

---

## Design notes

**A request covers every artifact, not just the first.** The first version froze only the DDL, which
approved the create and then refused chunk 0. RFC 0161 says "the artifact ids and their content
hashes", plural. `review` and `load` now share one `plan_artifacts` — if they built the set
separately they could disagree, and an approval covering different statements than the ones that run
is exactly what the hash check exists to catch; better not to create the opportunity.

**The two gates compose rather than duplicate.** RFC 0161 decides *whether* an approval is needed and
finds it; RFC 0160 checks that the approval matches this artifact's hash and environment. Neither is
sufficient alone — a valid approval for a different statement is precisely the hash check's job.

**`--chunk-rows` on `load` deliberately invalidates an approval.** The flag overrides the config, which
changes the artifact set, which changes the hashes. The hash check then refuses rather than quietly
loading a different set of chunks than the one somebody approved.

**Changed evidence makes a request dead, not warned-about.** Re-validating would mean deciding which
changes matter, which is the approver's judgement rather than the tool's. There is no override flag,
because any such flag becomes the documented workaround within a month.

**The human-only guard now names both lifecycle modules.** A second decision path is a second way in,
and a source scan that names only the one somebody remembered protects only that one.

---

## Knowledge Captured

**A control that passed its unit tests did nothing in production, twice, for the same reason:** the
test used inputs that differed in the dimension the bug was in. Self-approval used two different
names; the escalation test used one escalation. Both times the fix included a test built from the real
failure rather than from the happy path.

**"The default policy allowed it" is a different statement from "our policy allowed it".** `LoadedPolicy`
carries `from_file`, and `load` prints which one is in force. An approval workflow whose thresholds
came from a built-in default, reported as though they were the organisation's, is worse than no
policy file.

**An unmeasured action records the gap at its current class rather than escalating.** It cannot be
escalated by a number nobody has, and it must not read as affecting zero rows either. The assessment
says *"affected rows were not measured, so no row-count escalation could be evaluated"* — which is
what appeared in the live run above, and is the honest reason that load was R2.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/crates/migrate-approval/src/risk.rs` | New — `RiskClass`, `assess`, escalations recorded individually, 11 tests |
| `ekos/crates/migrate-approval/src/request.rs` | New — `EvidenceSnapshot`, `ApprovalRequest`, identity comparison, 11 tests |
| `ekos/crates/migrate-approval/src/lifecycle.rs` | New — human-only `Actor`, 3 tests incl. the one-variant guard |
| `ekos/crates/migrate-approval/src/policy.rs` | New — `migrate.policy.toml`, content hash, `from_file`, 4 tests |
| `ekos/crates/cli/src/commands/migrate.rs` | `review`, `decide`, `plan_artifacts`, `blast_radius`, `current_hashes`; the gate wired into `load`; guard extended |
| `ekos/crates/cli/src/app.rs` | `review`, `approve`, `reject` |
| `ekos/crates/migrate/src/{kinds,profile_facts}.rs` | `MigrationApproval` kind + writer |
| `ekos/crates/kir/src/custom_kinds.rs` | Registry row |
| `ekos/crates/compiler-core/src/config.rs` | `chunk_rows` |
| `TODO.md` | RFC 0161 ticked |
