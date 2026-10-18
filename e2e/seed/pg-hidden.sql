-- A Postgres container that publishes no port: browsed with psql inside it.
CREATE TABLE bins (id int PRIMARY KEY, label text NOT NULL, aisle int);
INSERT INTO bins VALUES (1, 'Bolts', 3), (2, 'Nuts', 3), (3, 'Washers', NULL);
