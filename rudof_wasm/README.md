# rudof_wasm

`wasm-bindgen` bindings that expose rudof's SHACL stack to JavaScript. `Shapes` is
a parsed shapes graph: it validates a triples relation through SQL on the page's
engine. A `FormSession` holds a live RDF data graph under one `Shapes`, and offers
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

- SHACL validation evaluates the shapes' algebra over the in-memory graph; the
  SQL script is the same algebra, rendered.
- `projectForm` evaluates each property path of a node shape from a focus node,
  preserving value order (forms need deterministic ordering).
