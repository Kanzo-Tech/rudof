//! SHACL 1.2 Core §3.1.3.6: the nodes of the data graph that conform to a value of
//! `sh:targetWhere` are targets of the shape that has it.
#![cfg(not(target_family = "wasm"))]

use rudof_rdf::backend::{OxigraphInMemory, ReaderMode};
use rudof_rdf::RDFFormat;
use shacl::ir::IRSchema;
use shacl::rdf::ShaclParser;

const PREFIXES: &str = r#"
@prefix sh:  <http://www.w3.org/ns/shacl#> .
@prefix ex:  <http://example.org/> .
@prefix rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#> .
@prefix xsd: <http://www.w3.org/2001/XMLSchema#> .
"#;

fn graph(ttl: &str) -> OxigraphInMemory {
    OxigraphInMemory::from_str(&format!("{PREFIXES}{ttl}"), &RDFFormat::Turtle, None, &ReaderMode::Lax)
        .expect("turtle parses")
}

/// The results of validating `data` against `shapes`, as `(focus, path, component)`, sorted.
fn results(shapes: &str, data: &str) -> Vec<(String, String, String)> {
    let ast = ShaclParser::new(graph(shapes)).parse().expect("sh:targetWhere parses");
    let schema = IRSchema::try_from(&ast).expect("schema compiles");
    let report = shacl::validator::validate(&schema, &graph(data)).expect("validates");
    let mut out: Vec<_> = report
        .results()
        .iter()
        .map(|r| {
            (
                r.focus_node().to_string(),
                r.path().map(ToString::to_string).unwrap_or_default(),
                r.constraint_component().to_string(),
            )
        })
        .collect();
    out.sort();
    out
}

/// Performer is forbidden on automated actions (the WG's example, w3c/data-shapes#341).
const AUTOMATED: &str = r#"
ex:Automated a sh:NodeShape ;
    sh:targetWhere ex:Selector ;
    sh:property [ sh:path ex:performer ; sh:maxCount 0 ] .
ex:Selector a sh:NodeShape ;
    sh:class ex:Action ;
    sh:property [ sh:path ex:isAutomated ; sh:hasValue true ] .
"#;

#[test]
fn a_node_that_conforms_to_the_where_shape_is_validated_and_one_that_does_not_is_not() {
    let found = results(
        AUTOMATED,
        r#"
ex:a1 a ex:Action ; ex:isAutomated true ; ex:performer ex:p .
ex:a2 a ex:Action ; ex:isAutomated false ; ex:performer ex:p .
ex:a3 a ex:Action ; ex:performer ex:p .
ex:a4 ex:isAutomated true ; ex:performer ex:p .
"#,
    );
    assert_eq!(
        found,
        [(
            "http://example.org/a1".into(),
            "http://example.org/performer".into(),
            "http://www.w3.org/ns/shacl#MaxCountConstraintComponent".into()
        )]
    );
}

#[test]
fn a_literal_is_a_node_of_the_graph_and_can_be_a_target() {
    let shapes = r#"
ex:Short a sh:NodeShape ;
    sh:targetWhere [ sh:datatype xsd:string ] ;
    sh:minLength 3 .
"#;
    let found = results(shapes, r#"ex:a ex:p "ab", "abcd" ; ex:q 7 ."#);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].0, r#""ab""#);
    assert!(found[0].2.ends_with("MinLengthConstraintComponent"));
}

#[test]
fn an_iri_that_is_only_a_predicate_is_not_a_node() {
    let shapes = r#"ex:Nothing a sh:NodeShape ; sh:targetWhere [ sh:nodeKind sh:IRI ] ; sh:class ex:C ."#;
    let found = results(shapes, "ex:a ex:p ex:b .");
    let focus: Vec<_> = found.iter().map(|r| r.0.as_str()).collect();
    assert_eq!(focus, ["http://example.org/a", "http://example.org/b"]);
}

#[test]
fn a_deactivated_shape_reports_nothing() {
    let shapes = format!("{AUTOMATED} ex:Automated sh:deactivated true .");
    let found = results(&shapes, "ex:a1 a ex:Action ; ex:isAutomated true ; ex:performer ex:p .");
    assert!(found.is_empty());
}

#[test]
fn a_where_shape_that_leads_back_to_the_shape_terminates() {
    let shapes = r#"
ex:T a sh:NodeShape ; sh:targetWhere ex:W ; sh:class ex:C .
ex:W a sh:NodeShape ; sh:node ex:T .
"#;
    assert!(results(shapes, "ex:a a ex:C .").is_empty());
}

#[test]
fn a_where_shape_of_another_kind_than_the_shape_it_targets_is_read_as_a_shape() {
    // The where shape is only ever referred to as a value of sh:targetWhere.
    let shapes = r#"ex:T sh:targetWhere ex:W ; sh:nodeKind sh:Literal . ex:W sh:class ex:C ."#;
    let found = results(shapes, "ex:a a ex:C .");
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].0, "http://example.org/a");
}
