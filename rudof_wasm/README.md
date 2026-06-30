# rudof_wasm

`wasm-bindgen` bindings that expose rudof's SHACL stack to JavaScript. A single
stateful `Session` owns the current RDF data graph plus the loaded shapes, and
offers parsing, graph editing, serialization, projection and SHACL validation —
all running in WebAssembly, no SPARQL endpoint or threads required.

Values cross the boundary as plain JSON via `serde-wasm-bindgen`: RDF terms as
`TermValue` records, shapes as a vocabulary-agnostic `ShapeModelJson`, and
validation as a `RudofReport`. See `src/dto.rs` for the full contract.

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
const session = new Session();
session.loadShapes(shaclTurtle, "text/turtle"); // → ShapeModelJson
session.loadData(dataTurtle, "text/turtle");
session.add(subject, predicate, object);        // live graph editing
const form = session.projectForm(focus, shapeId);
const report = session.validate(null);           // { conforms, results }
const ttl = session.serialize("text/turtle");
```

## Notes

- SHACL validation uses the native engine over an in-memory graph. Shapes are
  validated sequentially on wasm (no rayon/threads), in parallel elsewhere.
- `projectForm` evaluates each property path of a node shape from a focus node,
  preserving value order (forms need deterministic ordering).
