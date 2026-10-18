-- The Postgres the tests browse over its published port (logins from the container's settings).
CREATE TABLE customers (id serial PRIMARY KEY, name text NOT NULL, email text, vip boolean NOT NULL DEFAULT false, meta jsonb);
INSERT INTO customers (name, email, vip, meta)
SELECT 'Customer ' || lpad(i::text, 3, '0'), 'customer' || i || '@example.com', i % 10 = 0,
       jsonb_build_object('tier', CASE WHEN i % 10 = 0 THEN 'gold' ELSE 'basic' END, 'visits', i)
FROM generate_series(1, 250) i;
UPDATE customers SET name = 'Ada Lovelace', email = 'ada@example.com' WHERE id = 1;
UPDATE customers SET email = NULL WHERE id = 2;

CREATE TABLE orders (id serial PRIMARY KEY, customer_id int REFERENCES customers, total numeric(10, 2), placed_at timestamptz DEFAULT now());
INSERT INTO orders (customer_id, total) SELECT (i % 250) + 1, (i * 7 % 500) + 0.99 FROM generate_series(1, 600) i;

CREATE VIEW big_spenders AS
  SELECT c.name, sum(o.total) AS spent FROM customers c JOIN orders o ON o.customer_id = c.id GROUP BY c.name HAVING sum(o.total) > 1000;

-- Edited by the write-mode tests, a row each.
CREATE TABLE settings (key text PRIMARY KEY, value text);
INSERT INTO settings VALUES ('theme', 'dark'), ('currency', 'EUR'), ('greeting', 'hello'), ('locale', 'en'), ('notes', 'none');

-- No primary key: its rows can't be edited one by one.
CREATE SCHEMA audit;
CREATE TABLE audit.events (at timestamptz DEFAULT now(), what text);
INSERT INTO audit.events (what) VALUES ('signup'), ('login');

ANALYZE;
