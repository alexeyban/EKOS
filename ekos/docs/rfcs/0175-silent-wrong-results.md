# RFC 0175 — Four places that returned a wrong answer without saying so

| | |
|---|---|
| **Status** | Implemented (2026-10-11) |
| **Depends on** | RFC 0010/0096 (EKL), RFC 0027 (Transformation IR), RFC 0158 (DQ rules, inferred keys), RFC 0032 (treasury) |

## Problem

Four open items in `TODO.md` have the same shape: the command succeeds and the output is wrong or short, with nothing to say so.

1. **EKL.** `FIND Object WHERE status = 'confirmed'` returns 0 rows. EKL rows have five columns (`id`, `name`, `kind`, `evidence`, `created_at`); any other field matches nothing. "No rows" reads as "no such objects".
2. **Inferred keys.** `ekos migrate` reads join predicates from the Transformation IR to propose undeclared foreign keys. On LedgerSMB it proposed `country.country_id → entity.id`. The IR keeps a join key as the two sides of `a = b` in the order they were written, and the harvest assigned the first to the join's left operand and the second to the right. Written order is not operand order, the left operand of a chained join is another join, and the alias that says which table a column belongs to was not kept.
3. **`DQ.UNIQ.001`.** The rule measures duplicates in key-shaped column names (`*_id`, `*_key`, `*_code`) and blocks on any. `acc_trans.trans_id` is a foreign key: it repeats by design.
4. **Treasury.** `RealTreasuryClient` sends one request per explorer action. Etherscan-family explorers return at most 10,000 rows per request, so later payments of a large treasury never appear, and a missing payment cannot be matched to an approval or reported as lacking one.

## Design

### 1. EKL reads properties, and an unknown field is an error

- A field that is not a built-in column is read from the row's `properties`: `WHERE`, `RETURN`, `ORDER BY` and `GROUP BY`, for objects and relationships. A built-in column wins over a property of the same name.
- `a.b` is one field, a path into a nested object property. A key that itself contains a dot wins over the path reading.
- Only the properties a query names are copied into its rows.
- A `WHERE` / `ORDER BY` / `GROUP BY` field that is not built-in and that **no** candidate row has is `EklError::UnknownField`, naming the built-in columns. With no candidates there is nothing to check, and the result is empty.
- `RETURN` of a field some rows lack stays `null`.

The check is per query, not against a schema: properties are open, so "known" means "some candidate has it". A misspelt field fails; a property that only objects of another kind carry fails too, which is the case the TODO item described.

### 2. A join key belongs to the table its qualifier names

- `TransformNode::Source` gains `alias: Option<String>`, set by the SQL lowering from `FROM entity e`. It is left out of the serialized form when absent, so older artifacts still read. `sql-transform` goes to `v3`.
- The harvest builds one scope per graph: each table under its own name and under its alias. An alias that names two different tables in one graph is ambiguous.
- Each side of a key is attributed on its own: a qualified column to the table its qualifier resolves to; an unqualified column to the operand on that side, only when that operand is a single table (not another join).
- A side that cannot be attributed drops the observation. A missing candidate is a gap; a wrong one looks derived.

### 3. `DQ.UNIQ.001` skips referencing columns

`ColumnContext` gains `references_another_table`. `migrate assess` sets it from `pg_constraint` (`contype = 'f'`, every column of the constraint). The rule does not apply to such a column. Inferred keys are candidates until measured, so they do not suppress the rule.

### 4. The treasury client pages by block

`collect_pages` requests `page=1&offset=10000` ascending. A full page means more may follow, and the next request starts **at** the last block seen, because the cap can cut a block in half. Rows of that block already returned are skipped by count, so two identical transfers in one transaction both survive.

Two histories cannot be read whole and are errors, never a short list: one block holding a full page of rows, and more than 500 pages. Requests after the first wait 250 ms.

## Not in this RFC

- Running the client against a live explorer. It needs an API key; the pager is tested against a capped fake.
- EKL property filters pushed down to the store. Rows are still filtered after a full scan, as before.

## Tests

- EKL: property filter, nested path, `GROUP BY` a property, relationship property, unknown field is an error, unknown field over no candidates is empty, built-in wins.
- Harvest: qualifier wins over operand, schema-qualified and quoted names, ambiguous alias, unknown alias, expression, unqualified with no single table, one table under two aliases.
- `DQ.UNIQ.001`: fires on a profiled key-shaped column, not on the same column when it references another table.
- Pager: history longer than the cap with a block across the boundary and identical rows; short and empty histories in one request; an unreadable block and a page limit are errors.
