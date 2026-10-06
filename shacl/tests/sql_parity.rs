//! The SQL engine reads the same report from one dataset whether it sits in a
//! triples table or in ordinary tables under a triples view — and that report
//! is the in-memory evaluator's.
#![cfg(not(target_family = "wasm"))]

use futures::executor::block_on;
use rudof_rdf::RDFFormat;
use rudof_rdf::backend::{OxigraphInMemory, ReaderMode};
use shacl::ir::IRSchema;
use shacl::rdf::ShaclParser;
use shacl::validator::report::ValidationReport;
use shacl::validator::sql::{DuckDbEngine, validate};

mod common;
use common::validate_with_duckdb;

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

/// The same data, as the tables a host would hold: vertices with surrogate
/// keys, and an edge table `knows(src, dst)` that refers to them by key.
const TABLES: &str = r#"
CREATE TABLE node (id INTEGER, iri VARCHAR);
INSERT INTO node VALUES
    (1, 'http://example.org/alice'), (2, 'http://example.org/bob'), (3, 'http://example.org/carol'),
    (4, 'http://example.org/dave'), (5, 'http://example.org/erin');
CREATE TABLE person (id INTEGER, iri VARCHAR, name VARCHAR, age INTEGER, email VARCHAR);
INSERT INTO person VALUES
    (1, 'http://example.org/alice', 'Alice', 30, NULL),
    (2, 'http://example.org/bob', 'Bob', 17, NULL),
    (4, 'http://example.org/dave', NULL, 40, 'dave@example.org'),
    (5, 'http://example.org/erin', NULL, NULL, NULL);
CREATE TABLE student (id INTEGER, iri VARCHAR, name VARCHAR, age INTEGER);
INSERT INTO student VALUES (3, 'http://example.org/carol', 'carol', 21);
CREATE TABLE knows (src INTEGER, dst INTEGER);
INSERT INTO knows VALUES (1, 2), (1, 3), (1, 4), (1, 5), (2, 1), (2, 5), (3, 4);
"#;

/// Those tables as the one relation the engine reads, the way a host states
/// what its tables mean: a branch per class and per column, the edge table
/// joined to its vertices, and the subclass triple as a constant row.
fn view(schema: &str) -> String {
    let rdf_type = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";
    let string = "http://www.w3.org/2001/XMLSchema#string";
    let integer = "http://www.w3.org/2001/XMLSchema#integer";
    let column = |table: &str, column: &str, predicate: &str, lexical: &str, datatype: &str| {
        format!(
            "SELECT 'I', iri, 'http://example.org/{predicate}', 'L', {lexical}, '{datatype}', '' \
             FROM {schema}{table} WHERE {column} IS NOT NULL"
        )
    };
    [
        format!(
            "SELECT 'I' AS s_k, iri AS s_v, '{rdf_type}' AS p, 'I' AS o_k, 'http://example.org/Person' AS o_v, \
             '' AS o_d, '' AS o_l FROM {schema}person"
        ),
        column("person", "name", "name", "name", string),
        column("person", "age", "age", "CAST(age AS VARCHAR)", integer),
        column("person", "email", "email", "email", string),
        format!("SELECT 'I', iri, '{rdf_type}', 'I', 'http://example.org/Student', '', '' FROM {schema}student"),
        column("student", "name", "name", "name", string),
        column("student", "age", "age", "CAST(age AS VARCHAR)", integer),
        format!(
            "SELECT 'I', s.iri, 'http://example.org/knows', 'I', d.iri, '', '' FROM {schema}knows AS k \
             JOIN {schema}node AS s ON k.src = s.id JOIN {schema}node AS d ON k.dst = d.id"
        ),
        "SELECT 'I', 'http://example.org/Student', 'http://www.w3.org/2000/01/rdf-schema#subClassOf', 'I', \
         'http://example.org/Person', '', ''"
            .to_owned(),
    ]
    .join("\nUNION ALL ")
}

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

fn in_memory(data: &OxigraphInMemory, schema: &IRSchema) -> ValidationReport {
    shacl::validator::validate(schema, data).expect("in memory validates")
}

/// Validates through a triples view over the tables, all of them in
/// `db_schema` when one is given.
fn through_tables(schema: &IRSchema, db_schema: Option<&str>) -> ValidationReport {
    let engine = DuckDbEngine::in_memory().expect("duckdb opens");
    let (ddl, prefix) = match db_schema {
        Some(name) => (
            format!(
                "CREATE SCHEMA {name};\n{}",
                TABLES
                    .replace("TABLE ", &format!("TABLE {name}."))
                    .replace("INTO ", &format!("INTO {name}."))
            ),
            format!("{name}."),
        ),
        None => (TABLES.to_owned(), String::new()),
    };
    engine.connection().execute_batch(&ddl).expect("tables load");
    engine
        .connection()
        .execute_batch(&format!("CREATE VIEW {prefix}triples AS {}", view(&prefix)))
        .expect("the view is created");
    block_on(validate(schema, &format!("{prefix}triples"), &engine)).expect("validates")
}

