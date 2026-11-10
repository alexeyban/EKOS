-- RFC 0252 §3.5 — the Informix fixture EKOS's live Informix source is tested against.
-- Small on purpose: two owners, tables with keys, a unique index, foreign keys (one cascading,
-- one self-reference), a check, defaults, serials, a view, an SPL procedure with a DOCUMENT
-- clause, an SPL function, a trigger, and rows with nulls. Run by docker-compose.informix.yml's
-- fixture step with DELIMIDENT=y (owners are quoted); safe to run again (drops and recreates).
DROP DATABASE IF EXISTS shop;
CREATE DATABASE shop WITH LOG;

CREATE TABLE "sales".customers (
    id          SERIAL NOT NULL,
    name        VARCHAR(100) NOT NULL,
    email       VARCHAR(200),
    birth_date  DATE,
    created_at  DATETIME YEAR TO SECOND DEFAULT CURRENT YEAR TO SECOND NOT NULL,
    PRIMARY KEY (id) CONSTRAINT pk_customers,
    UNIQUE (email) CONSTRAINT uq_customers_email
);
CREATE TABLE "sales".products (
    sku          CHAR(8) NOT NULL,
    title        LVARCHAR(200) NOT NULL,
    price        DECIMAL(10, 2) NOT NULL,
    discontinued BOOLEAN DEFAULT 'f' NOT NULL,
    PRIMARY KEY (sku) CONSTRAINT pk_products,
    CHECK (price >= 0) CONSTRAINT ck_products_price
);
CREATE TABLE "sales".orders (
    id          SERIAL8 NOT NULL,
    customer_id INTEGER,
    status      SMALLINT DEFAULT 1 NOT NULL,
    placed_at   DATETIME YEAR TO SECOND NOT NULL,
    note        LVARCHAR(2000),
    PRIMARY KEY (id) CONSTRAINT pk_orders,
    -- A coded column: 1 open, 2 shipped, 9 closed (only `close_order` writes 9).
    CHECK (status IN (1, 2, 9)) CONSTRAINT ck_orders_status,
    FOREIGN KEY (customer_id) REFERENCES "sales".customers (id) CONSTRAINT fk_orders_customers
);
CREATE TABLE "sales".order_lines (
    order_id    INT8 NOT NULL,
    line_no     SMALLINT NOT NULL,
    sku         CHAR(8) NOT NULL,
    quantity    INTEGER NOT NULL,
    PRIMARY KEY (order_id, line_no) CONSTRAINT pk_order_lines,
    FOREIGN KEY (order_id) REFERENCES "sales".orders (id) ON DELETE CASCADE CONSTRAINT fk_lines_orders,
    FOREIGN KEY (sku) REFERENCES "sales".products (sku) CONSTRAINT fk_lines_products
);
CREATE TABLE "sales".order_log (
    id          SERIAL NOT NULL,
    order_id    INT8,
    logged_at   DATETIME YEAR TO SECOND DEFAULT CURRENT YEAR TO SECOND
);
CREATE UNIQUE INDEX "sales".ux_orders_ref ON "sales".orders (customer_id, id);
CREATE TABLE "hr".employees (
    id          INTEGER NOT NULL,
    full_name   VARCHAR(100) NOT NULL,
    manager_id  INTEGER,
    hired_on    DATE,
    PRIMARY KEY (id) CONSTRAINT pk_employees,
    FOREIGN KEY (manager_id) REFERENCES "hr".employees (id) CONSTRAINT fk_employees_manager
);

CREATE VIEW "sales".big_orders (id, customer_id, total) AS
    SELECT o.id, o.customer_id, SUM(l.quantity * p.price)
    FROM "sales".orders o, "sales".order_lines l, "sales".products p
    WHERE l.order_id = o.id AND p.sku = l.sku
    GROUP BY o.id, o.customer_id
    HAVING SUM(l.quantity * p.price) > 100;

CREATE PROCEDURE "sales".close_order(p_id INT8)
    UPDATE "sales".orders SET status = 9 WHERE id = p_id;
    INSERT INTO "sales".order_log (order_id) VALUES (p_id);
