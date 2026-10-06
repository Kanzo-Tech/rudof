# W3C R2RML test cases

The R2RML test cases of the W3C RDB2RDF Working Group
(https://www.w3.org/2001/sw/rdb2rdf/test-cases/), as KG-Construct packages them
for the R2RML implementation report in
https://github.com/kg-construct/r2rml-test-cases-support at commit
`976098802e9bb0e297d196b26a4c2e0b95fd7644` (Apache-2.0, `LICENSE`).

Copied verbatim: `manifest.ttl`, `databases/*.sql` and each case's mapping and
expected output. The MySQL variants of the mappings (`*-mysql.ttl`) are left
out; the engine reads the standard ones.

`src/validator/sql/r2rml_suite.rs` runs every case and says which are
inapplicable or fail, and why. `R2RML_EARL=<path>` writes the outcomes as an
EARL 1.0 report.
