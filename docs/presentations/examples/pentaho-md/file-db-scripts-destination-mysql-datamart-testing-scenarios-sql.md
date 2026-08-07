# DB Scripts/Destination MySQL/datamart.testing.scenarios.sql (File)

## Properties

| Key | Value |
|---|---|
| `artifact_id` | de6710e261f6e77d48955d42a8d9518fe03e2356713bdbf267cb078e308c8d84 |
| `excerpt` | /*
Escenario 1
0. Run dim_product transformation
1. Check dim_product of product id 875 at the datamart
2. Check product of product id 875 at the transactional Adventureworks
3. Update product name from product id 875 at the transactional Adventureworks
4. Check dim_product of product id 875 at the datamart, new version should've been created
*/

select  dim_product_id, product_id, 
product_name, product_category_name, product_subcategory_name, 
version_number, start_date, end_date
from dim_product
where product_id = 875


/* 
Escenario 2
0. Run first fact sales load
1. Check fact sales row a  |
| `path` | DB Scripts/Destination MySQL/datamart.testing.scenarios.sql |
| `size_bytes` | 1855 |

## Relationships

_No compiled relationships touch this object._

## Diagram

_No relationships to diagram._

## Evidence

- `325407ff-15ed-55ab-b2eb-47ea99ca4b6d` — file: DB Scripts/Destination MySQL/datamart.testing.scenarios.sql (1855 bytes) (confidence: 1.00)