#[test]
fn a_triples_table_and_a_view_over_tables_yield_the_evaluators_report() {
    let data = graph(&format!(
        "@prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .\n{DATA}"
    ));
    let schema = schema(SHAPES);

    let in_memory = in_memory(&data, &schema);
    let triples = validate_with_duckdb(&data, &schema).expect("triple table validates");
    let tables = through_tables(&schema, None);
    let in_a_schema = through_tables(&schema, Some("warehouse"));

    assert!(
        in_memory.results().len() > 15,
        "the dataset violates many components: {in_memory}"
    );
    assert_eq!(
        triples, in_memory,
        "triple table vs in memory\n{triples}\n---\n{in_memory}"
    );
    assert_eq!(tables, in_memory, "tables vs in memory\n{tables}\n---\n{in_memory}");
    assert_eq!(in_a_schema, in_memory, "tables in a schema vs in memory");
}

#[test]
fn messages_are_the_evaluators() {
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
    let in_memory = keyed(&in_memory(&data, &schema));
    let sql = keyed(&validate_with_duckdb(&data, &schema).expect("validates"));
    assert_eq!(sql.len(), in_memory.len());
    for ((key, sql), (_, in_memory)) in sql.iter().zip(&in_memory) {
        assert_eq!(sql, in_memory, "the message of {key}");
    }
}

/// The shapes `report` did not check, by node, with the reason.
fn unchecked(report: &ValidationReport) -> Vec<String> {
    report
        .unchecked()
        .iter()
        .map(|u| format!("{} {}", u.shape, u.reason))
        .collect()
}

/// A shape outside the engine's profile, or one that cannot be decided, is
/// not checked, and says so; the other shapes are, through both
/// interpretations alike.
fn holds_the_rest(shapes: &str, expected: &[&str]) {
    let data = graph(DATA);
    let schema = schema(&format!(
        "{shapes}\nex:Adult a sh:NodeShape ; sh:targetClass ex:Person ;\n    sh:property [ sh:path ex:age ; sh:minInclusive 18 ] ."
    ));
    let in_memory = in_memory(&data, &schema);
    let sql = validate_with_duckdb(&data, &schema).expect("validates");
    assert_eq!(sql, in_memory, "{sql}\n---\n{in_memory}");
    assert_eq!(unchecked(&in_memory), expected);
    assert!(
        in_memory
            .results()
            .iter()
            .any(|r| r.focus_node().to_string().contains("bob")),
        "the other shapes are checked: {in_memory}"
    );
}

#[test]
fn shacl_sparql_is_not_checked_and_the_rest_is() {
    holds_the_rest(
        r#"
ex:S a sh:NodeShape ; sh:targetClass ex:Person ;
    sh:sparql [ sh:select "SELECT $this WHERE { $this ?p ?o }" ] .
ex:T a sh:NodeShape ; sh:targetClass ex:Person ; sh:node ex:S .
"#,
        &[
            "http://example.org/S http://www.w3.org/ns/shacl#SPARQLConstraintComponent is outside the engine's profile",
            "http://example.org/T it depends on http://example.org/S, which is not checked",
        ],
    );
}

#[test]
fn recursive_shapes_are_not_checked_and_the_rest_is() {
    holds_the_rest(
        r#"
ex:S a sh:NodeShape ; sh:targetClass ex:Person ; sh:node ex:S .
"#,
        &["http://example.org/S recursive shapes have no SHACL semantics"],
    );
}

#[test]
fn target_where_selects_the_same_focus_nodes_through_sql() {
    let data = graph(
        r#"
ex:a a ex:Person .
ex:b a ex:Person ; ex:name "B" .
ex:c ex:name "C" .
"#,
    );
    let schema = schema(
        r#"
ex:S a sh:NodeShape ; sh:targetWhere ex:W ; sh:property [ sh:path ex:name ; sh:minCount 1 ] .
ex:W a sh:NodeShape ; sh:class ex:Person .
"#,
    );
    let sql = validate_with_duckdb(&data, &schema).expect("sql validates");
    assert_eq!(sql, in_memory(&data, &schema));
    assert_eq!(sql.results().len(), 1);
}
