//! The Fossil → rudof contract, end to end: the RML mapping a Fossil corpus
//! ships (`fixtures/fossil`, the verbatim output of
//! `@fossil-lang/corpus@0.3.0-alpha.26`), compiled by `compile` and run
//! on DuckDB tables shaped as the corpus stores them, gives the in-memory
//! engine's report on the same data as RDF.
//!
//! The mapping names its tables as delimited identifiers (`"Person"`, and
//! `"acme.Org"`, whose dot is part of the name), and declares an
//! `rml:datatype` on every literal, so the lexical forms must be the natural
//! ones (R2RML §10.2) under the override: ISO timestamps, hex binary,
//! `INF` doubles, `xsd:time` times.
#![cfg(not(target_family = "wasm"))]

use rudof_rdf::RDFFormat;
use rudof_rdf::backend::{OxigraphInMemory, ReaderMode};
use shacl::ir::IRSchema;
use shacl::rdf::ShaclParser;
use shacl::validator::report::ValidationReport;
use shacl::validator::sql::{DuckDbExecutor, SqlDialect, SqlMapping, SqlPlan, compile};

const MAPPING: &str = include_str!("fixtures/fossil/fossil-mapping.rml.ttl");

/// The corpus tables, under a delimited schema with a space in its name.
const TABLES: &str = r#"
CREATE SCHEMA "Fossil Corpus";
CREATE TABLE "Fossil Corpus"."Person" (
    dense_id UINTEGER, subject VARCHAR, name VARCHAR, born TIMESTAMP, birthday DATE,
    wakes TIME, photo BLOB, score DOUBLE, tags VARCHAR[]
);
INSERT INTO "Fossil Corpus"."Person" VALUES
    (0, 'http://example.org/alice', 'Alice', '2020-01-02 03:04:05', '1990-05-06', '07:30:00',
     '\xAB\x01'::BLOB, 'inf'::DOUBLE, ['a', 'b']),
    (1, 'http://example.org/bob', 'Bob', NULL, NULL, NULL, NULL, 2.5, NULL);
CREATE TABLE "Fossil Corpus"."acme.Org" (dense_id UINTEGER, subject VARCHAR, label VARCHAR);
INSERT INTO "Fossil Corpus"."acme.Org" VALUES (0, 'http://example.org/acme', 'ACME');
CREATE TABLE "Fossil Corpus"."Person_worksFor_acme.Org" (src UINTEGER, dst UINTEGER);
INSERT INTO "Fossil Corpus"."Person_worksFor_acme.Org" VALUES (0, 0), (1, 0);
"#;

/// The same corpus as the RDF the mapping means.
const DATA: &str = r#"
@prefix ex:  <http://example.org/> .
@prefix xsd: <http://www.w3.org/2001/XMLSchema#> .
ex:alice a ex:Person ; ex:name "Alice" ;
    ex:born "2020-01-02T03:04:05"^^xsd:dateTime ; ex:birthday "1990-05-06"^^xsd:date ;
    ex:wakes "07:30:00"^^xsd:time ; ex:photo "AB01"^^xsd:hexBinary ; ex:score "INF"^^xsd:double ;
    ex:worksFor ex:acme .
ex:bob a ex:Person ; ex:name "Bob" ; ex:score "2.5"^^xsd:double ; ex:worksFor ex:acme .
ex:acme a ex:Org ; ex:label "ACME" .
"#;

/// Shapes that read every column's term: datatypes, ranges, exact values.
const SHAPES: &str = r#"
@prefix sh:  <http://www.w3.org/ns/shacl#> .
@prefix ex:  <http://example.org/> .
@prefix xsd: <http://www.w3.org/2001/XMLSchema#> .
ex:PersonShape a sh:NodeShape ; sh:targetClass ex:Person ;
    sh:property [ sh:path ex:name ; sh:datatype xsd:string ; sh:minCount 1 ] ;
    sh:property [ sh:path ex:born ; sh:datatype xsd:dateTime ; sh:minCount 1 ;
                  sh:minInclusive "2000-01-01T00:00:00"^^xsd:dateTime ] ;
    sh:property [ sh:path ex:birthday ; sh:datatype xsd:date ] ;
    sh:property [ sh:path ex:wakes ; sh:datatype xsd:time ] ;
    sh:property [ sh:path ex:photo ; sh:datatype xsd:hexBinary ; sh:pattern "^[0-9A-F]+$" ] ;
    sh:property [ sh:path ex:score ; sh:datatype xsd:double ; sh:minInclusive 3 ] ;
    sh:property [ sh:path ex:worksFor ; sh:class ex:Org ; sh:maxCount 1 ] .
