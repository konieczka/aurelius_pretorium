-- Its password is nowhere: the tab asks for it.
CREATE TABLE vault (id int PRIMARY KEY, secret text);
INSERT INTO vault VALUES (1, 'unlocked');
