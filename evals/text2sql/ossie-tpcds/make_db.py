"""A small, deterministic SQLite database for `tests/fixtures/ossie-tpcds` (RFC 0246 A/B).

The same five TPC-DS tables as the fixture DDL, filled from a fixed seed: 2 years of dates,
4 stores, 30 items, 60 customers, 800 sales lines. Synthetic data, no TPC tooling.

    python3 make_db.py <out.sqlite>
"""

from __future__ import annotations

import random
import sqlite3
import sys
from datetime import date, timedelta

DDL = """
CREATE TABLE date_dim (d_date_sk INTEGER PRIMARY KEY, d_date_id TEXT NOT NULL, d_date DATE,
  d_year INTEGER, d_moy INTEGER, d_quarter_name TEXT);
CREATE TABLE customer (c_customer_sk INTEGER PRIMARY KEY, c_customer_id TEXT NOT NULL UNIQUE,
  c_first_name TEXT, c_last_name TEXT, c_preferred_cust_flag TEXT, c_email_address TEXT);
CREATE TABLE item (i_item_sk INTEGER PRIMARY KEY, i_item_id TEXT NOT NULL, i_item_desc TEXT,
  i_current_price REAL, i_brand TEXT, i_category TEXT);
CREATE TABLE store (s_store_sk INTEGER PRIMARY KEY, s_store_id TEXT NOT NULL, s_store_name TEXT,
  s_number_employees INTEGER, s_city TEXT, s_state TEXT);
CREATE TABLE store_sales (ss_sold_date_sk INTEGER REFERENCES date_dim (d_date_sk),
  ss_item_sk INTEGER NOT NULL REFERENCES item (i_item_sk),
  ss_customer_sk INTEGER REFERENCES customer (c_customer_sk),
  ss_store_sk INTEGER REFERENCES store (s_store_sk), ss_ticket_number INTEGER NOT NULL,
  ss_quantity INTEGER, ss_sales_price REAL, ss_ext_sales_price REAL, ss_net_profit REAL,
  PRIMARY KEY (ss_item_sk, ss_ticket_number));
"""

FIRST = ["Ann", "Bob", "Carla", "Dev", "Eli", "Fay", "Gus", "Hana", "Ivo", "Jo"]
LAST = ["Smith", "Lopez", "Novak", "Kim", "Okafor", "Rossi"]
STORES = [("Northgate", 12, "Seattle", "WA"), ("Riverside", 8, "Portland", "OR"),
          ("Lakeview", 20, "Chicago", "IL"), ("Harbor", 5, "Boston", "MA")]
CATEGORIES = ["Books", "Electronics", "Home"]
BRANDS = ["Acme", "Globex", "Initech", "Umbrella", "Hooli"]


def build(path: str) -> None:
    rng = random.Random(245)
    db = sqlite3.connect(path)
    db.executescript(DDL)
    start = date(2001, 1, 1)
    for i in range(730):
        d = start + timedelta(days=i)
        q = (d.month - 1) // 3 + 1
        db.execute("INSERT INTO date_dim VALUES (?,?,?,?,?,?)",
                   (i + 1, f"D{i + 1:06d}", d.isoformat(), d.year, d.month, f"{d.year}Q{q}"))
    for c in range(1, 61):
        f, l = rng.choice(FIRST), rng.choice(LAST)
        db.execute("INSERT INTO customer VALUES (?,?,?,?,?,?)",
                   (c, f"C{c:06d}", f, l, "Y" if rng.random() < 0.3 else "N",
                    f"{f.lower()}.{l.lower()}{c}@example.com"))
    for i in range(1, 31):
        db.execute("INSERT INTO item VALUES (?,?,?,?,?,?)",
                   (i, f"I{i:06d}", f"item {i}", round(rng.uniform(2, 200), 2),
                    rng.choice(BRANDS), rng.choice(CATEGORIES)))
    for s, (name, emp, city, state) in enumerate(STORES, start=1):
        db.execute("INSERT INTO store VALUES (?,?,?,?,?,?)",
                   (s, f"S{s:04d}", name, emp, city, state))
    for t in range(1, 801):
        item = rng.randint(1, 30)
        qty = rng.randint(1, 5)
        price = round(rng.uniform(2, 200), 2)
        ext = round(price * qty, 2)
        db.execute("INSERT INTO store_sales VALUES (?,?,?,?,?,?,?,?,?)",
                   (rng.randint(1, 730), item, rng.randint(1, 60), rng.randint(1, 4), t, qty,
                    price, ext, round(ext * rng.uniform(-0.1, 0.35), 2)))
    db.commit()
    db.close()


if __name__ == "__main__":
    build(sys.argv[1])
