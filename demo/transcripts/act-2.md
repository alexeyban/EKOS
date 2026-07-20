# Act 2 transcript

**Prompt:** Use the estate-scout agent: find every database table related to orders across my estate, and show me what one of them is connected to.

```
The estate-scout came back with results. Note: all of these are test fixtures, not production tables.

**Order-related tables found (all in `EKOS/tests/fixtures/`, not real production schemas):**

| Table | File | Notes |
|---|---|---|
| `orders` | `ecommerce.sql` | generic e-commerce fixture |
| `order_items` | `ecommerce.sql` | line items for the above |
| `Orders` | `northwind.sql` | classic Northwind demo schema |
| `"Order Details"` | `northwind.sql` | Northwind line items |

These are sample/test data used for exercising EKOS itself — the ledger didn't surface any orders tables from your other real projects (DeFi Sentinel, azure-data-project, etc.), so either none exist there or they haven't been ingested.

**Connections of `Orders` (Northwind)** — the most connected one, 4 direct FKs:

- **"Order Details"** ← `OrderID` (line items: product, price, qty, discount)
- **Customers** ← `CustomerID` (company/contact/address info)
- **Employees** ← `EmployeeID` (with self-referential `ReportsTo` hierarchy, and a link to `EmployeeTerritories`)
- **Shippers** ← `ShipVia`

Transitively, through "Order Details" it also reaches **Products**, and through Customers it reaches **CustomerCustomerDemo**. Classic star-schema shape: Orders as fact table, Customers/Employees/Shippers/Products as dimensions.

Want me to check if any of your other repos (azure-data-project, adf-pipelines, cdc-lab) have real order tables that just haven't been indexed into EKOS yet?
```
