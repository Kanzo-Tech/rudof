# Fossil corpus mapping fixture

`fossil-mapping.rml.ttl` is the verbatim output of `mapping()` from
`@fossil-lang/corpus@0.3.0-alpha.26` for the manifest `fossil.json`, generated
once with `gen.mjs`:

```sh
npm install @fossil-lang/corpus@0.3.0-alpha.26
node gen.mjs > fossil-mapping.rml.ttl
```

The manifest exercises what a corpus can name: a delimited mixed-case table
(`"Person"`), a table whose name contains a dot (`"acme.Org"`), an edge table
joined to both by surrogate keys, and timestamp, date, time, binary, double
and list columns. `tests/fossil_contract.rs` runs the mapping through
`compile` on DuckDB tables shaped as the corpus stores them.
