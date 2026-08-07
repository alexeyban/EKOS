# DB Scripts/Destination MySQL/create.staging_data_management_mmjja.sql (File)

## Properties

| Key | Value |
|---|---|
| `artifact_id` | 7892e72a3377610a2ed8a4334734e885dd88b7c2c9a5d02349a30ac927a64eb5 |
| `excerpt` | drop schema if exists staging_data_management_mmjja; 
create schema staging_data_management_mmjja;
use staging_data_management_mmjja;

# Create staging date table 
drop table if exists staging_date;

CREATE TABLE staging_date
(
    date_id INT NOT NULL,
    date DATE,    
    day_of_the_week_number INT,    
    day_of_the_week_text VARCHAR(10),
    day_of_the_month_number INT,    
    day_of_the_month_text VARCHAR(10),
    month_number INT,    
    month_text VARCHAR(10),    
    day_of_the_year INT,    
    week_of_the_year INT,
    quarter_number INT,
    year_number INT,
    PRIMARY KEY(dat |
| `path` | DB Scripts/Destination MySQL/create.staging_data_management_mmjja.sql |
| `size_bytes` | 3462 |

## Relationships

_No compiled relationships touch this object._

## Diagram

_No relationships to diagram._

## Evidence

- `ddd98016-06a2-54ab-909d-c4aaa260aab8` — file: DB Scripts/Destination MySQL/create.staging_data_management_mmjja.sql (3462 bytes) (confidence: 1.00)
