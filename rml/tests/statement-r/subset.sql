-- R4, monotonicity: every table of the fixture cut to some of its rows. Every logical source of the
-- mapping is a selection, a projection or a join, so what the cut graph generates is among what the
-- whole one does.
CREATE SCHEMA sub;
CREATE TABLE sub."Person" AS SELECT * FROM g."Person" WHERE dense_id % 3 <> 0;
CREATE TABLE sub."Order" AS SELECT * FROM g."Order" WHERE dense_id % 2 = 0;
CREATE TABLE sub."Tag" AS SELECT * FROM g."Tag" WHERE dense_id % 5 <> 0;
CREATE TABLE sub."Person_knows_Person" AS SELECT * FROM g."Person_knows_Person" WHERE (src + dst) % 4 <> 0;
CREATE TABLE sub."Order_placedBy_Person" AS SELECT * FROM g."Order_placedBy_Person" WHERE src % 3 <> 1;
CREATE TABLE sub."Person_tagged_Tag" AS SELECT * FROM g."Person_tagged_Tag" WHERE dst % 2 = 0;
CREATE TABLE sub."Person_nickname" AS SELECT * FROM g."Person_nickname" WHERE src % 2 = 0;
