//! SHACL 1.2 Core §7.1.2: the value of `sh:datatype` is an IRI or a SHACL list of
//! IRIs, and a value node conforms when its datatype is one of them.
#![cfg(not(target_family = "wasm"))]

use rudof_rdf::backend::{OxigraphInMemory, ReaderMode};
use rudof_rdf::RDFFormat;
use shacl::ir::IRSchema;
use shacl::rdf::ShaclParser;

const SHAPES: &str = r#"
@prefix sh:  <http://www.w3.org/ns/shacl#> .
@prefix ex:  <http://example.org/> .
@prefix rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#> .
@prefix xsd: <http://www.w3.org/2001/XMLSchema#> .

ex:S a sh:NodeShape ;
    sh:targetNode ex:a ;
    sh:property [ sh:path ex:p ; sh:datatype ( xsd:string rdf:langString ) ] .
"#;

fn graph(ttl: &str) -> OxigraphInMemory {
    OxigraphInMemory::from_str(ttl, &RDFFormat::Turtle, None, &ReaderMode::Lax).expect("turtle parses")
}

fn conforms(data: &str) -> bool {
    let ast = ShaclParser::new(graph(SHAPES))
        .parse()
        .expect("a list-valued sh:datatype parses");
    let schema = IRSchema::try_from(&ast).expect("schema compiles");
    let report = shacl::validator::validate(&schema, &graph(data)).expect("validates");
    report.conforms()
}

#[test]
fn a_value_of_any_listed_datatype_conforms() {
    assert!(conforms(r#"@prefix ex: <http://example.org/> . ex:a ex:p "plain" ."#));
    assert!(conforms(
        r#"@prefix ex: <http://example.org/> . ex:a ex:p "tagged"@en ."#
    ));
}

#[test]
fn a_value_of_none_of_the_listed_datatypes_does_not() {
    assert!(!conforms(r#"@prefix ex: <http://example.org/> . ex:a ex:p 42 ."#));
    assert!(!conforms(r#"@prefix ex: <http://example.org/> . ex:a ex:p ex:b ."#));
}
