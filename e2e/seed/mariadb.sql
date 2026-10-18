CREATE TABLE products (id int AUTO_INCREMENT PRIMARY KEY, name varchar(100) NOT NULL, price decimal(8, 2), stock int, tags json);
INSERT INTO products (name, price, stock, tags) VALUES
  ('Espresso cup', 12.50, 40, '["kitchen"]'), ('Moka pot', 34.00, 12, '["kitchen", "coffee"]'), ('Grinder', 89.90, 5, '["coffee"]'),
  ('Kettle', 45.00, 0, NULL), ('Scale', 25.00, 18, '["coffee"]'), ('Filter papers', 4.20, 300, '["coffee", "refill"]');
CREATE TABLE suppliers (id int PRIMARY KEY, name varchar(100), country char(2));
INSERT INTO suppliers VALUES (1, 'Bialetti', 'IT'), (2, 'Hario', 'JP');
