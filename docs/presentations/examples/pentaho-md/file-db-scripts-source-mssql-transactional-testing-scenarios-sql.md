# DB Scripts/Source MSSQL/transactional.testing.scenarios.sql (File)

## Properties

| Key | Value |
|---|---|
| `artifact_id` | a680bce801e0022f376f9b23576352d91eaff1c6677204451d572c68db000149 |
| `excerpt` | /*
Escenario 1
0. Run dim_product transformation
1. Check dim_product of product id 875 at the datamart
2. Check product of product id 875 at the transactional Adventureworks
3. Update product name from product id 875 at the transactional Adventureworks
4. Check dim_product of product id 875 at the datamart, new version should've been created
*/
select p.ProductID, p.Name,
pc.Name as CategoryName, ps.Name as SubCategoryName
from Production.Product p
join Production.ProductSubcategory ps 
	on p.ProductSubcategoryID = ps.ProductSubcategoryID
join Production.ProductCategory pc 
	on ps.ProductCate |
| `path` | DB Scripts/Source MSSQL/transactional.testing.scenarios.sql |
| `size_bytes` | 3491 |

## Relationships

_No compiled relationships touch this object._

## Diagram

_No relationships to diagram._

## Evidence

- `a60022c9-8daf-566b-bfec-a44358eb7b70` — file: DB Scripts/Source MSSQL/transactional.testing.scenarios.sql (3491 bytes) (confidence: 1.00)
