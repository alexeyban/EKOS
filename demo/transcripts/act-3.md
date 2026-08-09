# Act 3 transcript

**Prompt:** Ask the impact-analyst: what breaks if I rename the customers table?

```
## Blast radius: renaming `customers`

**Direct impact (1 table):**
- `orders` — has FK `orders.customer_id → customers.id`, breaks immediately unless rename is propagated.

**Transitive impact (2 more tables, via `orders`):**
- `payments` (joins through `orders.id`)
- `order_items` (joins through `orders.id`)

Traversal terminated cleanly — no further hops.

**Nothing else in the ledger references this table:** no dbt models, views, SQL transformations, application code, or cross-system identity matches point at it. That's either genuinely all there is, or those paths haven't been scanned.

**Flags to verify before you rename:**
- There are two other, unrelated `Customers` tables in the ledger (Northwind schema, different columns) — not linked to this one and not affected, but worth double-checking you mean the ecommerce `customers` (id `c2fde5fc…`) and not one of those.
- Several files mentioning "customer" (`dbt/models/quarantine/quarantine_customer_no_payments.sql`, `dbt/models/gold/gold_store_revenue.sql`, `databricks-lab/ingestion/load_bulk_data.py`, `scripts/load_bulk_data.py`) showed up in text search but have **no graph relationship** to this table — the agent couldn't confirm or rule out a hard-coded reference. Worth a manual grep before you commit to the rename.

**Bottom line:** narrow, well-contained rename as far as the ledger can see — update the FK on `orders`, re-check `payments`/`order_items`, and manually verify the unlinked dbt/script files aren't silently hard-coding the old name.
```
