# Devlog 217 — RFC 0160: a real migration, gated at every statement

**Date:** 2026-09-25
**PRs:** none — committed to main, local gates green, `[skip ci]`
**Branch:** main

---

## Summary

The first component that writes to a target. A statement classifier, environments, artifact hash
pinning, chunked loading, a dry-run gate, and `ekos migrate validate`.

**A real PostgreSQL table was migrated to ClickHouse and validated across both engines.** Then a
one-row change was planted in the target and re-validated: V1 and V2 passed, V3 caught it and named
the bucket — exactly the tier table RFC 0156 specifies, on real data.

```
  v1 passed — 0 control(s) fired
  v2 passed — 0 control(s) fired
  v3 FAILED — 1 unexplained divergence(s)
    ekos_fk.orders:bucket:24 — source count 8 sum 5132663414281568055, target count 8 sum 5056821149750093473
```

---

## The credential decision

ClickHouse's `postgresql()` table function accepts a password positionally:

```sql
postgresql('host:5432', 'db', 'orders', 'user', 'hunter2')
```

EKOS refuses to emit that. A generated statement is hashed, pinned to an approval, printed in a dry
run and pasted into tickets — a production password must not travel that road. The load uses a
**named collection** instead, configured in ClickHouse's own server config by whoever administers it:

```sql
postgresql(ekos_migrate_source, schema = 'ekos_fk', table = 'orders')
```

The classifier enforces it: a call to `postgresql`, `mysql`, `mongodb`, `s3` or `url` with more than
one positional argument is refused, in a sandbox as much as in production, because the leak is in
the artifact rather than in the execution.

`docker-compose.migrate.yml` now mounts the collection config, which is also the realistic setup —
the sandbox matches how a real deployment works rather than taking a shortcut the real one cannot.

---

## The classifier, and two things it refused that I had to accept

Five rules, each because the obvious alternative fails: parse never match text; unparseable is
refused; a batch's class is the maximum of its parts; `Unknown` is refused; an unfiltered
`DELETE`/`UPDATE` is `Destructive`.

Then the classifier refused my own generated DDL, twice.

**`CODEC(...)` does not parse.** `sqlparser`'s ClickHouse dialect rejects a codec annotation in a
`CREATE TABLE` and in an `ALTER TABLE ... MODIFY COLUMN` — both probed. The options were to weaken
the control so the optimization fits, or to drop the optimization. A codec is a compression choice
and the classifier is a safety one, so codecs are no longer emitted; the design still chooses them
and `rationale_comment` carries them, visible to whoever reviews the DDL.

**`PARTITION BY` does not parse either**, and that one is not an optimization. Silently dropping
partitioning from a 500M-row table's DDL is as unacceptable as bypassing the control, so
`create_table` **refuses to emit** a partitioned design, with an error saying exactly that and the
expression preserved in the comment. The real fix is extending the dialect parser, which is now in
TODO.

Preprocessing the text before classification was the tempting third option and is the worst one: the
classified statement would not be the executed statement, which is the hole the whole control exists
to close.

---

## Knowledge Captured

**An emitter and its verifier must be tested against each other, or the gap surfaces at the first
real run.** `every_generated_statement_passes_the_classifier` asserts that everything `create_table`
produces passes `batch_class`. I added it while fixing the codec problem and it immediately found the
`PARTITION BY` problem — a second mismatch I did not know about, in a case none of the unit tests
covered. Any generator with a downstream validator wants this invariant, and it is three lines.

**My own credential check broke the module's first rule.** The first version scanned the SQL *text*
for `postgresql(` and refused `WHERE note = 'url(a,b,c)'`, because a string literal contains the
pattern. The module's rule 1 is "parse; never match text", and the credential check — the newest and
most safety-critical part of it — was the one piece doing exactly that. Rewritten on the AST.

**ClickHouse keyword arguments parse as *unnamed* arguments.** `postgresql(name, table = 'orders')`
comes back as `FunctionArg::Unnamed` holding an `Eq` expression, not `FunctionArg::Named`. Counting
unnamed arguments to detect positional credentials therefore read a perfectly safe named-collection
call as five positional ones. A genuine positional argument is an unnamed, *non-assignment*
expression.

**Half-open chunk bounds, and the last chunk extends past the maximum key.** A closed upper bound is
exactly how a load loses its last row — one of RFC 0156's planted controls. `plan_chunks` has a test
asserting every key in the range lands in exactly one chunk.

**"Passed" and "would have caught it" are different claims, and the CLI says so.** `validate` prints
*"no planted controls were run, so this says the tiers found nothing — not that they would have."*
The tiers are built to run controls; this path does not run them yet, and the output states the
difference rather than letting a green line imply more than it proved.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/crates/migrate-target-clickhouse/src/classify.rs` | New — `StatementClass`, five rules, AST-based credential refusal, 13 tests |
| `ekos/crates/migrate-target-clickhouse/src/execute.rs` | New — `Environment`, `Artifact`, `Approval`, `authorize`, `plan_chunks`, `chunk_insert`, 14 tests |
| `ekos/crates/migrate-target-clickhouse/src/ddl.rs` | Codecs and partitioning no longer emitted; `UnverifiablePartitioning`; the emitter/classifier invariant test |
| `ekos/crates/cli/src/commands/migrate.rs` | `load`, `validate`, `ChClient` (+ `EngineReader` impl), `generate_ddl_for`, `column_rule_for` |
| `ekos/crates/cli/src/app.rs` | `ekos migrate load` and `ekos migrate validate` |
| `ekos/crates/compiler-core/src/config.rs` | `source_named_collection` |
| `docker/clickhouse/named-collections.xml`, `ekos-user.xml` | New — the source credential, server-side |
| `docker-compose.migrate.yml` | Mounts both |
| `TODO.md` | RFC 0160 core ticked; dialect-parser extension and approval wiring recorded |
