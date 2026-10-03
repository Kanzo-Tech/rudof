//! The SQL engine reads the same report from one dataset whether it sits in a
//! triple table or in ordinary tables described by a `Tables` mapping — and
//! that report is the native engine's.
#![cfg(not(target_family = "wasm"))]

use rudof_rdf::backend::{OxigraphInMemory, ReaderMode};
use rudof_rdf::{BuildRDF, RDFFormat};
use shacl::ir::IRSchema;
use shacl::rdf::ShaclParser;
use shacl::validator::processor::validate_with_subset;
use shacl::validator::report::ValidationReport;
use shacl::validator::sql::{
    DuckDb, DuckDbExecutor, SqlCompileError, SqlPlan, Tables, compile_sql, validate_with_duckdb,
};

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

fn schema(shapes: &str) -> IRSchema {
    let ast = ShaclParser::new(graph(shapes)).parse().expect("shapes parse");
    IRSchema::try_from(&ast).expect("schema compiles")
}

/// The data, as RDF.
const DATA: &str = r#"
ex:alice a ex:Person ; ex:name "Alice" ; ex:age 30 ; ex:knows ex:bob, ex:carol, ex:dave, ex:erin .
ex:bob a ex:Person ; ex:name "Bob" ; ex:age 17 ; ex:knows ex:alice, ex:erin .
ex:carol a ex:Student ; ex:name "carol" ; ex:age 21 ; ex:knows ex:dave .
ex:dave a ex:Person ; ex:age 40 ; ex:email "dave@example.org" .
ex:erin a ex:Person .
ex:Student rdfs:subClassOf ex:Person .
"#;

/// The same data, as the tables a host would hold.
const TABLES: &str = r#"
CREATE TABLE person (id VARCHAR, name VARCHAR, age INTEGER, email VARCHAR);
INSERT INTO person VALUES
    ('http://example.org/alice', 'Alice', 30, NULL),
    ('http://example.org/bob', 'Bob', 17, NULL),
    ('http://example.org/dave', NULL, 40, 'dave@example.org'),
    ('http://example.org/erin', NULL, NULL, NULL);
CREATE TABLE student (id VARCHAR, name VARCHAR, age INTEGER);
INSERT INTO student VALUES ('http://example.org/carol', 'carol', 21);
CREATE TABLE knows (src VARCHAR, dst VARCHAR);
INSERT INTO knows VALUES
    ('http://example.org/alice', 'http://example.org/bob'),
    ('http://example.org/alice', 'http://example.org/carol'),
    ('http://example.org/alice', 'http://example.org/dave'),
    ('http://example.org/alice', 'http://example.org/erin'),
    ('http://example.org/bob', 'http://example.org/alice'),
    ('http://example.org/bob', 'http://example.org/erin'),
    ('http://example.org/carol', 'http://example.org/dave');
"#;

/// Where those tables' terms are: classes, literal columns, an edge table.
const MAPPING: &str = r#"{
  "classes": [
    { "class": "http://example.org/Person", "table": "person", "subject": { "column": "id" } },
    { "class": "http://example.org/Student", "table": "student", "subject": { "column": "id" } }
  ],
  "properties": [
    { "predicate": "http://example.org/name", "table": "person",
      "subject": { "column": "id" }, "object": { "column": "name", "termType": "Literal" } },
    { "predicate": "http://example.org/name", "table": "student",
      "subject": { "column": "id" }, "object": { "column": "name", "termType": "Literal" } },
    { "predicate": "http://example.org/age", "table": "person",
      "subject": { "column": "id" },
      "object": { "column": "age", "termType": "Literal",
                  "datatype": "http://www.w3.org/2001/XMLSchema#integer" } },
    { "predicate": "http://example.org/age", "table": "student",
      "subject": { "column": "id" },
      "object": { "column": "age", "termType": "Literal",
                  "datatype": "http://www.w3.org/2001/XMLSchema#integer" } },
    { "predicate": "http://example.org/email", "table": "person",
      "subject": { "column": "id" }, "object": { "column": "email", "termType": "Literal" } },
    { "predicate": "http://example.org/knows", "table": "knows",
      "subject": { "column": "src" }, "object": { "column": "dst" } }
  ],
  "subClassOf": [ { "sub": "http://example.org/Student", "super": "http://example.org/Person" } ]
}"#;

/// Shapes over classes, literal columns and edges: targets with subclasses,
/// cardinality, ranges, patterns, a path over the edge table, a closure, a
/// property pair, and the shape-based components.
const SHAPES: &str = r#"
@prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .
ex:PersonShape a sh:NodeShape ;
    sh:targetClass ex:Person ;
    sh:property [ sh:path ex:name ; sh:minCount 1 ; sh:datatype xsd:string ; sh:pattern "^[A-Z]" ] ;
    sh:property [ sh:path ex:age ; sh:maxCount 1 ; sh:minInclusive 18 ] ;
    sh:property [ sh:path ex:knows ; sh:class ex:Person ; sh:node ex:Named ] ;
    sh:property [ sh:path ( ex:knows ex:knows ) ; sh:disjoint ex:knows ] ;
    sh:property [ sh:path [ sh:oneOrMorePath ex:knows ] ; sh:minCount 2 ] ;
    sh:property [ sh:path [ sh:inversePath ex:knows ] ; sh:maxCount 1 ;
                  sh:qualifiedValueShape ex:Named ; sh:qualifiedMinCount 1 ] .
