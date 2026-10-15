CREATE TABLE "child" ("id" INTEGER, "a" VARCHAR, "b" VARCHAR);
CREATE TABLE "parent" ("id" INTEGER, "a" VARCHAR, "b" VARCHAR);
INSERT INTO "child" VALUES (1, 'x', 'y'), (2, 'x', 'z'), (3, NULL, 'y'), (4, 'x', 'y');
INSERT INTO "parent" VALUES (10, 'x', 'y'), (11, 'x', 'q'), (12, NULL, 'y'), (13, 'x', 'y');