ex:AliceShape a sh:NodeShape ; sh:targetNode ex:alice ;
    sh:property [ sh:path ex:born ; sh:hasValue "2020-01-02T03:04:05"^^xsd:dateTime ] ;
    sh:property [ sh:path ex:wakes ; sh:hasValue "07:30:00"^^xsd:time ] ;
    sh:property [ sh:path ex:photo ; sh:hasValue "AB01"^^xsd:hexBinary ] ;
    sh:property [ sh:path ex:score ; sh:hasValue "INF"^^xsd:double ] .
ex:OrgShape a sh:NodeShape ; sh:targetClass ex:Org ;
    sh:property [ sh:path ex:label ; sh:minCount 1 ] ;
    sh:property [ sh:path [ sh:inversePath ex:worksFor ] ; sh:minCount 2 ] .
"#;

fn graph(ttl: &str) -> OxigraphInMemory {
    OxigraphInMemory::from_str(ttl, &RDFFormat::Turtle, None, &ReaderMode::Lax).expect("turtle parses")
}

fn components(report: &ValidationReport) -> Vec<String> {
    let mut out: Vec<String> = report
        .results()
        .iter()
        .map(|r| format!("{} {}", r.focus_node(), r.constraint_component()))
        .collect();
    out.sort();
    out
}

