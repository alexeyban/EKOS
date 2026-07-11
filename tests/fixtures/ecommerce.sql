-- Ecommerce fixture schema
-- Used as a golden dataset for Phase 6 (Knowledge Recovery) testing.
-- Expected entities: Customer, Product, Category, Order, OrderItem, Payment
-- Expected relationships: Order→Customer (placed_by), OrderItem→Order, OrderItem→Product,
--   Product→Category, Category→Category (parent), Payment→Order

CREATE TABLE categories (
    id          SERIAL PRIMARY KEY,
    name        VARCHAR(100) NOT NULL,
    slug        VARCHAR(100) UNIQUE NOT NULL,
    parent_id   INT REFERENCES categories(id) ON DELETE SET NULL
);

CREATE TABLE customers (
    id          SERIAL PRIMARY KEY,
    email       VARCHAR(255) UNIQUE NOT NULL,
    name        VARCHAR(255) NOT NULL,
    phone       VARCHAR(50),
    created_at  TIMESTAMP DEFAULT NOW(),
    updated_at  TIMESTAMP DEFAULT NOW()
);

CREATE TABLE products (
    id              SERIAL PRIMARY KEY,
    sku             VARCHAR(100) UNIQUE NOT NULL,
    name            VARCHAR(255) NOT NULL,
    description     TEXT,
    price           DECIMAL(10, 2) NOT NULL CHECK (price >= 0),
    stock_quantity  INT NOT NULL DEFAULT 0 CHECK (stock_quantity >= 0),
    category_id     INT REFERENCES categories(id) ON DELETE SET NULL,
    created_at      TIMESTAMP DEFAULT NOW()
);

CREATE TABLE orders (
    id              SERIAL PRIMARY KEY,
    customer_id     INT NOT NULL REFERENCES customers(id) ON DELETE RESTRICT,
    status          VARCHAR(50) NOT NULL DEFAULT 'pending'
                        CHECK (status IN ('pending','confirmed','shipped','delivered','cancelled')),
    total_amount    DECIMAL(10, 2),
    shipping_addr   TEXT,
    placed_at       TIMESTAMP DEFAULT NOW(),
    shipped_at      TIMESTAMP,
    delivered_at    TIMESTAMP
);

CREATE TABLE order_items (
    id          SERIAL PRIMARY KEY,
    order_id    INT NOT NULL REFERENCES orders(id) ON DELETE CASCADE,
    product_id  INT NOT NULL REFERENCES products(id) ON DELETE RESTRICT,
    quantity    INT NOT NULL CHECK (quantity > 0),
    unit_price  DECIMAL(10, 2) NOT NULL
);

CREATE TABLE payments (
    id              SERIAL PRIMARY KEY,
    order_id        INT NOT NULL REFERENCES orders(id) ON DELETE RESTRICT,
    amount          DECIMAL(10, 2) NOT NULL CHECK (amount > 0),
    method          VARCHAR(50) CHECK (method IN ('card','bank_transfer','paypal','crypto')),
    status          VARCHAR(50) NOT NULL DEFAULT 'pending'
                        CHECK (status IN ('pending','completed','failed','refunded')),
    transaction_ref VARCHAR(255) UNIQUE,
    processed_at    TIMESTAMP
);

-- Indexes that reveal access patterns (useful for SqlAnalyzer pass)
CREATE INDEX idx_orders_customer ON orders(customer_id);
CREATE INDEX idx_orders_status   ON orders(status);
CREATE INDEX idx_items_order     ON order_items(order_id);
CREATE INDEX idx_items_product   ON order_items(product_id);
CREATE INDEX idx_products_cat    ON products(category_id);
