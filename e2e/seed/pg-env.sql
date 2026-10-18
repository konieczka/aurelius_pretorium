-- Its password is only in the project's .env.
CREATE TABLE greetings (id int PRIMARY KEY, text text);
INSERT INTO greetings VALUES (1, 'found through .env');
