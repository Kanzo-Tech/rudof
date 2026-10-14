# The cases of `rml::sql`

A case is a directory: `mapping.ttl` (an RML-Core mapping graph), `resource.sql` (the tables it reads, DuckDB
SQL), `output.nt` (the triples it must generate, loosely written N-Triples), `base` (the base IRI, when the case
has one) and `title`. `tests/cases.rs` renders every mapping, holds the text to what needs no engine, and, with
`--features duckdb`, runs it over the tables and compares the rows with `output.nt` as sets of terms (a language
tag compares case-insensitively).

## `RMLTC*`: the KG-Construct RML-Core test cases, over SQL

The suite is *RML-Core: Test Cases*, Draft Community Group Report, 31 March 2026
(<https://kg-construct.github.io/rml-core/test-cases/docs/20260331/>, read on 2026-10-14; "not a W3C Standard nor
on the W3C Standards Track"). **It has no SQL cases.** All 152 of its cases are `-JSON`: their input is a JSON file
(`rml:JSONPath`, `rml:RelativePathSource`), and the document says SQL statements for MySQL, PostgreSQL and SQL
Server may accompany a case, but none does. The repository `kg-construct/rml-core` could not be read from the
environment this was written in, so no commit is recorded; the document above is the source.

So these cases are *the same cases transposed*, not vendored files. For each id below:

- `mapping.ttl` is the case's mapping with its logical source replaced by a table (`rml:SQL2008Table`, the
  table named after the JSON file) and its JSONPath references replaced by column names (`$.Name` is `Name`,
  `$['Country Code']` is `Country Code`); nothing else is changed.
- `resource.sql` is the case's JSON input as a table (an integer is an `INTEGER`, a string a `VARCHAR`).
- `output.nt` is the case's expected output, verbatim.

| id | title (as the suite gives it) |
|---|---|
| RMLTC0000 | one table, one column, zero rows |
| RMLTC0001a | One column mapping, subject URI generation by using rml:template |
| RMLTC0002a | Two columns mapping, generation of a subject URI by the concatenation of two column values |
| RMLTC0003c | Three columns mapping, by using a rml:template to produce literal |
| RMLTC0004a | Two column mapping, from one row table to two different triples |
| RMLTC0005a | Typing of resources |
| RMLTC0009a | Generation of triples from foreign key relations |
| RMLTC0011b | M to M relation, by using an additional Triples Map |
| RMLTC0013a | Null value in JSON file (here a NULL) |
| RMLTC0019a | Generation of triples by using IRI value in columns |
| RMLTC0022d | Generating of triples with a constant-valued datatypeMap |
| RMLTC0026a-d | Two triples maps, with and without rml:baseIRI, and the base IRI parameter (`base`) |
| RMLTC0028a | Generation of the right datatype for a constant in the mapping |
| RMLTC0028c | Generation of the right language tag for a constant in the mapping |
| RMLTC0029a | Generating of triples with constant shortcut subject |
| RMLTC0007a | Typing resources by relying on rdf:type predicate |
| RMLTC0007c | One row mapping, using rml:class |
| RMLTC0007d | One column mapping, specifying an rml:predicateObjectMap with rdf:type |
| RMLTC0008b | Generation of triples referencing object map |
| RMLTC0008c | Generation of triples by using multiple predicateMaps within a rml:predicateObjectMap |
| RMLTC0010a | Template with table column with blank space |
| RMLTC0010b | Template with table columns with special chars |
| RMLTC0010c | Template with table columns with special chars and backslashes |
| RMLTC0020a | Generation of triples by using IRI value in columns |
| RMLTC0021a | Generation of triples referencing object map (same logical source and join condition) |
| RMLTC0022a | Generating of triples with constant datatype |
| RMLTC0023f | Valid IRI template with backslash-escape |
| RMLTC0027b | Generation of triples using the UnsafeIRI term type |
| RMLTC0027c | Generation an IRI using the term type IRI |
| RMLTC0030a | Generation of triples from a join with reference-valued parentMap and childMap |
| RMLTC0031a | Generating of triples with a constant-valued language map |
| RMLTC0031b | Generating of triples with a reference-valued language map |

**Left out, and why.** Named graphs (`rml:graph`, `rml:graphMap`: 0006a, 0007b/e/f/g/h, 0008a, 0009b, 0012b, 0028b): the
AST does not model graphs and the parser refuses them (`tests/rdf.rs`). Arrays (0025a-c). A non-constant datatype map
(0022b/c/e), a template-valued language map (0031c), a join whose child or parent map is not a reference (0030b-f), and
`rml:URI` (0027a): modelled by RML-Core, refused by this crate. Blank-node outputs (0001b, 0002b, 0012a/b/e) compare
by isomorphism, not by label; `X-blank-subject` states what this crate does. 0015a: the page prints one of its two
inputs. The error cases about the mapping are in `tests/rdf.rs` (0004b, 0012c/d, 0023a-e, 0024a); the others are about
JSONPath or about data errors a query cannot raise (0002e/g, 0015b, 0019b, 0025b).

## `X-*`: this crate's own

Each case's `title` says which clause it holds the query to.

## `D-fossil-conformance`: a differential test against fossil's `triplesOf`

`packages/corpus/src/triples.ts` of fossil-lang (`triplesOf`, at `7774afd5`, the commit that last changed it;
the repository was at `f47a848f`) hard-wires a reading of a `fossil/1` corpus as `(s, p, o)`. This case is that
corpus's conformance fixture (`packages/corpus/conformance/corpus`: 300 people, 75 orders, 16 tags, 600 `knows`
edges, 75 `placedBy`, 100 `tagged`, 360 nicknames) read through an RML-Core mapping:

- `resource.sql` is the fixture's seven Parquet files dumped as DuckDB tables (DuckDB-WASM 1.4.3).
- `output.nt` is what `triplesOf` generated over them, run on DuckDB-WASM 1.4.3: 2193 distinct triples, no
  duplicates, none with a language tag.
- `mapping.ttl` is written by hand to say what the manifest says: a class and the mapped columns per vertex table, an edge as a
  query that joins the edge table to its vertices by dense id, and the nickname table likewise.

`rml::sql::triples` generates exactly those 2193 triples. `tests/statement_r.rs` then holds the mapping over this
fixture to the round-trip property (layout independence, information preservation, monotonicity).
