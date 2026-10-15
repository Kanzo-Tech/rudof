# rudof_wasm

`wasm-bindgen` bindings that expose rudof's SHACL stack to JavaScript. `Shapes` is
a parsed shapes graph: it validates a triples relation through SQL on the page's
engine. `Mapping` is a parsed RML-Core mapping (SQL logical sources): it creates the triples
relation `Shapes` validates, as a view over the tables a host holds, and says per
table what the rows mean. A `FormSession` holds a live RDF data graph under one `Shapes`, and offers
parsing, graph editing, serialization, projection and in-memory SHACL validation —
all running in WebAssembly, no SPARQL endpoint or threads required.

Values cross the boundary as plain JS objects via `serde-wasm-bindgen`: RDF terms
as `TermValue` records, shapes as a vocabulary-agnostic `ShapeModelJson`, and
validation as an `sh:ValidationReport` (`ValidationReport`). Each is declared in the `.d.ts`, derived (`tsify`)
from its struct in `src/dto.rs`.

## Build

```sh
./build.sh   # → ./pkg  (wasm-bindgen --target web; size-optimized, wasm-opt -Oz)
```

Requires the `wasm32-unknown-unknown` target, `wasm-bindgen-cli`, and optionally
`wasm-opt` (binaryen). The crate builds `shacl`/`rudof_rdf` with
`default-features = false`: that path is wasm-clean and runs the **native**
validation engine (the `sparql` feature, which needs an endpoint, is off).

## API

```ts
const shapes = Shapes.parse(shaclTurtle, { mediaType: "text/turtle" });
const model = shapes.model();                     // ShapeModelJson

// SHACL on the page's SQL engine, over a triples relation (s_type, s_value, p, o_type, o_value, o_datatype, o_lang):
// `@fossil-lang/corpus`'s open() creates "<job>".triples; engine is mosaic's engine().
const report = await shapes.validate({ table: `"${job}".triples`, engine, signal });
// The same, as findings grouped by shape, constraint, path and severity (in Rust):
const { groups } = await shapes.validateGroups({ table, engine, signal, languages: [...navigator.languages], sample: 3 });
// The Shape Fragment: the rows that make the conforming focus nodes conform, written to `into`;
// with `conforming`, the focus nodes validation reports nothing for (s_type, s_value), not the fragment's subjects.
const { unchecked } = await shapes.fragment({ table, engine, signal, into: `"${job}".fragment`, conforming: `"${job}".conforming` });

// A mapping says what tables mean as RDF (RML-Core; rml:SQL2008Table and rml:SQL2008Query sources).
// `triples` creates or replaces the view the validation above reads; `tables` maps a table the mapping
// names to the SQL name of the relation that holds it. Nothing is read until the view is.
const mapping = Mapping.parse(mappingTurtle, { base: "https://example.org/" });
await mapping.triples({ engine, signal, into: `"${job}".triples`, tables: (t) => `"${job}"."${t}"` });
// Per table (per triples map): the classes, and per column the predicate, term type, datatype, language
// and the annotations (rdfs:label, ...) the mapping graph carries about the class and the predicate.
const { tables } = mapping.model();

// A form: a data graph under the shapes, in memory.
const session = new FormSession(shapes);
session.loadData(dataTurtle, "text/turtle");
session.add(subject, predicate, object);          // live graph editing
const form = session.projectForm(focus, shapeId);
const local = session.validate(null);             // the same ValidationReport
const { findings } = session.validateFindings(null, { languages: ["es"] }); // one Finding<RdfPlace> per result
const ttl = session.serialize("text/turtle");
```

Findings are the neutral model a UI lists — `Finding<Place>` (one result) and
`FindingGroup<Place>` (results sharing a rule, path and severity: a `count`, the
focus nodes as `places`, a `sample`) — with `RdfPlace { focus, path?, value? }`
as the place. `rule.id` is the source shape and constraint component;
`rule.label` reads `path · Constraint`. They are declared in `src/findings.rs`.

## Notes

- `Mapping` is `rml` (the crate beside `shacl`): `parse` reads the mapping graph and refuses what it does not
  model (graph maps, other reference formulations) by name; a generated relative IRI takes the base IRI of its
  triples map, else `options.base`, else `http://example.org/` (RML-Core §4.1.1). The view is a query, so
  the data errors RML-Core §4.3 raises (an invalid IRI, an ill-typed literal) are not raised.

- SHACL validation evaluates the shapes' algebra over the in-memory graph; the
  SQL script is the same algebra, rendered.
- `projectForm` evaluates each property path of a node shape from a focus node,
  preserving value order (forms need deterministic ordering).
