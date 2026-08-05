# Act 3 transcript

**Prompt:** Ask the impact-analyst: what breaks if I rename the customers table?

```
## Blast radius of renaming `customers`

The ledger actually has **three separate `customers`-like tables**, unlinked by any cross-system identity match — so nothing cascades between systems.

**ecommerce `customers`** (most likely the one you mean):
- Direct: `orders.customer_id → customers.id`
- Transitive: `payments` and `order_items` (via `orders`)
- No views, ETL jobs, stored procs, or app code reference it in the ledger — **low risk**, one FK to update.

**Northwind `Customers`** (exists as **two unmerged duplicate objects** from repeated file scans):
- Direct: `CustomerCustomerDemo`, `Orders`
- Transitive: `"Order Details"` (via `Orders`)
- Same low-risk shape, but a rename needs to be applied against both duplicate object IDs, or the duplication should be resolved first.

One noise source to ignore: hits for "customer" in `dbt/` and `databricks-lab/` files belong to an unrelated project and have zero actual relationship edges — text matches, not dependencies.

**Bottom line:** renaming is low-to-medium risk — just FK references within one schema, nothing cross-system. Separately, the Northwind duplicate-object issue is worth a dedup pass regardless of what you do with the rename.
```
