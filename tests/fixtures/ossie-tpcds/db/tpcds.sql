-- A subset of the TPC-DS retail schema: the five tables Apache Ossie's official
-- `tpcds_semantic_model.yaml` example describes, with the column names and types of the
-- TPC-DS specification. Written for EKOS's Ossie end-to-end test (RFC 0245); no TPC tooling
-- or generated data is included.

CREATE TABLE date_dim (
    d_date_sk        integer      NOT NULL,
    d_date_id        char(16)     NOT NULL,
    d_date           date,
    d_year           integer,
    d_moy            integer,
    d_quarter_name   char(6),
    PRIMARY KEY (d_date_sk)
);

CREATE TABLE customer (
    c_customer_sk           integer      NOT NULL,
    c_customer_id           char(16)     NOT NULL,
    c_first_name            char(20),
    c_last_name             char(30),
    c_preferred_cust_flag   char(1)      CHECK (c_preferred_cust_flag IN ('Y', 'N')),
    c_email_address         char(50),
    PRIMARY KEY (c_customer_sk),
    UNIQUE (c_customer_id)
);
COMMENT ON COLUMN customer.c_preferred_cust_flag IS 'Y = preferred customer, N = not preferred';

CREATE TABLE item (
    i_item_sk         integer        NOT NULL,
    i_item_id         char(16)       NOT NULL,
    i_item_desc       varchar(200),
    i_current_price   decimal(7,2),
    i_brand           char(50),
    i_category        char(50),
    PRIMARY KEY (i_item_sk)
);

CREATE TABLE store (
    s_store_sk           integer      NOT NULL,
    s_store_id           char(16)     NOT NULL,
    s_store_name         varchar(50),
    s_number_employees   integer,
    s_city               varchar(60),
    s_state              char(2),
    PRIMARY KEY (s_store_sk)
);

CREATE TABLE store_sales (
    ss_sold_date_sk      integer       REFERENCES date_dim (d_date_sk),
    ss_item_sk           integer       NOT NULL REFERENCES item (i_item_sk),
    ss_customer_sk       integer       REFERENCES customer (c_customer_sk),
    ss_store_sk          integer       REFERENCES store (s_store_sk),
    ss_ticket_number     integer       NOT NULL,
    ss_quantity          integer,
    ss_sales_price       decimal(7,2),
    ss_ext_sales_price   decimal(7,2),
    ss_net_profit        decimal(7,2),
    PRIMARY KEY (ss_item_sk, ss_ticket_number)
);

-- Preferred customers: a filter whose literal EKOS records as a business concept.
CREATE VIEW preferred_customers AS
    SELECT * FROM customer WHERE c_preferred_cust_flag = 'Y';
