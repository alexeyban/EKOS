# DB Scripts/Destination MySQL/init.staging_data_management_mmjja.sql (File)

## Properties

| Key | Value |
|---|---|
| `artifact_id` | 475bc80f0fa309446628d44d80e683ba601821971ed3fe6951507d577564daae |
| `excerpt` | use staging_data_management_mmjja;

-- CALL fill_staging_date_table('1970-01-01','2050-12-31'); 
-- If it is too broad, the date transformation will take a while to setup its holidays. 
-- In reality, adventureworks Sales & Purchases, only contain data from 2011 - 2013

-- truncate staging_date;  -- in case you don't want to recreate the schema, just truncate the staging date table

CALL fill_staging_date_table('2009-01-01','2015-12-31');
 |
| `path` | DB Scripts/Destination MySQL/init.staging_data_management_mmjja.sql |
| `size_bytes` | 443 |

## Relationships

_No compiled relationships touch this object._

## Diagram

_No relationships to diagram._

## Evidence

- `06890dfe-9848-593a-a50e-a8ceb8fb18a0` — file: DB Scripts/Destination MySQL/init.staging_data_management_mmjja.sql (443 bytes) (confidence: 1.00)
