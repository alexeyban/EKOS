# Create schemas
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
    dim_customer_id INT,
    dim_sales_territory_id INT,
    dim_sales_person_id INT,
    dim_product_id INT,
    order_qty INT,
    product_unit_price DECIMAL(19, 2),
    product_unit_price_discount DECIMAL(19, 2),
    total_amount DECIMAL(19, 2),
    total_discount DECIMAL(19, 2),
    sales_order_detail_id INT,
    sales_order_id INT,
    sales_order_number VARCHAR(25),
    product_standard_cost_on_day DECIMAL(19, 2),
    ship_date_id INT,
    ship_date_datetime DATETIME,
    sales_profit DECIMAL(19, 2),
    PRIMARY KEY(fact_sales_id)
);

CREATE TABLE dim_product
(
    dim_product_id INT NOT NULL,
    product_id INT,
    product_number VARCHAR(50),
    product_name VARCHAR(100),
    make_flag TINYINT(1),
    finished_goods_flag TINYINT(1),
    color VARCHAR(15),
    size VARCHAR(5),
    size_unit_measure_code VARCHAR(3),    
    weight DECIMAL(19, 2),
    weight_unit_measure_code VARCHAR(3),    
    product_line_code VARCHAR(2),
    product_line_description VARCHAR(100),
    class_code VARCHAR(2),
    style VARCHAR(2),
    product_subcategory_id INT,
    product_subcategory_name VARCHAR(100),
    product_category_id INT,
    product_category_name VARCHAR(100),
    product_model_id INT,
    product_model_name VARCHAR(100),
    version_number INT,
    start_date DATETIME,
    end_date DATETIME,        
    PRIMARY KEY(dim_product_id)
);

CREATE TABLE dim_customer
(
    dim_customer_id INT NOT NULL,
    customer_id INT,
    is_reseller TINYINT(1),
    reseller_store_id INT,
    reseller_store_name VARCHAR(50),
    person_title VARCHAR(8),
    person_first_name VARCHAR(50),
    person_middle_name VARCHAR(50),
    person_last_name VARCHAR(50),
    person_suffix VARCHAR(10),
    version_number INT,
    start_date DATETIME,
    end_date DATETIME,    
    PRIMARY KEY(dim_customer_id)
);

CREATE TABLE dim_sales_territory
(
    dim_sales_territory_id INT NOT NULL,
    territory_id INT,
    sales_territory_name VARCHAR(100),
    country_region_code VARCHAR(3),
    group_name VARCHAR(100),
    PRIMARY KEY(dim_sales_territory_id)
);

CREATE TABLE dim_date
(
    dim_date_id INT NOT NULL,
    date DATE,
    is_holiday TINYINT(1),
    holiday_name VARCHAR(100),
    day_of_the_week_number INT,
    day_of_the_week_text VARCHAR(10),
    day_of_the_month_number INT,
    day_of_the_month_text VARCHAR(10),
    month_number INT,
    month_text VARCHAR(10),
    day_of_the_year INT,
    northern_hemisphere_season VARCHAR(10),
    southern_hemisphere_season VARCHAR(10),
    week_of_the_year INT,
    quarter_number INT,
    year_number INT,
    PRIMARY KEY(dim_date_id)
);

CREATE TABLE dim_sales_person
(
    dim_sales_person_id INT NOT NULL,
    sales_person_entity_id INT,
    person_title VARCHAR(8),
    person_first_name VARCHAR(50),
    person_middle_name VARCHAR(50),
    person_last_name VARCHAR(50),
    person_suffix VARCHAR(10),
    employee_job_title VARCHAR(50),
    employee_gender_code VARCHAR(1),
    employee_hire_date DATE,
    employee_marital_status VARCHAR(1),
    employee_national_id_number VARCHAR(15),
    PRIMARY KEY(dim_sales_person_id)
);

CREATE TABLE fact_purchases
(
    fact_purchases_id INT NOT NULL AUTO_INCREMENT,
    order_date_id INT,
	order_date_datetime DATETIME,
    ship_date_id INT,
	ship_date_datetime DATETIME,
    dim_vendor_id INT,
    dim_product_id INT,
    product_unit_price DECIMAL(19, 2),
    order_qty INT,
    order_amount DECIMAL(19, 2),
    rejected_qty INT,
    received_qty INT,
    stocked_qty INT,
    rejected_amount DECIMAL(19, 2),
    received_amount DECIMAL(19, 2),
    stocked_amount DECIMAL(19, 2),
    received_flag TINYINT(1),
    purchase_order_detail_id INT,
    purchase_order_id INT,
    PRIMARY KEY(fact_purchases_id)
);

CREATE TABLE dim_vendor
(
    dim_vendor_id INT NOT NULL,
    vendor_id INT,
    account_number VARCHAR(15),
    name VARCHAR(50),
    credit_rating_id INT,
    preferred_vendor TINYINT(1),
    version_number INT,
    start_date DATETIME,
    end_date DATETIME,  
    PRIMARY KEY(dim_vendor_id)
);


/* 
Foreign Keys: out of Scope for this project
# Create FKs
    
ALTER TABLE fact_sales
    ADD    FOREIGN KEY (order_date_id)
    REFERENCES dim_date(dim_date_id)
;
    
ALTER TABLE fact_sales
    ADD    FOREIGN KEY (dim_product_id)
    REFERENCES dim_product(dim_product_id)
;
    
ALTER TABLE fact_sales
    ADD    FOREIGN KEY (dim_customer_id)
    REFERENCES dim_customer(dim_customer_id)
;
    
ALTER TABLE fact_sales
    ADD    FOREIGN KEY (dim_sales_person_id)
    REFERENCES dim_sales_person(dim_sales_person_id)
;
    
ALTER TABLE fact_sales
    ADD    FOREIGN KEY (dim_sales_territory_id)
    REFERENCES dim_sales_territory(dim_sales_territory_id)
;
    
ALTER TABLE fact_sales
    ADD    FOREIGN KEY (ship_date_id)
    REFERENCES dim_date(dim_date_id)
;
    
ALTER TABLE fact_purchases
    ADD    FOREIGN KEY (dim_product_id)
    REFERENCES dim_product(dim_product_id)
;
    
ALTER TABLE fact_purchases
    ADD    FOREIGN KEY (due_date_id)
    REFERENCES dim_date(dim_date_id)
;
    
ALTER TABLE fact_purchases
    ADD    FOREIGN KEY (dim_vendor_id)
    REFERENCES dim_vendor(dim_vendor_id)
;
    
*/ 
