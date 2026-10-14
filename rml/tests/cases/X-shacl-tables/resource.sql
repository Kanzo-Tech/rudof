CREATE TABLE node (id INTEGER, iri VARCHAR);
INSERT INTO node VALUES
    (1, 'http://example.org/alice'), (2, 'http://example.org/bob'), (3, 'http://example.org/carol'),
    (4, 'http://example.org/dave'), (5, 'http://example.org/erin');
CREATE TABLE person (id INTEGER, iri VARCHAR, name VARCHAR, age INTEGER, email VARCHAR);
INSERT INTO person VALUES
    (1, 'http://example.org/alice', 'Alice', 30, NULL),
    (2, 'http://example.org/bob', 'Bob', 17, NULL),
    (4, 'http://example.org/dave', NULL, 40, 'dave@example.org'),
    (5, 'http://example.org/erin', NULL, NULL, NULL);
CREATE TABLE student (id INTEGER, iri VARCHAR, name VARCHAR, age INTEGER);
INSERT INTO student VALUES (3, 'http://example.org/carol', 'carol', 21);
CREATE TABLE knows (src INTEGER, dst INTEGER);
INSERT INTO knows VALUES (1, 2), (1, 3), (1, 4), (1, 5), (2, 1), (2, 5), (3, 4);
