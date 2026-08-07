# DB Scripts/Destination MySQL/create.eae_data_management_mmjja.sql (File)

## Properties

| Key | Value |
|---|---|
| `artifact_id` | 26eb3f5888485a8a10f6ab8bf3b4879013aa5a1cb88d86b65977f1343c3ef312 |
| `excerpt` | # Create schemas
drop schema if exists eae_data_management_mmjja; 
create schema eae_data_management_mmjja;
use eae_data_management_mmjja;

# Drop tables
drop table IF EXISTS fact_sales;
drop table IF EXISTS fact_purchases;
drop table IF EXISTS dim_date;
drop table IF EXISTS dim_product;
drop table IF EXISTS dim_customer;
drop table IF EXISTS dim_sales_territory;
drop table IF EXISTS dim_sales_person;
drop table IF EXISTS dim_vendor;

# Create tables
CREATE TABLE fact_sales
(
    fact_sales_id INT NOT NULL AUTO_INCREMENT,
    order_date_id INT,
    order_date_datetime DATETIME,
    dim_custome |
| `path` | DB Scripts/Destination MySQL/create.eae_data_management_mmjja.sql |
| `size_bytes` | 5716 |

## Relationships

_No compiled relationships touch this object._

## Diagram

_No relationships to diagram._

## Evidence

- `5112ec2d-562a-5cc9-a3f7-38fbd0fa748f` — file: DB Scripts/Destination MySQL/create.eae_data_management_mmjja.sql (5716 bytes) (confidence: 1.00)