END PROCEDURE
DOCUMENT 'Closes one order and logs it';

CREATE FUNCTION "sales".order_total(p_id INT8) RETURNING DECIMAL(12, 2);
    DEFINE t DECIMAL(12, 2);
    SELECT SUM(l.quantity * p.price) INTO t
      FROM "sales".order_lines l, "sales".products p
     WHERE p.sku = l.sku AND l.order_id = p_id;
    RETURN t;
END FUNCTION;

CREATE TRIGGER "sales".orders_audit INSERT ON "sales".orders
    REFERENCING NEW AS n
    FOR EACH ROW (INSERT INTO "sales".order_log (order_id) VALUES (n.id));

INSERT INTO "sales".customers (id, name, email, birth_date)
    VALUES (0, 'Ada Fixture', 'ada.fixture@example.test', '1990-04-12');
INSERT INTO "sales".customers (id, name, email, birth_date)
    VALUES (0, 'Brook Fixture', NULL, NULL);
INSERT INTO "sales".customers (id, name, email, birth_date)
    VALUES (0, 'Cyd Fixture', 'cyd.fixture@example.test', '1985-11-02');
INSERT INTO "sales".products VALUES ('SKU-0001', 'Fixture Teapot Deluxe', 24.50, 'f');
INSERT INTO "sales".products VALUES ('SKU-0002', 'Fixture Kettle', 61.00, 'f');
INSERT INTO "sales".products VALUES ('SKU-0003', 'Fixture Cup', 3.75, 't');
INSERT INTO "sales".orders (id, customer_id, status, placed_at, note)
    VALUES (0, 1, 1, '2025-01-15 09:30:00', 'leave at the fixture door');
INSERT INTO "sales".orders (id, customer_id, status, placed_at, note)
    VALUES (0, 1, 2, '2025-03-02 14:00:00', NULL);
INSERT INTO "sales".orders (id, customer_id, status, placed_at, note)
    VALUES (0, 3, 1, '2026-06-30 18:45:10', NULL);
INSERT INTO "sales".orders (id, customer_id, status, placed_at, note)
    VALUES (0, NULL, 1, '2026-10-01 08:00:00', 'walk-in');
INSERT INTO "sales".order_lines VALUES (1, 1, 'SKU-0001', 2);
INSERT INTO "sales".order_lines VALUES (1, 2, 'SKU-0003', 4);
INSERT INTO "sales".order_lines VALUES (2, 1, 'SKU-0002', 3);
INSERT INTO "sales".order_lines VALUES (3, 1, 'SKU-0001', 1);
INSERT INTO "hr".employees VALUES (1, 'Dana Fixture', NULL, '2019-02-01');
INSERT INTO "hr".employees VALUES (2, 'Eli Fixture', 1, NULL);

UPDATE STATISTICS LOW FOR TABLE;

-- The read-only user: CONNECT and SELECT, nothing else. A table in a database without ANSI mode
-- grants every right to PUBLIC when it is created, so those grants are revoked first — as the
-- owner, who is their grantor.
REVOKE ALL ON "sales".customers FROM PUBLIC AS "sales";
REVOKE ALL ON "sales".products FROM PUBLIC AS "sales";
REVOKE ALL ON "sales".orders FROM PUBLIC AS "sales";
REVOKE ALL ON "sales".order_lines FROM PUBLIC AS "sales";
REVOKE ALL ON "sales".order_log FROM PUBLIC AS "sales";
REVOKE ALL ON "hr".employees FROM PUBLIC AS "hr";
GRANT CONNECT TO ekos_ro;
GRANT SELECT ON "sales".customers TO ekos_ro;
GRANT SELECT ON "sales".products TO ekos_ro;
GRANT SELECT ON "sales".orders TO ekos_ro;
GRANT SELECT ON "sales".order_lines TO ekos_ro;
GRANT SELECT ON "sales".order_log TO ekos_ro;
GRANT SELECT ON "sales".big_orders TO ekos_ro AS "sales";
GRANT SELECT ON "hr".employees TO ekos_ro;
