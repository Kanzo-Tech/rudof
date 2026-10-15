-- R1, layout independence: the fixture's graph with its vertices in another order, renumbered, and its
-- integers wider. Each type is renumbered by a bijection of 0..n (7 is coprime to 300 and 75, 3 to 16), the
-- edge endpoints with it, and every table is written in the reverse of its new order.
CREATE SCHEMA perm;
CREATE TABLE perm."Person" AS SELECT ((dense_id * 7 + 3) % 300)::BIGINT AS dense_id, subject, birth_year::BIGINT AS birth_year, postcode FROM g."Person" ORDER BY 1 DESC;
CREATE TABLE perm."Order" AS SELECT ((dense_id * 7 + 3) % 75)::BIGINT AS dense_id, subject, amount FROM g."Order" ORDER BY 1 DESC;
CREATE TABLE perm."Tag" AS SELECT ((dense_id * 3 + 5) % 16)::BIGINT AS dense_id, subject, name FROM g."Tag" ORDER BY 1 DESC;
CREATE TABLE perm."Person_knows_Person" AS SELECT ((src * 7 + 3) % 300)::BIGINT AS src, ((dst * 7 + 3) % 300)::BIGINT AS dst, since::BIGINT AS since FROM g."Person_knows_Person" ORDER BY 1 DESC, 2 DESC;
CREATE TABLE perm."Order_placedBy_Person" AS SELECT ((src * 7 + 3) % 75)::BIGINT AS src, ((dst * 7 + 3) % 300)::BIGINT AS dst FROM g."Order_placedBy_Person" ORDER BY 1 DESC, 2 DESC;
CREATE TABLE perm."Person_tagged_Tag" AS SELECT ((src * 7 + 3) % 300)::BIGINT AS src, ((dst * 3 + 5) % 16)::BIGINT AS dst FROM g."Person_tagged_Tag" ORDER BY 1 DESC, 2 DESC;
CREATE TABLE perm."Person_nickname" AS SELECT ((src * 7 + 3) % 300)::BIGINT AS src, nickname FROM g."Person_nickname" ORDER BY 1 DESC, 2 DESC;
