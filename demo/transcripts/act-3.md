# Act 3 transcript

**Prompt:** Ask the impact-analyst: what breaks if I rename the customers table?

```
The impact-analyst found three "customers"-named tables in EKOS, but all are in **test fixtures**, not a real production project:

| Table | Location | Direct dependents |
|---|---|---|
| `customers` (ecommerce) | `EKOS/tests/fixtures/ecommerce.sql` | `orders.customer_id → customers.id` FK |
| `Customers` (Northwind) | `EKOS/tests/fixtures/northwind.sql` | `Orders.CustomerID` and `CustomerCustomerDemo.CustomerID` FKs |
| `Customers` (Northwind, dup) | same file, likely a duplicate ledger entry from a second scan | same dependents as above |

No dbt models, DAGs, dashboards, or application code reference any of these — the agent checked and ruled out some similarly-named `customer` entities in `azure-databricks-project` (dbt models) since there's no FK/reference evidence linking them.

**Bottom line:** if you're asking about a real production `customers` table (not EKOS's test fixtures), it hasn't been ingested into the ledger yet — worth running `ekos_status` to check which projects are indexed before trusting an impact report. If you did mean one of the fixture tables, renaming just requires updating the FK column(s) in `orders`/`Orders`/`CustomerCustomerDemo` to match.
```
