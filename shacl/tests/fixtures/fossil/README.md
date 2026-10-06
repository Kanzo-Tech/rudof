# Fossil corpus mapping fixture

`fossil-mapping.r2rml.ttl` is the verbatim output of `mapping()` in
`@fossil-lang/corpus` (Kanzo-Tech/fossil-lang `packages/corpus/src/mapping.ts`
at commit 8542784) for the manifest `fossil.json`, generated with `gen.mjs`
against a build of that package:

```sh
node gen.mjs > fossil-mapping.r2rml.ttl
```

The manifest exercises what a corpus can name: a delimited mixed-case table
(`"Person"`), a table whose name contains a dot (`"acme.Org"`), an edge table
joined to both by surrogate keys (an R2RML view, `rr:sqlQuery`), a
multi-valued property in its own table, and timestamp, date, time, binary and
double columns with the datatypes their shape declares.
`tests/fossil_contract.rs` runs the mapping through `compile` on DuckDB tables
shaped as the corpus stores them.