#[test]
fn the_fossil_mapping_validates_the_corpus_tables_as_the_evaluator_validates_its_rdf() {
    let schema = IRSchema::try_from(&ShaclParser::new(graph(SHAPES)).parse().expect("shapes parse")).expect("compile");
    let in_memory = shacl::validator::validate(&schema, &graph(DATA)).expect("in memory validates");

    let executor = DuckDbExecutor::in_memory().expect("duckdb opens");
    executor.connection().execute_batch(TABLES).expect("tables load");
    let mapping = rml(MAPPING, Some(r#""Fossil Corpus""#));
    let plan = compile(&schema, &mapping, SqlDialect::DuckDb).expect("shapes compile");
    let sql = plan.validate(&executor).expect("plan runs");

    // Bob's score (2.5 < 3) and missing birth time: the two violations, and no
    // hasValue among them — every term of alice's row came out exact.
    assert_eq!(
        components(&in_memory),
        [
            "http://example.org/bob http://www.w3.org/ns/shacl#MinCountConstraintComponent",
            "http://example.org/bob http://www.w3.org/ns/shacl#MinInclusiveConstraintComponent",
        ],
        "{in_memory}"
    );
    assert_eq!(
        sql, in_memory,
        "SQL over the corpus tables vs in memory\n{sql}\n---\n{in_memory}"
    );
}

#[test]
fn every_table_name_reaches_duckdb_delimited_once() {
    let mapping = rml(MAPPING, Some(r#""Fossil Corpus""#));
    let schema = IRSchema::try_from(
        &ShaclParser::new(graph(
            "@prefix sh: <http://www.w3.org/ns/shacl#> . @prefix ex: <http://example.org/> .\n\
             ex:S a sh:NodeShape ; sh:targetClass ex:Org ; sh:property [ sh:path [ sh:inversePath ex:worksFor ] ; sh:minCount 1 ] .",
        ))
        .parse()
        .expect("parses"),
    )
    .expect("compiles");
    let plan = compile(&schema, &mapping, SqlDialect::DuckDb).expect("compiles");
    let sql = script(&plan);
    assert!(sql.contains(r#""Fossil Corpus"."acme.Org""#), "{sql}");
    assert!(sql.contains(r#""Fossil Corpus"."Person_worksFor_acme.Org""#), "{sql}");
    assert!(!sql.contains(r#""""#), "an identifier was quoted twice: {sql}");
}

#[test]
fn a_row_naming_no_check_of_the_plan_is_an_error_not_a_result() {
    let schema = IRSchema::try_from(&ShaclParser::new(graph(SHAPES)).parse().expect("shapes parse")).expect("compile");
    let mapping = rml(MAPPING, None);
    let plan = compile(&schema, &mapping, SqlDialect::DuckDb).expect("compiles");
    let row = |check: &str| {
        let mut row = vec![
            Some(check.to_owned()),
            Some("I".to_owned()),
            Some("http://example.org/bob".to_owned()),
        ];
        row.extend([Some(String::new()), Some(String::new()), None, None, None, None, None]);
        row
    };
    assert!(plan.report(&[row("0")]).is_ok());
    assert!(plan.report(&[row(&plan.checks.len().to_string())]).is_err());
    assert!(plan.report(&[row("one")]).is_err());
}

/// A relation several checks read is one temporary table: the focus nodes
/// of `sh:targetClass` are computed once, whatever the number of property
/// shapes reached from them (rudof#6), and the query has one branch per check.
#[test]
fn checks_share_the_relations_they_read() {
    let schema = IRSchema::try_from(&ShaclParser::new(graph(SHAPES)).parse().expect("shapes parse")).expect("compile");
    let mapping = rml(MAPPING, Some(r#""Fossil Corpus""#));
    let plan = compile(&schema, &mapping, SqlDialect::DuckDb).expect("compiles");
    assert!(plan.checks.len() > 1);
    let query = plan.query();
    assert_eq!(query.matches(r#" AS "check""#).count(), plan.checks.len(), "{query}");
    assert!(!plan.setup().is_empty());
    assert!(
        plan.setup().iter().all(|s| s.starts_with("CREATE TEMPORARY TABLE ")),
        "{:?}",
        plan.setup()
    );
    assert_eq!(plan.teardown().len(), plan.setup().len());
}

fn rml(mapping: &str, schema: Option<&str>) -> SqlMapping {
    SqlMapping::Rml {
        mapping: mapping.to_owned(),
        schema: schema.map(str::to_owned),
    }
}

/// The plan's statements, in the order a host runs them.
fn script(plan: &SqlPlan) -> String {
    plan.setup()
        .iter()
        .map(String::as_str)
        .chain([plan.query()])
        .chain(plan.teardown().iter().map(String::as_str))
        .collect::<Vec<_>>()
        .join(";\n")
}

/// An RDF graph is a set: a triple the mapping yields twice (two rules, here)
/// is one triple, and `sh:closed` reports it once, as on the RDF.
#[test]
fn a_triple_the_mapping_yields_twice_is_reported_once() {
    let mapping = r#"
@prefix rml: <http://w3id.org/rml/> . @prefix ex: <http://example.org/> .
<#T> rml:logicalSource [ rml:source [ a rml:Source ] ; rml:referenceFormulation rml:SQL2008Table ;
                         rml:iterator "\"thing\"" ] ;
    rml:subjectMap [ rml:reference "\"id\"" ; rml:class ex:Thing ] ;
    rml:predicateObjectMap [ rml:predicate ex:extra ; rml:objectMap [ rml:reference "x" ] ] ;
    rml:predicateObjectMap [ rml:predicate ex:extra ; rml:objectMap [ rml:reference "x" ] ] .
"#;
    let shapes = r#"
@prefix sh: <http://www.w3.org/ns/shacl#> . @prefix ex: <http://example.org/> .
@prefix rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#> .
ex:S a sh:NodeShape ; sh:targetClass ex:Thing ; sh:closed true ; sh:ignoredProperties ( rdf:type ) .
"#;
    let data = r#"@prefix ex: <http://example.org/> . ex:t a ex:Thing ; ex:extra "v" ."#;
    let schema = IRSchema::try_from(&ShaclParser::new(graph(shapes)).parse().expect("parses")).expect("compiles");
    let in_memory = shacl::validator::validate(&schema, &graph(data)).expect("in memory");
    let executor = DuckDbExecutor::in_memory().expect("duckdb opens");
    executor
        .connection()
        .execute_batch(
            r#"CREATE TABLE thing (id VARCHAR, x VARCHAR); INSERT INTO thing VALUES ('http://example.org/t', 'v');"#,
        )
        .expect("loads");
    let sql = compile(&schema, &rml(mapping, None), SqlDialect::DuckDb)
        .expect("compiles")
        .validate(&executor)
        .expect("runs");
    assert_eq!(in_memory.results().len(), 1);
    assert_eq!(sql, in_memory, "{sql}\n---\n{in_memory}");
}
