# Act 2 transcript

**Prompt:** Use the estate-scout agent: find every database table related to orders across my estate, and show me what one of them is connected to.

```
Found 6 order-related tables in the ledger, all coming from test/demo fixtures rather than your real production estate:

| Table | Source | Notes |
|---|---|---|
| `orders` | `tests/fixtures/ecommerce.sql` | Central order header (status, totals, fulfillment timestamps) |
| `order_items` | `tests/fixtures/ecommerce.sql` | Order line items |
| `Orders` | `tests/fixtures/northwind.sql` | Northwind-style, with shipping/employee/shipper fields (2 compiled instances) |
| `"Order Details"` | `tests/fixtures/northwind.sql` | Northwind line items w/ discount (2 compiled instances) |

**Neighborhood of the `orders` table (ecommerce.sql, id `69f1afa9-505f-4ed0-99f0-a6c077076cbd`):**

- **Upstream:** `customers` — via `orders.customer_id → customers.id`
- **Downstream (1 hop):**
  - `order_items` — via `order_items.order_id → orders.id`
  - `payments` — via `payments.order_id → orders.id`
- **Downstream (2 hops):**
  - `products` — via `order_items.product_id → products.id`

So it's a standard e-commerce shape: `customers → orders → order_items → products`, with `payments` hanging off `orders` directly. All 4 FK relationships carry evidence IDs traceable back to the source SQL DDL.

One thing worth flagging: everything found lives under `tests/fixtures/`, not a real production system — so this is demo/fixture data in the ledger, not your actual estate's order tables. If you have real order tables elsewhere (a production Postgres, Databricks, etc.) that haven't been observed/compiled into EKOS yet, they won't show up here.
```
