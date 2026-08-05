# Act 2 transcript

**Prompt:** Use the estate-scout agent: find every database table related to orders across my estate, and show me what one of them is connected to.

```
## Order-related tables found

**E-commerce fixture** (`EKOS/tests/fixtures/ecommerce.sql`)
- `orders`, `order_items`

**Northwind fixture** (`EKOS/tests/fixtures/northwind.sql`)
- `Orders` (×2 — appears twice, likely from repeated compilation), `"Order Details"` (×2)

## Neighborhood of `orders` (e-commerce)

```
customers (parent)
    ↓ FK customer_id
  orders  ←────────────── central table
    ├─ order_items  ← FK order_id
    │     └─ products  ← FK product_id (via order_items)
    └─ payments     ← FK order_id
```

- `orders.customer_id → customers.id`
- `order_items.order_id → orders.id`
- `order_items.product_id → products.id`
- `payments.order_id → orders.id`

Every edge is backed by a `ForeignKey` relationship in the ledger with cited fragment evidence from `ecommerce.sql`. `orders` sits at the hub of the e-commerce domain — tying customer identity, line-item/product detail, and payment records together.

One caveat: the found tables are all from test fixtures (`tests/fixtures/ecommerce.sql`, `tests/fixtures/northwind.sql`), not a production estate — if you have real production databases compiled into the ledger, let me know which project/workspace and I can re-scope the search.
```