ex:Named a sh:NodeShape ;
    sh:property [ sh:path ex:name ; sh:minCount 1 ] .
ex:AdultShape a sh:NodeShape ;
    sh:targetSubjectsOf ex:age ;
    sh:or ( [ sh:path ex:age ; sh:minExclusive 20 ] [ sh:path ex:knows ; sh:minCount 2 ] ) ;
    sh:closed true ;
    sh:ignoredProperties ( rdf:type ex:name ex:age ) ;
    sh:property [ sh:path ex:knows ] .
"#;

fn native(data: &OxigraphInMemory, schema: &IRSchema) -> ValidationReport {
    validate_with_subset(data, schema, OxigraphInMemory::empty())
        .expect("native validates")
        .0
}

fn through_tables(schema: &IRSchema) -> ValidationReport {
    let executor = DuckDbExecutor::in_memory().expect("duckdb opens");
    executor.connection().execute_batch(TABLES).expect("tables load");
    let mapping = Tables::from_json(MAPPING, DuckDb).expect("mapping parses");
    let plan: SqlPlan = compile_sql(schema, &mapping, &DuckDb).expect("shapes compile");
    plan.validate(schema, &executor).expect("plan runs")
}

#[test]
fn tables_and_triple_table_yield_the_native_report() {
    let data = graph(&format!(
        "@prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .\n{DATA}"
    ));
    let schema = schema(SHAPES);

    let native = native(&data, &schema);
    let triples = validate_with_duckdb(&data, &schema).expect("triple table validates");
    let tables = through_tables(&schema);

    assert!(
        native.results().len() > 15,
        "the dataset violates many components: {native}"
    );
    assert_eq!(triples, native, "triple table vs native\n{triples}\n---\n{native}");
    assert_eq!(tables, native, "tables vs native\n{tables}\n---\n{native}");
}

#[test]
fn messages_are_the_native_engines() {
    let data = graph(&format!(
        "@prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .\n{DATA}"
    ));
    let schema = schema(SHAPES);
    let keyed = |report: &ValidationReport| {
        let mut out: Vec<_> = report
            .results()
            .iter()
            .map(|r| {
                (
                    format!("{} {} {:?}", r.focus_node(), r.constraint_component(), r.value()),
                    r.message().clone(),
                )
            })
            .collect();
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
    };
    let native = keyed(&native(&data, &schema));
    let sql = keyed(&validate_with_duckdb(&data, &schema).expect("validates"));
    assert_eq!(sql.len(), native.len());
    for ((key, sql), (_, native)) in sql.iter().zip(&native) {
        assert_eq!(sql, native, "the message of {key}");
    }
}

#[test]
fn recursive_shapes_are_refused() {
    let schema = schema(
        r#"
ex:S a sh:NodeShape ; sh:targetClass ex:Person ;
    sh:property [ sh:path ex:knows ; sh:node ex:S ] .
"#,
    );
    let mapping = Tables::from_json(MAPPING, DuckDb).expect("mapping parses");
    let refused = compile_sql(&schema, &mapping, &DuckDb).expect_err("recursion is refused");
    assert!(matches!(refused, SqlCompileError::RecursiveShapes(_)), "{refused}");
}

#[test]
fn shacl_sparql_is_refused_not_skipped() {
    let schema = schema(
        r#"
ex:S a sh:NodeShape ; sh:targetClass ex:Person ;
    sh:sparql [ sh:select "SELECT $this WHERE { $this ?p ?o }" ] .
"#,
    );
    let mapping = Tables::from_json(MAPPING, DuckDb).expect("mapping parses");
    let refused = compile_sql(&schema, &mapping, &DuckDb).expect_err("sh:sparql is refused");
    assert!(matches!(refused, SqlCompileError::Unsupported(_)), "{refused}");
}

#[test]
fn target_where_is_refused_not_skipped() {
    let schema = schema(
        r#"
ex:S a sh:NodeShape ; sh:targetWhere ex:W ; sh:property [ sh:path ex:name ; sh:minCount 1 ] .
ex:W a sh:NodeShape ; sh:class ex:Person .
"#,
    );
    let mapping = Tables::from_json(MAPPING, DuckDb).expect("mapping parses");
    let refused = compile_sql(&schema, &mapping, &DuckDb).expect_err("sh:targetWhere is refused");
    assert!(matches!(refused, SqlCompileError::Unsupported(_)), "{refused}");
}
