//! Capa-4 parity contract for the SHACL graph-subset recorder.
//!
//! Validating against a `RecordingRdf<&S, BuildRdfRecorder<S>>` wrapper captures
//! the exact subgraph the native validator touches — because every read funnels
//! through the single `NeighsRDF::triples_matching` primitive the decorator
//! overrides. This test pins the design's contract:
//!
//! 1. the recorded subgraph is *exactly* the visited frontier (a strict subset
//!    of the data — untouched triples are absent), and
//! 2. re-validating that subgraph **alone** reproduces the same report and visits
//!    exactly the same frontier (a fixpoint).
#![cfg(not(target_family = "wasm"))]

use rudof_rdf::{BuildRDF, NeighsRDF, RDFFormat};
use rudof_rdf::backend::{OxigraphInMemory, ReaderMode};
use shacl::ast::ASTSchema;
use shacl::ir::IRSchema;
use shacl::rdf::ShaclParser;
use shacl::validator::processor::validate_with_subset;
use std::collections::HashSet;

const SHAPES: &str = r#"
@prefix sh:  <http://www.w3.org/ns/shacl#> .
@prefix ex:  <http://example.org/> .
@prefix xsd: <http://www.w3.org/2001/XMLSchema#> .

ex:PersonShape a sh:NodeShape ;
    sh:targetNode ex:alice ;
    sh:property [
        sh:path ex:name ;
        sh:minCount 2 ;
        sh:datatype xsd:string ;
    ] .
"#;

const DATA: &str = r#"
@prefix ex: <http://example.org/> .
ex:alice ex:name "Alice" .
ex:alice ex:age  30 .
ex:bob   ex:name "Bob" .
"#;

fn graph(ttl: &str) -> OxigraphInMemory {
    OxigraphInMemory::from_str(ttl, &RDFFormat::Turtle, None, &ReaderMode::Lax).expect("turtle parses")
}

fn schema() -> IRSchema {
    let shapes = graph(SHAPES);
    let ast: ASTSchema = ShaclParser::new(shapes).parse().expect("shapes parse");
    IRSchema::try_from(&ast).expect("schema compiles")
}

fn triples_set(g: &OxigraphInMemory) -> HashSet<String> {
    g.triples().expect("triples").map(|t| t.to_string()).collect()
}

#[test]
fn recorded_subgraph_is_exactly_the_visited_frontier_and_revalidates_identically() {
    let schema = schema();
    let data = graph(DATA);

    // Validate the full data graph while recording the visited frontier.
    let (report1, subgraph) =
        validate_with_subset(&data, &schema, OxigraphInMemory::empty()).expect("validate + record");

    let sub = triples_set(&subgraph);

    // (a) The slurp is exactly the path-reachable triple of the one targeted node
    // — a strict subset of the data. Triples the validation never visits (ex:age,
    // the untargeted ex:bob) are absent.
    assert_eq!(sub.len(), 1, "exactly one triple visited via triples_matching, got {sub:?}");
    assert!(
        sub.iter().any(|t| t.contains("/alice") && t.contains("/name")),
        "the ex:alice ex:name triple was recorded: {sub:?}"
    );
    assert!(!sub.iter().any(|t| t.contains("/age")), "ex:age is never read → not recorded");
    assert!(!sub.iter().any(|t| t.contains("/bob")), "ex:bob is never targeted → not recorded");

    // The run produced a real report: minCount 2 with a single value → one violation.
    assert_eq!(report1.results().len(), 1, "one minCount violation");

    // (b) Re-validating the recorded subgraph ALONE reproduces the report, and
    // recording that run captures exactly the same frontier — a fixpoint, proving
    // the slurp is self-contained for the shapes that produced it.
    let (report2, subgraph2) =
        validate_with_subset(&subgraph, &schema, OxigraphInMemory::empty()).expect("re-validate subgraph");

    assert_eq!(report1, report2, "the subgraph alone yields the same report");
    assert_eq!(
        sub,
        triples_set(&subgraph2),
        "re-validation visits exactly the recorded frontier (fixpoint)"
    );
}
