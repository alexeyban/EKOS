-- RFC 0252 §3.5 — the SQL Server fixture EKOS's live SQL Server source is tested against.
-- Small on purpose: two schemas, tables with keys and foreign keys, comments (MS_Description),
-- a check, defaults, an identity, a view, a procedure, a function, a trigger, and rows with nulls.
-- Run by docker-compose.sqlserver.yml's init service; safe to run again (drops and recreates).
IF DB_ID(N'shop') IS NOT NULL
BEGIN
    ALTER DATABASE shop SET SINGLE_USER WITH ROLLBACK IMMEDIATE;
    DROP DATABASE shop;
END
GO
CREATE DATABASE shop;
GO
USE shop;
GO
CREATE SCHEMA sales;
GO
CREATE SCHEMA hr;
GO
CREATE TABLE sales.customers (
    id          int IDENTITY(1, 1) NOT NULL CONSTRAINT PK_customers PRIMARY KEY,
    name        nvarchar(100) NOT NULL,
    email       varchar(200) NULL,
    birth_date  date NULL,
    created_at  datetime2(3) NOT NULL CONSTRAINT DF_customers_created DEFAULT (SYSUTCDATETIME()),
    CONSTRAINT UQ_customers_email UNIQUE (email)
);
CREATE TABLE sales.products (
    sku         char(8) NOT NULL CONSTRAINT PK_products PRIMARY KEY,
    title       nvarchar(200) NOT NULL,
    price       decimal(10, 2) NOT NULL CONSTRAINT CK_products_price CHECK (price >= 0),
    discontinued bit NOT NULL CONSTRAINT DF_products_discontinued DEFAULT (0)
);
CREATE TABLE sales.orders (
    id          bigint IDENTITY(1, 1) NOT NULL CONSTRAINT PK_orders PRIMARY KEY,
    customer_id int NULL CONSTRAINT FK_orders_customers REFERENCES sales.customers (id),
    status      tinyint NOT NULL CONSTRAINT DF_orders_status DEFAULT (1),
    placed_at   datetime2(0) NOT NULL,
    note        nvarchar(max) NULL
);
CREATE TABLE sales.order_lines (
    order_id    bigint NOT NULL,
    line_no     smallint NOT NULL,
    sku         char(8) NOT NULL,
    quantity    int NOT NULL,
    CONSTRAINT PK_order_lines PRIMARY KEY (order_id, line_no),
    CONSTRAINT FK_order_lines_orders FOREIGN KEY (order_id) REFERENCES sales.orders (id)
        ON DELETE CASCADE,
    CONSTRAINT FK_order_lines_products FOREIGN KEY (sku) REFERENCES sales.products (sku)
);
CREATE TABLE sales.order_log (
    id          int IDENTITY(1, 1) NOT NULL CONSTRAINT PK_order_log PRIMARY KEY,
    order_id    bigint NOT NULL,
    logged_at   datetime2(3) NOT NULL CONSTRAINT DF_order_log_at DEFAULT (SYSUTCDATETIME())
);
CREATE INDEX IX_orders_customer ON sales.orders (customer_id);
CREATE TABLE hr.employees (
    id          int NOT NULL CONSTRAINT PK_employees PRIMARY KEY,
    full_name   nvarchar(100) NOT NULL,
    manager_id  int NULL CONSTRAINT FK_employees_manager REFERENCES hr.employees (id),
    salary      money NULL
);
GO
EXEC sp_addextendedproperty N'MS_Description', N'People and companies who buy from the shop',
    N'SCHEMA', N'sales', N'TABLE', N'customers';
EXEC sp_addextendedproperty N'MS_Description', N'Shown on invoices',
    N'SCHEMA', N'sales', N'TABLE', N'customers', N'COLUMN', N'name';
EXEC sp_addextendedproperty N'MS_Description', N'1 = open, 2 = paid, 3 = shipped, 9 = cancelled',
    N'SCHEMA', N'sales', N'TABLE', N'orders', N'COLUMN', N'status';
GO
CREATE VIEW sales.open_orders AS
    SELECT o.id, o.customer_id, o.placed_at
    FROM sales.orders o
    WHERE o.status = 1;
GO
CREATE FUNCTION sales.order_total (@order_id bigint)
RETURNS decimal(12, 2)
AS
BEGIN
    RETURN (
        SELECT SUM(l.quantity * p.price)
        FROM sales.order_lines l JOIN sales.products p ON p.sku = l.sku
        WHERE l.order_id = @order_id
    );
END;
GO
CREATE PROCEDURE sales.cancel_order @order_id bigint
AS
BEGIN
    SET NOCOUNT ON;
    UPDATE sales.orders SET status = 9 WHERE id = @order_id;
    DELETE FROM sales.order_lines WHERE order_id = @order_id;
END;
GO
EXEC sp_addextendedproperty N'MS_Description', N'Cancels an order and drops its lines',
    N'SCHEMA', N'sales', N'PROCEDURE', N'cancel_order';
GO
CREATE TRIGGER sales.orders_log ON sales.orders AFTER INSERT AS
BEGIN
    SET NOCOUNT ON;
    INSERT INTO sales.order_log (order_id) SELECT id FROM inserted;
END;
GO
INSERT INTO sales.customers (name, email, birth_date) VALUES
    (N'Ada', 'ada@example.org', '1990-05-01'),
    (N'Brook', NULL, NULL),
    (N'Chen', 'chen@example.org', '1985-11-23');
INSERT INTO sales.products (sku, title, price) VALUES
    ('SKU00001', N'Kettle', 39.90),
    ('SKU00002', N'Teapot', 24.50),
    ('SKU00003', N'Cup', 6.00);
INSERT INTO sales.orders (customer_id, status, placed_at, note) VALUES
    (1, 1, '2026-01-05 10:00:00', NULL),
    (1, 2, '2026-02-11 09:30:00', N'gift wrap'),
    (3, 3, '2026-03-20 16:45:00', NULL),
    (NULL, 9, '2026-04-02 12:00:00', NULL);
INSERT INTO sales.order_lines (order_id, line_no, sku, quantity) VALUES
    (1, 1, 'SKU00001', 1), (1, 2, 'SKU00003', 4), (2, 1, 'SKU00002', 1), (3, 1, 'SKU00003', 2);
INSERT INTO hr.employees (id, full_name, manager_id, salary) VALUES
    (1, N'Dana', NULL, 9000), (2, N'Eli', 1, 6500);
GO
-- The login EKOS reads with: data reader plus VIEW DEFINITION (module text), nothing else.
IF SUSER_ID(N'ekos_ro') IS NULL
    CREATE LOGIN ekos_ro WITH PASSWORD = N'Ekos-ro-local-only1', CHECK_POLICY = OFF;
GO
CREATE USER ekos_ro FOR LOGIN ekos_ro;
ALTER ROLE db_datareader ADD MEMBER ekos_ro;
GRANT VIEW DEFINITION TO ekos_ro;
GO
