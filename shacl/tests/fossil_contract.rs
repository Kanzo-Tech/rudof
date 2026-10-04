//! The Fossil → rudof contract, end to end: the RML mapping a Fossil corpus
//! ships (`fixtures/fossil`, the verbatim output of
//! `@fossil-lang/corpus@0.3.0-alpha.26`), compiled by `compile_sql` and run
//! on DuckDB tables shaped as the corpus stores them, gives the native
//! engine's report on the same data as RDF.
//!
//! The mapping names its tables as delimited identifiers (`"Person"`, and
//! `"acme.Org"`, whose dot is part of the name), and declares an
//! `rml:datatype` on every literal, so the lexical forms must be the natural
//! ones (R2RML §10.2) under the override: ISO timestamps, hex binary,
//! `INF` doubles, `xsd:time` times.
#![cfg(not(target_family = "wasm"))]

use rudof_rdf::backend::{OxigraphInMemory, ReaderMode};
use rudof_rdf::{BuildRDF, RDFFormat};
use shacl::ir::IRSchema;
use shacl::rdf::ShaclParser;
use shacl::validator::processor::validate_with_subset;
use shacl::validator::report::ValidationReport;
use shacl::validator::sql::{DuckDb, DuckDbExecutor, Tables, compile_sql};

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
fn the_fossil_mapping_validates_the_corpus_tables_as_the_native_engine_validates_its_rdf() {
    let schema = IRSchema::try_from(&ShaclParser::new(graph(SHAPES)).parse().expect("shapes parse")).expect("compile");
    let native = validate_with_subset(&graph(DATA), &schema, OxigraphInMemory::empty())
        .expect("native validates")
        .0;

    let executor = DuckDbExecutor::in_memory().expect("duckdb opens");
    executor.connection().execute_batch(TABLES).expect("tables load");
    let mapping = Tables::from_rml(MAPPING, Some(r#""Fossil Corpus""#), DuckDb).expect("the Fossil mapping reads");
    let plan = compile_sql(&schema, &mapping, &DuckDb).expect("shapes compile");
    let sql = plan.validate(&schema, &executor).expect("plan runs");

    // Bob's score (2.5 < 3) and missing birth time: the two violations, and no
    // hasValue among them — every term of alice's row came out exact.
    assert_eq!(
        components(&native),
        [
            "http://example.org/bob http://www.w3.org/ns/shacl#MinCountConstraintComponent",
            "http://example.org/bob http://www.w3.org/ns/shacl#MinInclusiveConstraintComponent",
        ],
        "{native}"
    );
    assert_eq!(
        sql, native,
        "SQL over the corpus tables vs native\n{sql}\n---\n{native}"
    );
}

#[test]
fn every_table_name_reaches_duckdb_delimited_once() {
    let mapping = Tables::from_rml(MAPPING, Some(r#""Fossil Corpus""#), DuckDb).expect("reads");
    let schema = IRSchema::try_from(
        &ShaclParser::new(graph(
            "@prefix sh: <http://www.w3.org/ns/shacl#> . @prefix ex: <http://example.org/> .\n\
             ex:S a sh:NodeShape ; sh:targetClass ex:Org ; sh:property [ sh:path [ sh:inversePath ex:worksFor ] ; sh:minCount 1 ] .",
        ))
        .parse()
        .expect("parses"),
    )
    .expect("compiles");
    let plan = compile_sql(&schema, &mapping, &DuckDb).expect("compiles");
    let sql = plan.checks[0].sql();
    assert!(sql.contains(r#""Fossil Corpus"."acme.Org""#), "{sql}");
    assert!(sql.contains(r#""Fossil Corpus"."Person_worksFor_acme.Org""#), "{sql}");
    assert!(!sql.contains(r#""""#), "an identifier was quoted twice: {sql}");
}

#[test]
fn rows_for_fewer_checks_than_the_plan_has_are_an_error_not_conformance() {
    let schema = IRSchema::try_from(&ShaclParser::new(graph(SHAPES)).parse().expect("shapes parse")).expect("compile");
    let mapping = Tables::from_rml(MAPPING, None, DuckDb).expect("reads");
    let plan = compile_sql(&schema, &mapping, &DuckDb).expect("compiles");
    assert!(plan.checks.len() > 1);
    assert!(plan.report(&schema, &[]).is_err());
    assert!(plan.report(&schema, &[Vec::new()]).is_err());
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
    let native = validate_with_subset(&graph(data), &schema, OxigraphInMemory::empty())
        .expect("native")
        .0;
    let executor = DuckDbExecutor::in_memory().expect("duckdb opens");
    executor
        .connection()
        .execute_batch(
            r#"CREATE TABLE thing (id VARCHAR, x VARCHAR); INSERT INTO thing VALUES ('http://example.org/t', 'v');"#,
        )
        .expect("loads");
    let tables = Tables::from_rml(mapping, None, DuckDb).expect("reads");
    let sql = compile_sql(&schema, &tables, &DuckDb)
        .expect("compiles")
        .validate(&schema, &executor)
        .expect("runs");
    assert_eq!(native.results().len(), 1);
    assert_eq!(sql, native, "{sql}\n---\n{native}");
}
