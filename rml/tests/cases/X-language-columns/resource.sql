CREATE TABLE "Person" ("subject" VARCHAR, "nickname" STRUCT("value" VARCHAR, "lang" VARCHAR)[]);
INSERT INTO "Person" VALUES ('http://example.com/p/1', [{'value': 'Ali', 'lang': 'en'}, {'value': 'Alí', 'lang': 'es'}]);
INSERT INTO "Person" VALUES ('http://example.com/p/2', []);
INSERT INTO "Person" VALUES ('http://example.com/p/3', NULL);
INSERT INTO "Person" VALUES ('http://example.com/p/4', [{'value': 'Zed', 'lang': 'EN-gb'}]);
INSERT INTO "Person" VALUES ('http://example.com/p/5', [{'value': 'NoTag', 'lang': NULL}, {'value': 'EmptyTag', 'lang': ''}, {'value': 'Kept', 'lang': 'de'}]);
